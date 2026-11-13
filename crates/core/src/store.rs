//! The local cache: one SQLite file with full-text search over posts and comments.
//!
//! Rules: rows are upserted by id; text of anything deleted or removed is blanked and stays blank;
//! the request log holds no content.

use crate::model::{Comment, MoreStub, Post, State, Subreddit, Thread};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::Path;

pub type Error = rusqlite::Error;
pub type Result<T> = std::result::Result<T, Error>;
/// A cached post with its source, when it was fetched, and when its comments were (if ever).
pub type PostRow = (Post, String, i64, Option<i64>);

const SCHEMA_VERSION: i64 = 3;

/// Version 2 remembers what the archive said about a subreddit, so asking again costs no request.
const SUBREDDIT_TABLE: &str = "CREATE TABLE subreddit(key TEXT PRIMARY KEY, name TEXT NOT NULL, subscribers INTEGER, over_18 INTEGER NOT NULL,
  description TEXT NOT NULL, posts INTEGER, in_archive INTEGER NOT NULL, checked_at INTEGER NOT NULL);";

/// Version 3 makes the request log a machine-wide ledger: every `rdt` process books its request here, to the
/// millisecond, before sending it, so parallel runs pace against each other instead of only against themselves.
/// A booked request whose answer never came stays at status -1 and still counts.
const LEDGER_COLUMNS: &str = "ALTER TABLE request_log ADD COLUMN at_ms INTEGER; CREATE INDEX request_log_at_ms ON request_log(at_ms);";

/// The machine-wide pacing rules a booking follows.
#[derive(Debug, Clone, Copy)]
pub struct Pacing {
    /// Least time between any two requests from this machine.
    pub gap_ms: u64,
    /// At most `cap` requests in any `window_ms`.
    pub cap: u64,
    pub window_ms: u64,
}

/// What the ledger says about the next request.
#[derive(Debug, PartialEq)]
pub enum Booking {
    /// Booked: send at `send_at_ms` (now or later); `id` settles the row once the answer is in.
    Slot { id: i64, send_at_ms: u64 },
    /// Not booked, nothing recorded: the archive said it is rate limiting until then.
    Blocked { until_ms: u64 },
    /// Not booked, nothing recorded: the earliest free slot is further away than the caller will wait.
    Busy { free_at_ms: u64 },
}

/// The machine-wide budget as of one moment.
#[derive(Debug, PartialEq)]
pub struct Budget {
    /// Requests booked in the window that ends now (including any booked for a moment still ahead).
    pub used: u64,
    pub cap: u64,
    pub window_ms: u64,
    /// When the archive's last rate limit ends, if it has not ended yet.
    pub blocked_until_ms: Option<u64>,
    /// When a slot in the window frees up, if the window is full.
    pub frees_at_ms: Option<u64>,
}

impl Budget {
    pub fn left(&self) -> u64 {
        self.cap.saturating_sub(self.used)
    }
}

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE post(
  id TEXT PRIMARY KEY, subreddit TEXT NOT NULL, title TEXT NOT NULL, selftext TEXT NOT NULL, author TEXT,
  score INTEGER, upvote_ratio REAL, num_comments INTEGER, created INTEGER NOT NULL, edited INTEGER,
  permalink TEXT NOT NULL, url TEXT, flair TEXT, crosspost_parent TEXT, over_18 INTEGER NOT NULL, locked INTEGER NOT NULL,
  state TEXT NOT NULL, scores_as_of INTEGER, source TEXT NOT NULL, fetched_at INTEGER NOT NULL, comments_fetched_at INTEGER);
CREATE TABLE comment(
  id TEXT PRIMARY KEY, post_id TEXT NOT NULL, parent_id TEXT, depth INTEGER NOT NULL, ord INTEGER NOT NULL, author TEXT, body TEXT NOT NULL,
  score INTEGER, created INTEGER NOT NULL, edited INTEGER, is_op INTEGER NOT NULL, distinguished TEXT, stickied INTEGER NOT NULL,
  state TEXT NOT NULL, fetched_at INTEGER NOT NULL);
CREATE INDEX comment_post ON comment(post_id, ord);
CREATE TABLE more_stub(post_id TEXT NOT NULL, parent_id TEXT NOT NULL, count INTEGER NOT NULL, children TEXT NOT NULL, PRIMARY KEY(post_id, parent_id));
CREATE TABLE request_log(at INTEGER NOT NULL, source TEXT NOT NULL, endpoint TEXT NOT NULL, status INTEGER NOT NULL);
CREATE VIRTUAL TABLE post_fts USING fts5(title, selftext, content='post', content_rowid='rowid', tokenize='porter unicode61');
CREATE VIRTUAL TABLE comment_fts USING fts5(body, content='comment', content_rowid='rowid', tokenize='porter unicode61');
CREATE TRIGGER post_ai AFTER INSERT ON post BEGIN INSERT INTO post_fts(rowid, title, selftext) VALUES (new.rowid, new.title, new.selftext); END;
CREATE TRIGGER post_ad AFTER DELETE ON post BEGIN INSERT INTO post_fts(post_fts, rowid, title, selftext) VALUES ('delete', old.rowid, old.title, old.selftext); END;
CREATE TRIGGER post_au AFTER UPDATE ON post BEGIN
  INSERT INTO post_fts(post_fts, rowid, title, selftext) VALUES ('delete', old.rowid, old.title, old.selftext);
  INSERT INTO post_fts(rowid, title, selftext) VALUES (new.rowid, new.title, new.selftext); END;
CREATE TRIGGER comment_ai AFTER INSERT ON comment BEGIN INSERT INTO comment_fts(rowid, body) VALUES (new.rowid, new.body); END;
CREATE TRIGGER comment_ad AFTER DELETE ON comment BEGIN INSERT INTO comment_fts(comment_fts, rowid, body) VALUES ('delete', old.rowid, old.body); END;
CREATE TRIGGER comment_au AFTER UPDATE ON comment BEGIN
  INSERT INTO comment_fts(comment_fts, rowid, body) VALUES ('delete', old.rowid, old.body);
  INSERT INTO comment_fts(rowid, body) VALUES (new.rowid, new.body); END;
";

pub struct Store {
    db: Connection,
}

/// A thread as held in the cache.
#[derive(Debug)]
pub struct Cached {
    pub thread: Thread,
    pub source: String,
    pub fetched_at: i64,
    /// `None` when only the post is cached (it came from a search, its comments were never fetched).
    pub comments_fetched_at: Option<i64>,
}

/// A cached post or comment that matched a full-text query.
#[derive(Debug, PartialEq)]
pub struct Match {
    pub post_id: String,
    /// `None` when the post itself matched.
    pub comment_id: Option<String>,
    pub text: String,
}

#[derive(Debug, PartialEq, Default)]
pub struct Stats {
    pub posts: i64,
    pub threads_with_comments: i64,
    pub comments: i64,
    pub oldest_fetch: Option<i64>,
    pub newest_fetch: Option<i64>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        Store::init(Connection::open(path)?)
    }

    pub fn memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Store> {
        // Several rdt processes may share this file; a booking holds the write lock for a moment only.
        db.busy_timeout(std::time::Duration::from_secs(30))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        // Creating or upgrading happens under the write lock, with the version read inside it: two processes opening
        // an old cache at once must not both try the same upgrade.
        {
            let tx = rusqlite::Transaction::new_unchecked(&db, rusqlite::TransactionBehavior::Immediate)?;
            let has_meta: bool = tx.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'meta'", [], |r| r.get::<_, i64>(0)).map(|n| n > 0)?;
            if !has_meta {
                tx.execute_batch(&format!("{SCHEMA} {SUBREDDIT_TABLE} {LEDGER_COLUMNS} INSERT INTO meta VALUES ('schema_version', '{SCHEMA_VERSION}');"))?;
            }
            let version: i64 = tx.query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get::<_, String>(0))?.parse().unwrap_or(0);
            if version == 1 {
                tx.execute_batch(&format!("{SUBREDDIT_TABLE} UPDATE meta SET value = '2' WHERE key = 'schema_version';"))?;
            }
            if version == 1 || version == 2 {
                // One way: a 0.2 build refuses a version 3 cache rather than rewrite it.
                tx.execute_batch(&format!("{LEDGER_COLUMNS} UPDATE meta SET value = '3' WHERE key = 'schema_version';"))?;
            } else if version != SCHEMA_VERSION {
                // A cache from a newer build is refused, never rewritten.
                return Err(rusqlite::Error::InvalidParameterName(format!("cache schema version {version}, this build understands {SCHEMA_VERSION}")));
            }
            tx.commit()?;
        }
        Ok(Store { db })
    }

    pub fn save_post(&self, p: &Post, source: &str, now: i64) -> Result<()> {
        self.db.execute(
            "INSERT INTO post(id, subreddit, title, selftext, author, score, upvote_ratio, num_comments, created, edited, permalink, url, flair,
                              crosspost_parent, over_18, locked, state, scores_as_of, source, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
             ON CONFLICT(id) DO UPDATE SET subreddit = excluded.subreddit, title = excluded.title, selftext = excluded.selftext, author = excluded.author,
               score = excluded.score, upvote_ratio = excluded.upvote_ratio, num_comments = excluded.num_comments, created = excluded.created,
               edited = excluded.edited, permalink = excluded.permalink, url = excluded.url, flair = excluded.flair,
               crosspost_parent = excluded.crosspost_parent, over_18 = excluded.over_18, locked = excluded.locked, state = excluded.state,
               scores_as_of = excluded.scores_as_of, source = excluded.source, fetched_at = excluded.fetched_at",
            params![
                p.id,
                p.subreddit,
                p.title,
                p.selftext,
                p.author,
                p.score,
                p.upvote_ratio,
                p.num_comments,
                p.created,
                p.edited,
                p.permalink,
                p.url,
                p.flair,
                p.crosspost_parent,
                p.over_18,
                p.locked,
                p.state.as_str(),
                p.scores_as_of,
                source,
                now
            ],
        )?;
        Ok(())
    }

    /// Upserts comments after any already cached for the post, and replaces the stubs of the parents they hang under.
    pub fn save_comments(&self, post_id: &str, comments: &[Comment], more: &[MoreStub], now: i64) -> Result<()> {
        let mut ord: i64 = self.db.query_row("SELECT coalesce(max(ord), 0) FROM comment WHERE post_id = ?1", [post_id], |r| r.get(0))?;
        let mut upsert = self.db.prepare_cached(
            "INSERT INTO comment(id, post_id, parent_id, depth, ord, author, body, score, created, edited, is_op, distinguished, stickied, state, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET parent_id = excluded.parent_id, depth = excluded.depth, author = excluded.author, body = excluded.body,
               score = excluded.score, edited = excluded.edited, is_op = excluded.is_op, distinguished = excluded.distinguished,
               stickied = excluded.stickied, state = excluded.state, fetched_at = excluded.fetched_at",
        )?;
        for c in comments {
            ord += 1;
            upsert.execute(params![
                c.id,
                post_id,
                c.parent_id,
                c.depth,
                ord,
                c.author,
                c.body,
                c.score,
                c.created,
                c.edited,
                c.is_op,
                c.distinguished,
                c.stickied,
                c.state.as_str(),
                now
            ])?;
        }
        for m in more {
            self.db.execute(
                "INSERT OR REPLACE INTO more_stub(post_id, parent_id, count, children) VALUES (?1, ?2, ?3, ?4)",
                params![post_id, m.parent_id.as_deref().unwrap_or(""), m.count, m.children.join(",")],
            )?;
        }
        Ok(())
    }

    /// Saves a freshly fetched thread in one transaction.
    pub fn save_thread(&mut self, t: &Thread, source: &str, now: i64) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        self.save_post(&t.post, source, now)?;
        self.db.execute("DELETE FROM more_stub WHERE post_id = ?1", [&t.post.id])?;
        self.save_comments(&t.post.id, &t.comments, &t.more, now)?;
        // A stub whose listed replies are all cached (loaded earlier by an expansion) is no longer "not loaded".
        for m in t.more.iter().filter(|m| !m.children.is_empty()) {
            let mut cached = true;
            for child in &m.children {
                cached &= self.db.query_row("SELECT count(*) FROM comment WHERE id = ?1", [child], |r| r.get::<_, i64>(0))? > 0;
            }
            if cached {
                self.db.execute("DELETE FROM more_stub WHERE post_id = ?1 AND parent_id = ?2", params![t.post.id, m.parent_id.as_deref().unwrap_or("")])?;
            }
        }
        // When the source returned the whole tree, a cached comment it no longer returns has gone: blank it.
        if t.more.is_empty() {
            self.db.execute(
                "UPDATE comment SET body = '', author = NULL, state = 'removed' WHERE post_id = ?1 AND fetched_at < ?2 AND state = 'live'",
                params![t.post.id, now],
            )?;
        }
        self.db.execute("UPDATE post SET comments_fetched_at = ?2 WHERE id = ?1", params![t.post.id, now])?;
        tx.commit()
    }

    /// The source no longer has this thread: blank the post and every comment, keeping only placeholders.
    pub fn tombstone_thread(&mut self, post_id: &str) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        self.tombstone_post(post_id, State::Removed)?;
        self.db.execute("UPDATE comment SET body = '', author = NULL, state = 'removed' WHERE post_id = ?1 AND state = 'live'", [post_id])?;
        tx.commit()
    }

    /// Remembers the archive's answer about a subreddit name; `None` means it said there is no such subreddit.
    pub fn save_subreddit(&self, key: &str, info: Option<&Subreddit>, now: i64) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO subreddit(key, name, subscribers, over_18, description, posts, in_archive, checked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                key.to_lowercase(),
                info.map_or(key, |i| i.name.as_str()),
                info.and_then(|i| i.subscribers),
                info.is_some_and(|i| i.over_18),
                info.map_or("", |i| i.description.as_str()),
                info.and_then(|i| i.posts),
                info.is_some(),
                now
            ],
        )?;
        Ok(())
    }

    /// A remembered answer: `Some(None)` is a remembered "no such subreddit". A found subreddit is trusted back to
    /// `since`, a missing one only back to `since_missing`, because the archive's list lags and a wrong "no" should not stick.
    pub fn subreddit(&self, key: &str, since: i64, since_missing: i64) -> Result<Option<Option<Subreddit>>> {
        self.db
            .query_row(
                "SELECT name, subscribers, over_18, description, posts, in_archive FROM subreddit
                 WHERE key = ?1 AND checked_at >= CASE WHEN in_archive THEN ?2 ELSE ?3 END",
                params![key.to_lowercase(), since, since_missing],
                |r| {
                    let found: bool = r.get(5)?;
                    Ok(found.then_some(Subreddit { name: r.get(0)?, subscribers: r.get(1)?, over_18: r.get(2)?, description: r.get(3)?, posts: r.get(4)? }))
                },
            )
            .optional()
    }

    /// Cached threads that have comments, most recently fetched first, each with its comment count.
    pub fn recent_threads(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        self.db
            .prepare(
                "SELECT p.id, (SELECT count(*) FROM comment c WHERE c.post_id = p.id) FROM post p
                 WHERE p.comments_fetched_at IS NOT NULL ORDER BY p.comments_fetched_at DESC, p.id LIMIT ?1",
            )?
            .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    }

    /// When the archive was last asked for anything, so separate runs can keep their distance.
    pub fn last_request_at(&self) -> Result<Option<i64>> {
        self.db.query_row("SELECT max(at) FROM request_log", [], |r| r.get(0))
    }

    /// Records that the replies under `parent` are now loaded.
    pub fn save_expansion(&mut self, post_id: &str, parent: &str, comments: &[Comment], more: &[MoreStub], now: i64) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        self.db.execute("DELETE FROM more_stub WHERE post_id = ?1 AND parent_id = ?2", params![post_id, parent])?;
        self.save_comments(post_id, comments, more, now)?;
        tx.commit()
    }

    pub fn post(&self, id: &str) -> Result<Option<PostRow>> {
        self.db
            .query_row(
                "SELECT id, subreddit, title, selftext, author, score, upvote_ratio, num_comments, created, edited, permalink, url, flair,
                        crosspost_parent, over_18, locked, state, scores_as_of, source, fetched_at, comments_fetched_at FROM post WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        Post {
                            id: r.get(0)?,
                            subreddit: r.get(1)?,
                            title: r.get(2)?,
                            selftext: r.get(3)?,
                            author: r.get(4)?,
                            score: r.get(5)?,
                            upvote_ratio: r.get(6)?,
                            num_comments: r.get(7)?,
                            created: r.get(8)?,
                            edited: r.get(9)?,
                            permalink: r.get(10)?,
                            url: r.get(11)?,
                            flair: r.get(12)?,
                            crosspost_parent: r.get(13)?,
                            over_18: r.get(14)?,
                            locked: r.get(15)?,
                            state: State::parse(&r.get::<_, String>(16)?),
                            scores_as_of: r.get(17)?,
                        },
                        r.get(18)?,
                        r.get(19)?,
                        r.get(20)?,
                    ))
                },
            )
            .optional()
    }

    pub fn comment(&self, id: &str) -> Result<Option<Comment>> {
        self.db.query_row(&format!("{COMMENT_COLS} WHERE id = ?1"), [id], row_comment).optional()
    }

    pub fn thread(&self, id: &str) -> Result<Option<Cached>> {
        let Some((post, source, fetched_at, comments_fetched_at)) = self.post(id)? else { return Ok(None) };
        let flat: Vec<Comment> =
            self.db.prepare(&format!("{COMMENT_COLS} WHERE post_id = ?1 ORDER BY ord"))?.query_map([id], row_comment)?.collect::<Result<_>>()?;
        let more = self
            .db
            .prepare("SELECT parent_id, count, children FROM more_stub WHERE post_id = ?1")?
            .query_map([id], |r| {
                let (parent, children): (String, String) = (r.get(0)?, r.get(2)?);
                Ok(MoreStub {
                    parent_id: Some(parent).filter(|p| !p.is_empty()),
                    count: r.get(1)?,
                    children: children.split(',').filter(|c| !c.is_empty()).map(str::to_owned).collect(),
                })
            })?
            .collect::<Result<_>>()?;
        Ok(Some(Cached { thread: Thread { post, comments: preorder(flat), more }, source, fetched_at, comments_fetched_at }))
    }

    /// Blanks the text and author of something now known to be deleted or removed. The row stays so the tree holds together.
    pub fn tombstone_post(&self, id: &str, state: State) -> Result<usize> {
        self.db.execute(
            "UPDATE post SET title = '[' || ?2 || ']', selftext = '', author = NULL, url = NULL, flair = NULL, state = ?2 WHERE id = ?1",
            params![id, state.as_str()],
        )
    }

    pub fn tombstone_comment(&self, id: &str, state: State) -> Result<usize> {
        self.db.execute("UPDATE comment SET body = '', author = NULL, state = ?2 WHERE id = ?1", params![id, state.as_str()])
    }

    /// Full-text retrieval only; scoring happens in `rank`. Each term is quoted, so query text can never be read as FTS syntax.
    pub fn matches(&self, terms: &[String], limit: usize) -> Result<Vec<Match>> {
        let query = terms.iter().map(|t| format!("\"{}\"", t.replace('"', " "))).collect::<Vec<_>>().join(" OR ");
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut out: Vec<Match> = self
            .db
            .prepare("SELECT p.id, p.title || char(10) || p.selftext FROM post_fts JOIN post p ON p.rowid = post_fts.rowid WHERE post_fts MATCH ?1 ORDER BY rank LIMIT ?2")?
            .query_map(params![query, limit as i64], |r| Ok(Match { post_id: r.get(0)?, comment_id: None, text: r.get(1)? }))?
            .collect::<Result<_>>()?;
        let comments: Vec<Match> = self
            .db
            .prepare("SELECT c.post_id, c.id, c.body FROM comment_fts JOIN comment c ON c.rowid = comment_fts.rowid WHERE comment_fts MATCH ?1 ORDER BY rank LIMIT ?2")?
            .query_map(params![query, (limit * 5) as i64], |r| Ok(Match { post_id: r.get(0)?, comment_id: Some(r.get(1)?), text: r.get(2)? }))?
            .collect::<Result<_>>()?;
        out.extend(comments);
        Ok(out)
    }

    pub fn stats(&self) -> Result<Stats> {
        let (posts, with_comments, oldest, newest) =
            self.db.query_row("SELECT count(*), count(comments_fetched_at), min(fetched_at), max(fetched_at) FROM post", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        let comments = self.db.query_row("SELECT count(*) FROM comment", [], |r| r.get(0))?;
        Ok(Stats { posts, threads_with_comments: with_comments, comments, oldest_fetch: oldest, newest_fetch: newest })
    }

    pub fn forget(&mut self, post_id: &str) -> Result<usize> {
        let tx = self.db.unchecked_transaction()?;
        self.db.execute("DELETE FROM comment WHERE post_id = ?1", [post_id])?;
        self.db.execute("DELETE FROM more_stub WHERE post_id = ?1", [post_id])?;
        let n = self.db.execute("DELETE FROM post WHERE id = ?1", [post_id])?;
        tx.commit()?;
        Ok(n)
    }

    /// Deletes every thread last fetched before `cutoff`. Returns how many posts went.
    pub fn purge_before(&mut self, cutoff: i64) -> Result<usize> {
        let tx = self.db.unchecked_transaction()?;
        self.db.execute("DELETE FROM comment WHERE post_id IN (SELECT id FROM post WHERE fetched_at < ?1)", [cutoff])?;
        self.db.execute("DELETE FROM more_stub WHERE post_id IN (SELECT id FROM post WHERE fetched_at < ?1)", [cutoff])?;
        let n = self.db.execute("DELETE FROM post WHERE fetched_at < ?1", [cutoff])?;
        self.db.execute("DELETE FROM request_log WHERE at < ?1", [cutoff])?;
        tx.commit()?;
        Ok(n)
    }

    /// Ids of live posts and comments, for re-checking against the source.
    pub fn live_ids(&self) -> Result<(Vec<String>, Vec<String>)> {
        let ids = |sql: &str| -> Result<Vec<String>> { self.db.prepare(sql)?.query_map([], |r| r.get(0))?.collect() };
        Ok((ids("SELECT id FROM post WHERE state = 'live'")?, ids("SELECT id FROM comment WHERE state = 'live'")?))
    }

    pub fn log_request(&self, now: i64, source: &str, endpoint: &str, status: u16) -> Result<()> {
        self.db.execute(
            "INSERT INTO request_log(at, source, endpoint, status, at_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now, source, endpoint, status, now.saturating_mul(1000)],
        )?;
        Ok(())
    }

    /// Books the next request from this machine under `pacing`, inside one write transaction so two processes
    /// can never take the same slot. A wait longer than `max_wait_ms` is not booked: nothing is recorded and the
    /// caller is told when to come back. Times are milliseconds since the Unix epoch.
    pub fn book_request(&self, now_ms: u64, source: &str, endpoint: &str, pacing: Pacing, max_wait_ms: u64) -> Result<Booking> {
        self.book_request_with(&|| now_ms, source, endpoint, pacing, max_wait_ms)
    }

    /// The same, reading the clock only once the write lock is held: a process that waited for the lock must not
    /// book from the moment it started waiting, or its slot would sit closer than the gap to the next one.
    pub fn book_request_with(&self, clock: &dyn Fn() -> u64, source: &str, endpoint: &str, pacing: Pacing, max_wait_ms: u64) -> Result<Booking> {
        let tx = rusqlite::Transaction::new_unchecked(&self.db, rusqlite::TransactionBehavior::Immediate)?;
        let now_ms = clock();
        if let Some(until) = self.blocked_until(now_ms)? {
            return Ok(Booking::Blocked { until_ms: until });
        }
        let last: Option<i64> = tx.query_row("SELECT max(at_ms) FROM request_log", [], |r| r.get(0))?;
        let nth: Option<i64> = tx
            .query_row("SELECT at_ms FROM request_log WHERE at_ms IS NOT NULL ORDER BY at_ms DESC LIMIT 1 OFFSET ?1", [pacing.cap.max(1) as i64 - 1], |r| {
                r.get(0)
            })
            .optional()?;
        let mut slot = now_ms;
        if let Some(last) = last {
            slot = slot.max((last.max(0) as u64).saturating_add(pacing.gap_ms));
        }
        if let Some(nth) = nth {
            slot = slot.max((nth.max(0) as u64).saturating_add(pacing.window_ms));
        }
        if slot - now_ms > max_wait_ms {
            return Ok(Booking::Busy { free_at_ms: slot });
        }
        tx.execute(
            "INSERT INTO request_log(at, source, endpoint, status, at_ms) VALUES (?1, ?2, ?3, -1, ?4)",
            params![(slot / 1000) as i64, source, endpoint, slot as i64],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Booking::Slot { id, send_at_ms: slot })
    }

    /// Records the answer to a booked request (0: no response at all).
    pub fn settle_request(&self, id: i64, status: u16) -> Result<()> {
        self.db.execute("UPDATE request_log SET status = ?1 WHERE rowid = ?2", params![status, id])?;
        Ok(())
    }

    /// Remembers, for every process on this machine, that the archive is rate limiting until then.
    /// A later answer never shortens an earlier, longer one.
    pub fn block_until(&self, until_ms: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta VALUES ('archive_blocked_until_ms', ?1)
             ON CONFLICT(key) DO UPDATE SET value = CAST(max(CAST(value AS INTEGER), CAST(excluded.value AS INTEGER)) AS TEXT)",
            [until_ms.to_string()],
        )?;
        Ok(())
    }

    pub fn blocked_until(&self, now_ms: u64) -> Result<Option<u64>> {
        let v: Option<String> = self.db.query_row("SELECT value FROM meta WHERE key = 'archive_blocked_until_ms'", [], |r| r.get(0)).optional()?;
        Ok(v.and_then(|v| v.parse::<u64>().ok()).filter(|&t| t > now_ms))
    }

    /// The machine-wide budget at `now_ms`: bookings in the window that ends now, plus any booked for later.
    pub fn budget(&self, now_ms: u64, pacing: Pacing) -> Result<Budget> {
        let start = now_ms.saturating_sub(pacing.window_ms) as i64;
        let times: Vec<i64> =
            self.db.prepare("SELECT at_ms FROM request_log WHERE at_ms > ?1 ORDER BY at_ms")?.query_map([start], |r| r.get(0))?.collect::<Result<_>>()?;
        let used = times.len() as u64;
        let frees_at_ms = (used >= pacing.cap && pacing.cap > 0).then(|| times[(used - pacing.cap) as usize].max(0) as u64 + pacing.window_ms);
        Ok(Budget { used, cap: pacing.cap, window_ms: pacing.window_ms, blocked_until_ms: self.blocked_until(now_ms)?, frees_at_ms })
    }

    pub fn requests_since(&self, since: i64) -> Result<i64> {
        self.db.query_row("SELECT count(*) FROM request_log WHERE at >= ?1", [since], |r| r.get(0))
    }
}

const COMMENT_COLS: &str = "SELECT id, post_id, parent_id, depth, author, body, score, created, edited, is_op, distinguished, stickied, state FROM comment";

fn row_comment(r: &rusqlite::Row) -> Result<Comment> {
    Ok(Comment {
        id: r.get(0)?,
        post_id: r.get(1)?,
        parent_id: r.get(2)?,
        depth: r.get(3)?,
        author: r.get(4)?,
        body: r.get(5)?,
        score: r.get(6)?,
        created: r.get(7)?,
        edited: r.get(8)?,
        is_op: r.get(9)?,
        distinguished: r.get(10)?,
        stickied: r.get(11)?,
        state: State::parse(&r.get::<_, String>(12)?),
    })
}

/// Rebuilds pre-order from parent links, keeping siblings in stored order. A comment whose parent is not
/// cached is shown at the top level rather than lost. Depth is recomputed from the tree as rebuilt.
fn preorder(flat: Vec<Comment>) -> Vec<Comment> {
    let known: std::collections::HashSet<&str> = flat.iter().map(|c| c.id.as_str()).collect();
    let mut children: HashMap<Option<String>, Vec<usize>> = HashMap::new();
    for (i, c) in flat.iter().enumerate() {
        let parent = c.parent_id.clone().filter(|p| known.contains(p.as_str()) && *p != c.id);
        children.entry(parent).or_default().push(i);
    }
    let self_parented: Vec<bool> = flat.iter().map(|c| c.parent_id.as_deref() == Some(c.id.as_str())).collect();
    let mut order = Vec::with_capacity(flat.len());
    let mut stack: Vec<(usize, u32)> = children.get(&None).map(|v| v.iter().rev().map(|i| (*i, 0)).collect()).unwrap_or_default();
    let mut seen = vec![false; flat.len()];
    while let Some((i, depth)) = stack.pop() {
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        order.push((i, depth));
        if let Some(kids) = children.get(&Some(flat[i].id.clone())) {
            stack.extend(kids.iter().rev().map(|k| (*k, depth + 1)));
        }
    }
    // Anything not reached sits in a parent loop: surface it at the top level with the loop cut, never drop it.
    let looped: Vec<usize> = (0..flat.len()).filter(|i| !seen[*i]).collect();
    order.extend(looped.iter().map(|i| (*i, 0)));
    let mut slots: Vec<Option<Comment>> = flat.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|(i, depth)| {
            slots[i].take().map(|mut c| {
                c.depth = depth;
                if self_parented[i] || looped.contains(&i) {
                    c.parent_id = None;
                }
                c
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn post(id: &str, title: &str, selftext: &str) -> Post {
        Post {
            id: id.into(),
            subreddit: "sub".into(),
            title: title.into(),
            selftext: selftext.into(),
            author: Some("op".into()),
            score: Some(10),
            upvote_ratio: Some(0.9),
            num_comments: Some(2),
            created: 1_700_000_000,
            edited: None,
            permalink: format!("/r/sub/comments/{id}/slug/"),
            url: None,
            flair: None,
            crosspost_parent: None,
            over_18: false,
            locked: false,
            state: State::Live,
            scores_as_of: None,
        }
    }

    pub(crate) fn comment(id: &str, parent: Option<&str>, depth: u32, body: &str, score: i64) -> Comment {
        Comment {
            id: id.into(),
            post_id: "p1".into(),
            parent_id: parent.map(Into::into),
            depth,
            author: Some(format!("u_{id}")),
            body: body.into(),
            score: Some(score),
            created: 1_700_000_100,
            edited: None,
            is_op: false,
            distinguished: None,
            stickied: false,
            state: State::Live,
        }
    }

    fn thread() -> Thread {
        Thread {
            post: post("p1", "B650E-E boot loop after EXPO", "Board will not POST."),
            comments: vec![
                comment("c1", None, 0, "Update the BIOS first.", 5),
                comment("c2", Some("c1"), 1, "Memory training takes minutes.", 3),
                comment("c3", None, 0, "Reseat the RAM.", 1),
            ],
            more: vec![MoreStub { parent_id: Some("c3".into()), count: 4, children: vec!["c9".into()] }],
        }
    }

    #[test]
    fn round_trip_and_upsert() {
        let mut s = Store::memory().unwrap();
        let t = thread();
        s.save_thread(&t, "arctic", 100).unwrap();
        s.save_thread(&t, "arctic", 200).unwrap();
        let c = s.thread("p1").unwrap().unwrap();
        assert_eq!(c.thread, t);
        assert_eq!((c.fetched_at, c.comments_fetched_at, c.source.as_str()), (200, Some(200), "arctic"));
        assert_eq!(s.stats().unwrap(), Stats { posts: 1, threads_with_comments: 1, comments: 3, oldest_fetch: Some(200), newest_fetch: Some(200) });
    }

    #[test]
    fn post_from_search_does_not_claim_comments_and_keeps_them_on_refresh() {
        let mut s = Store::memory().unwrap();
        s.save_post(&post("p1", "t", "b"), "arctic", 50).unwrap();
        assert_eq!(s.thread("p1").unwrap().unwrap().comments_fetched_at, None);
        s.save_thread(&thread(), "arctic", 100).unwrap();
        s.save_post(&post("p1", "t", "b"), "arctic", 300).unwrap();
        let c = s.thread("p1").unwrap().unwrap();
        assert_eq!((c.comments_fetched_at, c.thread.comments.len()), (Some(100), 3));
    }

    #[test]
    fn full_text_finds_stems_and_hyphenated_identifiers_and_ignores_syntax() {
        let mut s = Store::memory().unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        let find = |q: &str| s.matches(&crate::text::terms(q), 10).unwrap();
        assert_eq!(find("b650e-e").len(), 1);
        assert_eq!(find("\"boot loop\"")[0].comment_id, None);
        assert_eq!(find("trained")[0].comment_id.as_deref(), Some("c2"), "porter stemming matches training");
        assert!(find("nothinghere").is_empty());
        assert!(find("AND ) (\" NEAR *").is_empty(), "query syntax characters are data, never an error");
    }

    #[test]
    fn tombstone_blanks_text_and_search_forgets_it() {
        let mut s = Store::memory().unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        assert_eq!(s.tombstone_comment("c2", State::Deleted).unwrap(), 1);
        let c = s.comment("c2").unwrap().unwrap();
        assert_eq!((c.body.as_str(), c.author, c.state), ("", None, State::Deleted));
        assert!(s.matches(&["training".into()], 10).unwrap().is_empty());
        assert_eq!(s.thread("p1").unwrap().unwrap().thread.comments.len(), 3, "the row stays as a placeholder");
        assert_eq!(s.live_ids().unwrap().1, ["c1", "c3"]);
    }

    #[test]
    fn a_refetch_that_reports_removal_blanks_the_cached_text() {
        let mut s = Store::memory().unwrap();
        let mut t = thread();
        s.save_thread(&t, "arctic", 100).unwrap();
        t.comments[0].state = State::Removed;
        t.comments[0].body.clear();
        t.comments[0].author = None;
        s.save_thread(&t, "arctic", 200).unwrap();
        assert_eq!(s.comment("c1").unwrap().unwrap().body, "");
        assert!(s.matches(&["bios".into()], 10).unwrap().is_empty());
    }

    #[test]
    fn expansion_adds_replies_under_their_parent_and_clears_the_stub() {
        let mut s = Store::memory().unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        s.save_expansion("p1", "c3", &[comment("c9", Some("c3"), 1, "Did that, no change.", 2)], &[], 150).unwrap();
        let t = s.thread("p1").unwrap().unwrap().thread;
        assert_eq!(t.comments.iter().map(|c| (c.id.as_str(), c.depth)).collect::<Vec<_>>(), [("c1", 0), ("c2", 1), ("c3", 0), ("c9", 1)]);
        assert!(t.more.is_empty());
    }

    #[test]
    fn orphans_surface_at_top_level() {
        let out = preorder(vec![comment("x", Some("missing"), 3, "orphan", 1), comment("y", Some("x"), 4, "child", 1)]);
        assert_eq!(out.iter().map(|c| (c.id.as_str(), c.depth)).collect::<Vec<_>>(), [("x", 0), ("y", 1)]);
    }

    #[test]
    fn parent_loops_are_cut_not_lost() {
        let out = preorder(vec![comment("a", Some("b"), 0, "one", 1), comment("b", Some("a"), 0, "two", 1), comment("c", Some("c"), 0, "three", 1)]);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|c| c.parent_id.is_none() && c.depth == 0));
    }

    #[test]
    fn a_complete_refetch_blanks_comments_the_source_no_longer_returns() {
        let mut s = Store::memory().unwrap();
        let mut t = thread();
        t.more.clear();
        s.save_thread(&t, "arctic", 100).unwrap();
        t.comments.remove(1);
        s.save_thread(&t, "arctic", 200).unwrap();
        let c2 = s.comment("c2").unwrap().unwrap();
        assert_eq!((c2.body.as_str(), c2.author, c2.state), ("", None, State::Removed));
        assert!(s.matches(&["training".into()], 10).unwrap().is_empty());
        // A partial tree (collapsed branches) proves nothing about what is missing.
        let mut partial = thread();
        partial.comments.remove(0);
        s.save_thread(&partial, "arctic", 300).unwrap();
        assert_eq!(s.comment("c1").unwrap().unwrap().state, State::Live);
    }

    #[test]
    fn a_stub_for_a_branch_already_expanded_is_not_counted_again() {
        let mut s = Store::memory().unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        s.save_expansion("p1", "c3", &[comment("c9", Some("c3"), 1, "Did that.", 2)], &[], 150).unwrap();
        s.save_thread(&thread(), "arctic", 200).unwrap();
        assert!(s.thread("p1").unwrap().unwrap().thread.more.is_empty());
    }

    #[test]
    fn a_gone_thread_keeps_nothing_readable() {
        let mut s = Store::memory().unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        s.tombstone_thread("p1").unwrap();
        let t = s.thread("p1").unwrap().unwrap().thread;
        assert_eq!((t.post.title.as_str(), t.post.selftext.as_str(), t.post.author, t.post.flair, t.post.url), ("[removed]", "", None, None, None));
        assert!(t.comments.iter().all(|c| c.body.is_empty() && c.author.is_none() && c.state == State::Removed));
        assert!(s.matches(&["b650e-e".into(), "bios".into(), "boot".into()], 10).unwrap().is_empty());
        assert_eq!(s.last_request_at().unwrap(), None);
    }

    #[test]
    fn a_version_1_cache_is_upgraded_in_place_and_keeps_its_threads() {
        let dir = std::env::temp_dir().join(format!("rdt-test-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.sqlite");
        let mut s = Store::open(&path).unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        drop(s);
        // Make it look like a cache written by version 1: no subreddit table.
        Connection::open(&path).unwrap().execute_batch("DROP TABLE subreddit; DROP INDEX request_log_at_ms; ALTER TABLE request_log DROP COLUMN at_ms; UPDATE meta SET value = '1' WHERE key = 'schema_version';").unwrap();
        let s = Store::open(&path).unwrap();
        assert_eq!(s.thread("p1").unwrap().unwrap().thread.comments.len(), 3);
        assert_eq!(s.subreddit("anything", 0, 0).unwrap(), None);
        drop(s);
        drop(Store::open(&path).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn subreddit_answers_are_remembered_until_they_age_out() {
        let s = Store::memory().unwrap();
        let info = Subreddit { name: "AMDHelp".into(), subscribers: Some(139_317), over_18: false, description: "help".into(), posts: Some(182_867) };
        s.save_subreddit("amdhelp", Some(&info), 100).unwrap();
        s.save_subreddit("Ghost_Sub", None, 100).unwrap();
        assert_eq!(s.subreddit("AMDHelp", 50, 150).unwrap(), Some(Some(info)), "a found subreddit is trusted for the long window");
        assert_eq!(s.subreddit("ghost_sub", 50, 50).unwrap(), Some(None), "a remembered answer that there is no such subreddit");
        assert_eq!(s.subreddit("ghost_sub", 50, 150).unwrap(), None, "a missing one is trusted only for the short window");
        assert_eq!(s.subreddit("amdhelp", 101, 101).unwrap(), None, "too old to trust");
        assert_eq!(s.subreddit("never_asked", 0, 0).unwrap(), None);
    }

    #[test]
    fn purge_and_forget() {
        let mut s = Store::memory().unwrap();
        s.save_thread(&thread(), "arctic", 100).unwrap();
        s.save_post(&post("p2", "newer", ""), "arctic", 500).unwrap();
        assert_eq!(s.purge_before(200).unwrap(), 1);
        assert_eq!(s.stats().unwrap().comments, 0);
        assert_eq!(s.forget("p2").unwrap(), 1);
        assert_eq!(s.stats().unwrap().posts, 0);
        assert!(s.matches(&["newer".into()], 10).unwrap().is_empty());
    }

    #[test]
    fn request_log_counts() {
        let s = Store::memory().unwrap();
        s.log_request(100, "arctic", "posts/ids", 200).unwrap();
        s.log_request(200, "arctic", "comments/tree", 429).unwrap();
        assert_eq!(s.requests_since(150).unwrap(), 1);
    }

    const PACE: Pacing = Pacing { gap_ms: 2000, cap: 3, window_ms: 60_000 };

    fn shared_file(tag: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rdt-test-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("c.sqlite")
    }

    #[test]
    fn two_processes_never_take_the_same_slot() {
        let path = shared_file(line!());
        // Two connections to one file stand for two rdt processes started at the same moment.
        let (a, b) = (Store::open(&path).unwrap(), Store::open(&path).unwrap());
        let now = 1_794_547_000_000;
        let Booking::Slot { id, send_at_ms: first } = a.book_request(now, "arctic", "posts/search", PACE, 90_000).unwrap() else { panic!() };
        let Booking::Slot { send_at_ms: second, .. } = b.book_request(now, "arctic", "posts/search", PACE, 90_000).unwrap() else { panic!() };
        assert_eq!((first, second), (now, now + 2000), "the second process waits out the machine-wide gap");
        a.settle_request(id, 200).unwrap();
        let Booking::Slot { send_at_ms: third, .. } = a.book_request(now, "arctic", "x", PACE, 90_000).unwrap() else { panic!() };
        assert_eq!(third, now + 4000);
        // The window holds 3: the fourth waits until the first leaves it, and a caller unwilling to wait books nothing.
        assert_eq!(b.book_request(now + 5000, "arctic", "x", PACE, 10_000).unwrap(), Booking::Busy { free_at_ms: now + 60_000 });
        assert_eq!(b.budget(now + 5000, PACE).unwrap().used, 3, "a refused booking leaves no row");
        let Booking::Slot { send_at_ms: fourth, .. } = b.book_request(now + 5000, "arctic", "x", PACE, 90_000).unwrap() else { panic!() };
        assert_eq!(fourth, now + 60_000);
        drop((a, b));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_rate_limit_is_shared_and_never_shortened() {
        let path = shared_file(line!());
        let (a, b) = (Store::open(&path).unwrap(), Store::open(&path).unwrap());
        a.block_until(50_000).unwrap();
        a.block_until(20_000).unwrap();
        assert_eq!(b.blocked_until(10_000).unwrap(), Some(50_000));
        assert_eq!(b.book_request(10_000, "arctic", "x", PACE, 90_000).unwrap(), Booking::Blocked { until_ms: 50_000 });
        assert_eq!(b.budget(10_000, PACE).unwrap().used, 0, "a blocked request is not booked");
        assert_eq!(b.blocked_until(50_000).unwrap(), None, "over once the moment passes");
        assert!(matches!(b.book_request(50_000, "arctic", "x", PACE, 0).unwrap(), Booking::Slot { send_at_ms: 50_000, .. }));
        drop((a, b));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn budget_counts_the_window_and_says_when_it_frees() {
        let s = Store::memory().unwrap();
        for t in [1_000u64, 3_000, 5_000] {
            assert!(matches!(s.book_request(t, "arctic", "x", PACE, 0).unwrap(), Booking::Slot { .. }));
        }
        let b = s.budget(6_000, PACE).unwrap();
        assert_eq!((b.used, b.left(), b.frees_at_ms, b.blocked_until_ms), (3, 0, Some(61_000), None));
        let later = s.budget(62_000, PACE).unwrap();
        assert_eq!((later.used, later.left(), later.frees_at_ms), (2, 1, None));
    }

    #[test]
    fn a_version_2_cache_gains_the_ledger_and_keeps_its_log() {
        let path = shared_file(line!());
        drop(Store::open(&path).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "DROP INDEX request_log_at_ms; ALTER TABLE request_log DROP COLUMN at_ms; INSERT INTO request_log VALUES (100, 'arctic', 'posts/ids', 200);
                 UPDATE meta SET value = '2' WHERE key = 'schema_version';",
            )
            .unwrap();
        let s = Store::open(&path).unwrap();
        assert_eq!(s.requests_since(0).unwrap(), 1);
        assert!(matches!(s.book_request(5_000_000, "arctic", "x", PACE, 0).unwrap(), Booking::Slot { .. }));
        assert_eq!(s.requests_since(0).unwrap(), 2);
        drop(s);
        drop(Store::open(&path).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_newer_cache_is_refused_not_rewritten() {
        let dir = std::env::temp_dir().join(format!("rdt-test-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.sqlite");
        drop(Store::open(&path).unwrap());
        Connection::open(&path).unwrap().execute("UPDATE meta SET value = '99' WHERE key = 'schema_version'", []).unwrap();
        assert!(Store::open(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
