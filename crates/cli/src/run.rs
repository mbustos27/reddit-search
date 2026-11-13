//! What each command does. Generic over the transport so every flow is tested against recorded responses.

use reddit_core::ids::{self, Ref};
use reddit_core::model::State;
use reddit_core::rank::{self, Candidate};
use reddit_core::render::{self, SearchMeta, ThreadView};
use reddit_core::select::{self, Options};
use reddit_core::store::{Budget, Store};
use reddit_core::text;
use reddit_sources::arctic::{self, Arctic, Error, Query};
use reddit_sources::transport::{Clock, Transport};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Path;

/// A cached thread is reused without asking the archive again for this long.
const FRESH_SECS: i64 = 6 * 3600;
/// Threads not fetched or re-checked for this long are deleted when the tool next runs.
pub const KEEP_DAYS: i64 = 30;
const MAX_SUBS: usize = 5;

#[derive(Debug, PartialEq)]
pub struct Fail {
    pub code: i32,
    pub message: String,
    pub retry_after_s: Option<u64>,
    /// What a search covered before it stopped, for `--json`.
    pub coverage: Option<serde_json::Value>,
}

impl Fail {
    pub fn new(code: i32, message: impl Into<String>) -> Fail {
        Fail { code, message: message.into(), retry_after_s: None, coverage: None }
    }
}

/// A line for stderr, shown as it happens. Tests stay quiet.
fn warn(line: &str) {
    if !cfg!(test) {
        eprintln!("rdt: {line}");
    }
}

/// Said before a search sends anything, when the machine-wide budget cannot carry it at full pace.
pub fn budget_warning(b: &Budget, planned: usize, now_ms: u64) -> Option<String> {
    let secs = |at: u64| at.saturating_sub(now_ms).div_ceil(1000);
    if let Some(until) = b.blocked_until_ms {
        return Some(format!(
            "rate limited, retry in {} s: the archive limited an rdt run on this PC; nothing is sent until then (with --wait, up to 90 s is waited out)",
            secs(until)
        ));
    }
    if (b.left() as usize) < planned {
        let free = b.frees_at_ms.map(|t| format!("; the next slot frees in {} s", secs(t))).unwrap_or_default();
        return Some(format!(
            "budget low: {} of {} requests left this minute, shared by every rdt run on this PC; this search needs about {planned}, so it will pause for slots{free}",
            b.left(),
            b.cap
        ));
    }
    None
}

/// The coverage status of a subreddit whose search failed.
fn status_of(e: &Error) -> &'static str {
    match e {
        Error::TooBroad(_) => "timed_out",
        Error::RateLimited { .. } => "rate_limited",
        Error::Busy { .. } => "budget_used_up",
        _ => "failed",
    }
}

#[derive(Debug, PartialEq)]
pub enum Narrow {
    /// Search only from this date on; the reason, for the report.
    After(String, String),
    /// Search for fewer words.
    Text(String, String),
}

/// How to retry a query the archive timed out on, narrower: first a shorter date window (the last 180 days, or the
/// newer half of the given window while it is longer than 30 days), then only the first two words.
pub fn narrower(query: &str, after: Option<&str>, before: Option<&str>, now: i64) -> Option<Narrow> {
    const DAY: i64 = 86_400;
    let end = before.and_then(text::epoch).unwrap_or(now);
    let start = after.and_then(text::epoch);
    let from = match start {
        None => Some(end - 180 * DAY),
        Some(s) if end - s > 30 * DAY => Some(s + (end - s) / 2),
        Some(_) => None,
    };
    if let Some(from) = from {
        let d = text::date(from);
        return Some(Narrow::After(d.clone(), format!("searched only from {d}; the older part of the range was dropped")));
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.len() > 2 && !query.contains('"') {
        let kept = words[..2].join(" ");
        return Some(Narrow::Text(kept.clone(), format!("searched for \"{kept}\" only; dropped \"{}\"", words[2..].join(" "))));
    }
    None
}

impl From<reddit_core::store::Error> for Fail {
    fn from(e: reddit_core::store::Error) -> Fail {
        Fail::new(6, format!("cache error: {e}"))
    }
}

impl From<Error> for Fail {
    fn from(e: Error) -> Fail {
        let (code, retry) = match &e {
            Error::NotFound(_) => (3, None),
            Error::RateLimited { retry_after_s } | Error::Busy { retry_after_s } => (5, Some(*retry_after_s)),
            Error::Unsupported(_) => (2, None),
            Error::Refused(_) | Error::TooBroad(_) | Error::Unavailable(_) => (6, None),
        };
        Fail { code, message: e.to_string(), retry_after_s: retry, coverage: None }
    }
}

pub struct Ctx<'a, T: Transport, C: Clock> {
    pub store: Store,
    pub arctic: &'a Arctic<T, C>,
    pub now: i64,
    pub nonce: &'a str,
}

impl<T: Transport, C: Clock> Ctx<'_, T, C> {
    /// Writes the requests made so far to the cache's request log (endpoint and status only).
    /// With a ledger the requests are already booked there, so this only clears the run's own list.
    pub fn flush_log(&self) {
        let made: Vec<(String, u16)> = self.arctic.log.borrow_mut().drain(..).collect();
        if self.arctic.has_ledger() {
            return;
        }
        for (endpoint, status) in made {
            let _ = self.store.log_request(self.now, arctic::NAME, &endpoint, status);
        }
    }
}

pub struct ThreadArgs {
    pub reference: String,
    pub budget: usize,
    pub focus: Option<String>,
    pub comment: Option<String>,
    pub expand: Option<String>,
    pub limit: usize,
    pub refresh: bool,
    pub offline: bool,
    pub full: bool,
    pub json: bool,
}

fn bare<'a>(id: &'a str, prefix: &str) -> &'a str {
    let id = id.trim();
    id.strip_prefix(prefix).unwrap_or(id)
}

pub fn thread<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, a: &ThreadArgs) -> Result<String, Fail> {
    let (id, linked_comment) = match ids::parse_ref(&a.reference) {
        Ref::Thread { post, comment } => (post, comment),
        Ref::ShareLink(_) => {
            return Err(Fail::new(
                2,
                "this is a share link (/s/...), which only Reddit can resolve and this tool does not ask Reddit; open it in a browser and pass the full thread URL",
            ));
        }
        Ref::NotThread => return Err(Fail::new(2, format!("not a Reddit thread URL or id: {}", a.reference))),
    };
    if !a.offline {
        ctx.store.purge_before(ctx.now - KEEP_DAYS * 86_400)?;
    }
    let cached = ctx.store.thread(&id)?;
    let have_comments = cached.as_ref().is_some_and(|c| c.comments_fetched_at.is_some());
    let fresh = cached.as_ref().and_then(|c| c.comments_fetched_at).is_some_and(|at| ctx.now - at < FRESH_SECS);
    let mut note = None;

    if a.offline {
        if !have_comments {
            return Err(Fail::new(4, format!("t3_{id} is not in the cache and --offline was given")));
        }
    } else if a.refresh || !fresh {
        match ctx.arctic.thread(&id, a.limit) {
            Ok(t) => ctx.store.save_thread(&t, arctic::NAME, ctx.now)?,
            Err(Error::NotFound(_)) if cached.is_some() => {
                ctx.store.tombstone_thread(&id)?;
                return Err(Fail::new(3, format!("the archive no longer has t3_{id}; its cached text has been blanked")));
            }
            Err(e) if have_comments => {
                let at = cached.as_ref().and_then(|c| c.comments_fetched_at).unwrap_or(0);
                note = Some(format!("served from the cache as fetched {}; {e}", text::datetime(at)));
            }
            Err(e) => return Err(e.into()),
        }
    }
    if let Some(parent) = a.expand.as_deref().map(|c| bare(c, "t1_")) {
        if a.offline {
            return Err(Fail::new(4, "--expand needs the archive and --offline was given"));
        }
        let (comments, more) = ctx.arctic.tree(&id, Some(parent), a.limit)?;
        ctx.store.save_expansion(&id, parent, &comments, &more, ctx.now)?;
    }

    let c = ctx.store.thread(&id)?.ok_or_else(|| Fail::new(6, "the thread was saved but could not be read back"))?;
    // Ids from non-Reddit hosts are taken on trust; when the URL named another subreddit, the id may be someone else's.
    if let Some(named) = ids::subreddit_of(&a.reference).filter(|n| a.reference.contains('/') && !n.eq_ignore_ascii_case(&c.thread.post.subreddit)) {
        let warn =
            format!("the address named r/{named} but this id is a thread in r/{}; check it is the one you meant", render::attr(&c.thread.post.subreddit));
        note = Some(match note {
            Some(n) => format!("{n}; {warn}"),
            None => warn,
        });
    }
    let focus = a.comment.as_deref().map(|c| bare(c, "t1_").to_owned()).or(linked_comment);
    // Unquoted focus words are also tried as one phrase, so "memory context restore" favours comments that say exactly that.
    let mut terms = a.focus.as_deref().map(text::terms).unwrap_or_default();
    if let Some(f) = a.focus.as_deref().filter(|f| !f.contains('"'))
        && terms.len() > 1
    {
        terms.insert(0, text::collapse_ws(f).to_lowercase());
    }
    let options = Options { budget: a.budget, terms, focus, full: a.full };
    let selection = select::select(&c.thread, &options);
    let view = ThreadView {
        thread: &c.thread,
        selection: &selection,
        source: &c.source,
        fetched_at: c.comments_fetched_at.unwrap_or(c.fetched_at),
        now: ctx.now,
        nonce: ctx.nonce,
        note,
        resolved_by: rank::resolved_by(&c.thread),
    };
    Ok(if a.json { format!("{}\n", render::thread_json(&view)) } else { render::thread_text(&view) })
}

pub fn resolve<T: Transport, C: Clock>(ctx: &Ctx<T, C>, refs: &[String], json: bool) -> Result<String, Fail> {
    if refs.is_empty() {
        return Err(Fail::new(2, "give one or more URLs or ids, or - to read them from stdin"));
    }
    let mut seen = HashSet::new();
    let (mut threads, mut share, mut other) = (Vec::new(), Vec::new(), Vec::new());
    for r in refs {
        match ids::parse_ref(r) {
            Ref::Thread { post, comment } => {
                if seen.insert(post.clone()) {
                    let cached = ctx.store.post(&post)?.is_some_and(|p| p.3.is_some());
                    threads.push((post, comment, cached));
                }
            }
            Ref::ShareLink(_) => share.push(r.clone()),
            Ref::NotThread => other.push(r.clone()),
        }
    }
    if json {
        let rows: Vec<_> = threads
            .iter()
            .map(|(p, c, cached)| serde_json::json!({"id": format!("t3_{p}"), "comment": c.as_ref().map(|c| format!("t1_{c}")), "cached": cached}))
            .collect();
        return Ok(format!("{}\n", serde_json::json!({"ok": true, "threads": rows, "unresolved_share_links": share, "not_threads": other})));
    }
    let mut out = String::new();
    for (p, c, cached) in &threads {
        let _ =
            writeln!(out, "t3_{p}{}  {}", c.as_ref().map(|c| format!("  comment t1_{c}")).unwrap_or_default(), if *cached { "cached" } else { "not cached" });
    }
    for s in &share {
        let _ = writeln!(out, "unresolved share link (open it in a browser for the full URL): {s}");
    }
    for o in &other {
        let _ = writeln!(out, "not a thread: {o}");
    }
    let _ = writeln!(out, "{} threads, {} duplicates dropped", threads.len(), refs.len() - threads.len() - share.len() - other.len());
    Ok(out)
}

pub struct SearchArgs {
    pub query: String,
    pub subs: Vec<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub limit: usize,
    pub comments: bool,
    pub pace: String,
    pub offline: bool,
    pub nsfw: bool,
    pub explain: bool,
    pub json: bool,
}

pub fn search<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, a: &SearchArgs) -> Result<String, Fail> {
    let terms = text::terms(&a.query);
    if terms.is_empty() {
        return Err(Fail::new(2, "the query has no searchable words"));
    }
    let half_life = match a.pace.as_str() {
        "fast" => Some(180.0),
        "normal" => Some(730.0),
        "timeless" => None,
        other => return Err(Fail::new(2, format!("--pace must be fast, normal or timeless, not {other}"))),
    };
    let range = |name: &str, v: &Option<String>| match v.as_deref().map(text::epoch) {
        Some(None) => Err(Fail::new(2, format!("--{name} must be a date like 2026-01-31"))),
        Some(Some(t)) => Ok(Some(t)),
        None => Ok(None),
    };
    let (after, before) = (range("after", &a.after)?, range("before", &a.before)?);
    let subs: Vec<&str> = a.subs.iter().map(|s| s.trim().trim_start_matches("r/")).filter(|s| !s.is_empty()).collect();
    if !a.offline {
        ctx.store.purge_before(ctx.now - KEEP_DAYS * 86_400)?;
    }

    let mut sources = Vec::new();
    let mut lists: Vec<(String, Vec<String>)> = Vec::new();
    let mut found: HashMap<String, Candidate> = HashMap::new();
    let mut stopped: Option<Fail> = None;
    let mut coverage: Vec<serde_json::Value> = Vec::new();
    let mut budget_note: Option<String> = None;

    if a.offline {
        sources.push("arctic: not asked (offline)".to_owned());
    } else if subs.is_empty() {
        sources.push("arctic: not asked. The archive searches text only inside a subreddit: pass --sub name[,name], or find thread URLs with a web search and use rdt thread".to_owned());
    } else {
        if subs.len() > MAX_SUBS {
            sources.push(format!("arctic: only the first {MAX_SUBS} subreddits were searched"));
        }
        // Say before anything is sent whether the machine-wide budget can carry this search.
        let planned = subs.len().min(MAX_SUBS) * if a.comments { 2 } else { 1 };
        if let Some(b) = ctx.arctic.budget() {
            budget_note = budget_warning(&b, planned, ctx.arctic.now_ms());
            if let Some(n) = &budget_note {
                warn(n);
            }
        }
        // Timed-out searches are retried here, narrower and once per subreddit, never also by the client.
        ctx.arctic.take_timeout_retry();
        let mut retries_open = true;
        'subs: for (i, sub) in subs.iter().take(MAX_SUBS).enumerate() {
            let mut q = Query { subreddit: sub, text: &a.query, after: a.after.as_deref(), before: a.before.as_deref(), limit: 25 };
            let mut narrowed: Option<String> = None;
            let narrow_after;
            let narrow_text;
            let mut result = ctx.arctic.search_posts(&q);
            if matches!(result, Err(Error::TooBroad(_))) && retries_open {
                match narrower(&a.query, a.after.as_deref(), a.before.as_deref(), ctx.now) {
                    Some(Narrow::After(d, why)) => {
                        narrow_after = d;
                        q.after = Some(&narrow_after);
                        narrowed = Some(why);
                    }
                    Some(Narrow::Text(t, why)) => {
                        narrow_text = t;
                        q.text = &narrow_text;
                        narrowed = Some(why);
                    }
                    None => {}
                }
                if narrowed.is_some() {
                    // One narrower retry per run, whatever its answer: the same allowance the old blind retry had,
                    // so a run never sends more requests than before. Later timeouts are reported in the coverage.
                    retries_open = false;
                    ctx.arctic.pause_after_timeout();
                    // Sent once: a rate limit on the retry is reported, not waited out and tried a third time.
                    result = ctx.arctic.search_posts_once(&q);
                }
            }
            match result {
                Ok(posts) => {
                    let note = narrowed.as_deref().map(|w| format!("; narrowed after a timeout: {w}")).unwrap_or_default();
                    sources.push(format!("arctic r/{sub}: {} posts (newest first; the archive has no relevance order){note}", posts.len()));
                    lists.push((format!("arctic:{sub}"), posts.iter().map(|p| p.id.clone()).collect()));
                    let full = posts.len() >= q.limit;
                    let oldest = posts.iter().map(|p| p.created).min();
                    coverage.push(serde_json::json!({
                        "subreddit": sub, "status": if narrowed.is_some() { "narrowed" } else { "answered" }, "posts": posts.len(),
                        "after": q.after, "before": q.before, "query": q.text, "narrowed": narrowed,
                        // A full page means older matches exist that were not seen: coverage reaches back only this far.
                        "reached_back_to": if full { oldest.map(text::date) } else { q.after.map(str::to_owned) },
                        "page_full": full,
                    }));
                    for p in posts {
                        ctx.store.save_post(&p, arctic::NAME, ctx.now)?;
                        found.entry(p.id.clone()).or_insert(Candidate { post: p, comments: Vec::new(), cached_comments: false });
                    }
                }
                Err(e) => {
                    sources.push(format!("arctic r/{sub}: {e}"));
                    coverage.push(serde_json::json!({"subreddit": sub, "status": status_of(&e), "error": e.to_string(), "narrowed": narrowed}));
                    if matches!(e, Error::RateLimited { .. } | Error::Busy { .. } | Error::Refused(_) | Error::Unavailable(_)) {
                        for rest in subs.iter().take(MAX_SUBS).skip(i + 1) {
                            coverage.push(serde_json::json!({"subreddit": rest, "status": "not_reached"}));
                        }
                        stopped = Some(e.into());
                        break 'subs;
                    }
                    continue;
                }
            }
            if a.comments {
                match ctx.arctic.search_comments(&q) {
                    Ok(comments) => {
                        sources.push(format!("arctic r/{sub} comments: {} matches", comments.len()));
                        let mut order = Vec::new();
                        for c in &comments {
                            if !order.contains(&c.post_id) {
                                order.push(c.post_id.clone());
                            }
                        }
                        let missing: Vec<String> = order.iter().filter(|id| !found.contains_key(*id)).cloned().collect();
                        if !missing.is_empty() {
                            for p in ctx.arctic.posts_by_ids(&missing).unwrap_or_default() {
                                ctx.store.save_post(&p, arctic::NAME, ctx.now)?;
                                found.insert(p.id.clone(), Candidate { post: p, comments: Vec::new(), cached_comments: false });
                            }
                        }
                        for c in comments.into_iter().filter(|c| c.state == State::Live) {
                            if let Some(cand) = found.get_mut(&c.post_id) {
                                cand.comments.push((c.id, c.body));
                            }
                        }
                        lists.push((format!("arctic-comments:{sub}"), order));
                    }
                    Err(e) => {
                        sources.push(format!("arctic r/{sub} comments: {e}"));
                        if let Some(row) = coverage.last_mut() {
                            row["comments"] = serde_json::json!(status_of(&e));
                        }
                    }
                }
            }
        }
        for rest in subs.iter().skip(MAX_SUBS) {
            coverage.push(serde_json::json!({"subreddit": rest, "status": "skipped", "error": format!("only {MAX_SUBS} subreddits are searched per run")}));
        }
    }
    let by = |status: &str| -> Vec<serde_json::Value> { coverage.iter().filter(|r| r["status"] == status).map(|r| r["subreddit"].clone()).collect() };
    let coverage_block = serde_json::json!({
        "offline": a.offline,
        "asked": subs,
        "answered": coverage.iter().filter(|r| r["status"] == "answered" || r["status"] == "narrowed").map(|r| r["subreddit"].clone()).collect::<Vec<_>>(),
        "narrowed": by("narrowed"),
        "timed_out": by("timed_out"),
        "rate_limited": by("rate_limited"),
        "budget_used_up": by("budget_used_up"),
        "failed": by("failed"),
        "not_reached": by("not_reached"),
        "skipped": by("skipped"),
        "comments_searched": a.comments && !a.offline,
        "requested": {"after": a.after, "before": a.before},
        "subreddits": coverage,
        "budget_note": budget_note,
    });

    let matches = ctx.store.matches(&terms, 50)?;
    let mut cache_order = Vec::new();
    for m in matches {
        if !cache_order.contains(&m.post_id) {
            cache_order.push(m.post_id.clone());
        }
        if !found.contains_key(&m.post_id) {
            let Some((post, ..)) = ctx.store.post(&m.post_id)? else { continue };
            found.insert(m.post_id.clone(), Candidate { post, comments: Vec::new(), cached_comments: false });
        }
        if let (Some(cid), Some(cand)) = (m.comment_id, found.get_mut(&m.post_id))
            && !cand.comments.iter().any(|(id, _)| *id == cid)
        {
            cand.comments.push((cid, m.text));
        }
    }
    sources.push(format!("cache: {} threads matched", cache_order.len()));
    lists.push(("cache".to_owned(), cache_order));

    let mut candidates: Vec<Candidate> = found.into_values().collect();
    candidates.retain(|c| (a.nsfw || !c.post.over_18) && after.is_none_or(|t| c.post.created >= t) && before.is_none_or(|t| c.post.created < t));
    for c in &mut candidates {
        c.cached_comments = ctx.store.post(&c.post.id)?.is_some_and(|p| p.3.is_some());
    }
    let total = candidates.len();
    let mut ranked = rank::rank(candidates, &lists, &terms, ctx.now, half_life);
    let folded = ranked.len();
    ranked.truncate(a.limit.max(1));

    if ranked.is_empty() {
        let mut fail = match stopped {
            Some(f) => f,
            None => Fail::new(3, format!("nothing found for \"{}\". {}", a.query, sources.join("; "))),
        };
        fail.coverage = Some(coverage_block);
        return Err(fail);
    }
    let _ = total;
    let meta = SearchMeta { query: &a.query, now: ctx.now, nonce: ctx.nonce, sources, candidates: folded, explain: a.explain, coverage: Some(coverage_block) };
    Ok(if a.json { format!("{}\n", render::search_json(&ranked, &meta)) } else { render::search_text(&ranked, &meta) })
}

pub fn cite<T: Transport, C: Clock>(ctx: &Ctx<T, C>, ids: &[String]) -> Result<String, Fail> {
    if ids.is_empty() {
        return Err(Fail::new(2, "give one or more ids such as t3_abc123 or t1_def456"));
    }
    let mut out = String::new();
    let mut hits = 0;
    for raw in ids {
        let lower = raw.trim().to_lowercase();
        let comment = if lower.starts_with("t3_") { None } else { ctx.store.comment(bare(&lower, "t1_"))? };
        let post_id = comment.as_ref().map_or_else(|| bare(&lower, "t3_").to_owned(), |c| c.post_id.clone());
        let Some((p, ..)) = ctx.store.post(&post_id)? else {
            let _ = writeln!(out, "[{raw}] not in the cache");
            continue;
        };
        hits += 1;
        let who = |a: &Option<String>| a.as_deref().map_or("[unknown]".to_owned(), |a| format!("u/{}", render::attr(a)));
        let (subreddit, title) = (render::attr(&p.subreddit), render::attr(&p.title));
        match comment {
            Some(c) => {
                let _ = writeln!(
                    out,
                    "[t1_{}] {}, comment in r/{} on \"{}\", {}. https://www.reddit.com{}{}/",
                    c.id,
                    who(&c.author),
                    subreddit,
                    title,
                    text::date(c.created),
                    p.permalink,
                    c.id
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "[t3_{}] {}, \"{}\", r/{}, {}. https://www.reddit.com{}",
                    p.id,
                    who(&p.author),
                    p.title,
                    p.subreddit,
                    text::date(p.created),
                    p.permalink
                );
            }
        }
    }
    if hits == 0 { Err(Fail::new(3, out.trim_end().to_owned())) } else { Ok(out) }
}

pub fn verify_quote<T: Transport, C: Clock>(ctx: &Ctx<T, C>, id: &str, quote: &str) -> Result<String, Fail> {
    let lower = id.trim().to_lowercase();
    let body = if lower.starts_with("t3_") {
        ctx.store.post(bare(&lower, "t3_"))?.map(|(p, ..)| format!("{}\n{}", p.title, p.selftext))
    } else {
        match ctx.store.comment(bare(&lower, "t1_"))? {
            Some(c) => Some(c.body),
            None => ctx.store.post(&lower)?.map(|(p, ..)| format!("{}\n{}", p.title, p.selftext)),
        }
    };
    let body = body.ok_or_else(|| Fail::new(3, format!("{id} is not in the cache")))?;
    if render::quote_matches(&body, quote) {
        Ok(format!("verified: the quote appears in {id}\n"))
    } else {
        Err(Fail::new(3, format!("not verified: the quote does not appear word for word in {id}")))
    }
}

pub fn cache_stats<T: Transport, C: Clock>(ctx: &Ctx<T, C>) -> Result<String, Fail> {
    let s = ctx.store.stats()?;
    let when = |t: Option<i64>| t.map_or("-".to_owned(), text::datetime);
    Ok(format!(
        "posts {}  (with comments {})\ncomments {}\noldest fetch {}\nnewest fetch {}\narchive requests in the last hour {}\nthreads not fetched for {KEEP_DAYS} days are deleted automatically\n",
        s.posts,
        s.threads_with_comments,
        s.comments,
        when(s.oldest_fetch),
        when(s.newest_fetch),
        ctx.store.requests_since(ctx.now - 3600)?
    ))
}

/// Blanks cached text the archive no longer has or now marks as removed or deleted.
pub fn cache_check<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>) -> Result<String, Fail> {
    let (posts, comments) = ctx.store.live_ids()?;
    let (mut blanked, mut unsure) = (0, 0);
    let now_posts = ctx.arctic.posts_by_ids(&posts)?;
    let states: HashMap<&str, State> = now_posts.iter().map(|p| (p.id.as_str(), p.state)).collect();
    // Absence from an answer means "gone" only when the answer is otherwise nearly whole. A short or empty answer is
    // more likely a fault at the archive, and blanking the cache on it could not be undone.
    let whole = |asked: &[String], states: &HashMap<&str, State>| asked.iter().filter(|id| states.contains_key(id.as_str())).count() * 10 >= asked.len() * 9;
    let trust_absence = whole(&posts, &states);
    for id in &posts {
        match states.get(id.as_str()) {
            Some(State::Live) => {}
            Some(s) => blanked += ctx.store.tombstone_post(id, *s)?,
            None if trust_absence => blanked += ctx.store.tombstone_post(id, State::Removed)?,
            None => unsure += 1,
        }
    }
    let now_comments = ctx.arctic.comments_by_ids(&comments)?;
    let states: HashMap<&str, State> = now_comments.iter().map(|c| (c.id.as_str(), c.state)).collect();
    let trust_absence = whole(&comments, &states);
    for id in &comments {
        match states.get(id.as_str()) {
            Some(State::Live) => {}
            Some(s) => blanked += ctx.store.tombstone_comment(id, *s)?,
            None if trust_absence => blanked += ctx.store.tombstone_comment(id, State::Removed)?,
            None => unsure += 1,
        }
    }
    let mut out =
        format!("checked {} posts and {} comments against the archive; blanked {blanked} that are now removed, deleted or gone\n", posts.len(), comments.len());
    if unsure > 0 {
        let _ = writeln!(
            out,
            "{unsure} items were missing from an incomplete answer and were left as they are; run the check again later, or refetch a thread with rdt thread <id> --refresh"
        );
    }
    Ok(out)
}

pub fn cache_purge<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, days: u32) -> Result<String, Fail> {
    let n = ctx.store.purge_before(ctx.now - i64::from(days) * 86_400)?;
    Ok(format!("deleted {n} threads not fetched in the last {days} days\n"))
}

pub fn cache_forget<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, id: &str) -> Result<String, Fail> {
    let Ref::Thread { post, .. } = ids::parse_ref(id) else { return Err(Fail::new(2, format!("not a thread id or URL: {id}"))) };
    match ctx.store.forget(&post)? {
        0 => Err(Fail::new(3, format!("t3_{post} is not in the cache"))),
        _ => Ok(format!("deleted t3_{post} and its comments from the cache\n")),
    }
}

pub struct SubsArgs {
    pub topic: String,
    pub urls: Vec<String>,
    pub offline: bool,
    pub nsfw: bool,
    pub json: bool,
}

/// Name lookups for words of the topic, and checks of the leading candidates: the archive's allowance is small.
const MAX_NAME_LOOKUPS: usize = 2;
const MAX_CHECKS: usize = 4;
/// How many subreddits the built-in starter list may propose for one topic.
const MAX_STARTERS: usize = 6;
/// Names are matched by prefix, largest first; a full page with no exact match proves nothing about a small namesake.
const LOOKUP_PAGE: usize = 100;
/// Above this many archived posts a text search usually needs a date range; far above it, it usually times out.
const BUSY_POSTS: i64 = 100_000;
const VERY_BUSY_POSTS: i64 = 1_000_000;

#[derive(Default)]
struct SubCandidate {
    name: String,
    urls: usize,
    cached: usize,
    name_match: bool,
    info: Option<reddit_core::model::Subreddit>,
    /// The archive answered about this exact name (present or not). False when unknown.
    checked: bool,
    /// A cached thread in it is marked over-18, known even when the archive was not asked.
    over_18: bool,
    /// Proposed by the built-in starter list: its rank there and the topic word that brought it in.
    starter: Option<(usize, &'static str)>,
}

impl SubCandidate {
    /// URLs and cached threads are evidence; a starter-list entry alone is only a guess and costs no lookups.
    fn evidence(&self) -> usize {
        self.urls + self.cached
    }
}

fn compact(n: i64) -> String {
    match n {
        1_000_000.. => format!("{:.1}M", n as f64 / 1e6),
        1_000.. => format!("{}k", n / 1000),
        _ => n.to_string(),
    }
}

/// Where a topic is discussed: subreddits named by the URLs a web search found, by matching cached threads,
/// and by name. The archive knows subreddits by name only, so the URLs are the signal that matters.
pub fn subs<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, a: &SubsArgs) -> Result<String, Fail> {
    let terms = text::terms(&a.topic);
    if terms.is_empty() {
        return Err(Fail::new(2, "the topic has no searchable words"));
    }
    let mut found: HashMap<String, SubCandidate> = HashMap::new();
    let mut sources = Vec::new();
    // Names reach the output and a suggested command line, so every one must pass the same rule, whatever its source.
    fn candidate<'a>(found: &'a mut HashMap<String, SubCandidate>, name: &str) -> Option<&'a mut SubCandidate> {
        ids::is_subreddit_name(name)
            .then(|| found.entry(name.to_lowercase()).or_insert_with(|| SubCandidate { name: name.to_owned(), ..SubCandidate::default() }))
    }

    let mut named = 0;
    for u in &a.urls {
        if let Some(c) = ids::subreddit_of(u).and_then(|name| candidate(&mut found, &name)) {
            c.urls += 1;
            named += 1;
        }
    }
    if !a.urls.is_empty() {
        sources.push(format!("urls: {named} of {} name a subreddit", a.urls.len()));
    }

    // A cached thread speaks for its subreddit only if it is about the topic: it must contain most of the topic's
    // terms (two thirds, rounded up), not just one common word.
    // Filler words do not count towards "most", and plurals are matched on a crude stem, because the cache's own
    // search matches word stems and the check here must not be stricter than the search that found the thread.
    const FILLER: [&str; 20] =
        ["a", "an", "the", "of", "for", "in", "on", "to", "with", "and", "is", "are", "my", "how", "what", "why", "does", "do", "it", "vs"];
    let stem = |t: &str| match t {
        t if t.contains(' ') || t.chars().any(|c| c.is_ascii_digit()) => t.to_owned(),
        t if t.len() > 4 && t.ends_with("es") => t[..t.len() - 2].to_owned(),
        t if t.len() > 3 && t.ends_with('s') => t[..t.len() - 1].to_owned(),
        t => t.to_owned(),
    };
    let mut key: Vec<String> = terms.iter().filter(|t| !FILLER.contains(&t.as_str())).map(|t| stem(t)).collect();
    if key.is_empty() {
        key = terms.clone();
    }
    let mut terms_in_post: HashMap<String, HashSet<usize>> = HashMap::new();
    for m in ctx.store.matches(&terms, 100)? {
        let lower = m.text.to_lowercase();
        let hits = key.iter().enumerate().filter(|(_, t)| rank::count(&lower, t) > 0).map(|(i, _)| i);
        terms_in_post.entry(m.post_id).or_default().extend(hits);
    }
    let needed = (key.len() * 2).div_ceil(3).max(1);
    let mut on_topic = 0;
    for (post_id, hit) in &terms_in_post {
        if hit.len() >= needed
            && let Some((p, ..)) = ctx.store.post(post_id)?
            && let Some(c) = candidate(&mut found, &p.subreddit)
        {
            c.cached += 1;
            c.over_18 |= p.over_18;
            on_topic += 1;
        }
    }
    sources.push(format!("cache: {on_topic} threads on the topic"));

    // Web searches often find no Reddit URLs at all: when neither URLs nor the cache gave anything, the built-in list
    // proposes subreddits from the topic words. Real evidence, when there is any, is never diluted by guesses.
    let starters = if found.is_empty() { reddit_core::starter::for_topic(&a.topic, MAX_STARTERS) } else { Vec::new() };
    for (i, s) in starters.iter().enumerate() {
        if let Some(c) = candidate(&mut found, s.name) {
            c.starter.get_or_insert((i, s.because));
        }
    }
    if !starters.is_empty() {
        sources.push(format!("starter list: {} subreddits from the topic words (not checked against the archive)", starters.len()));
    }

    // What earlier runs learned costs nothing and is used offline too. A "no such subreddit" answer is trusted
    // for a day only: the archive's list lags, and a wrong "no" should not stick.
    let (fresh, fresh_missing) = (ctx.now - KEEP_DAYS * 86_400, ctx.now - 86_400);
    let mut remembered = 0;
    for (key, c) in found.iter_mut() {
        if let Some(known) = ctx.store.subreddit(key, fresh, fresh_missing)? {
            (c.info, c.checked) = (known, true);
            remembered += 1;
        }
    }
    let mut stopped: Option<String> = None;
    let mut timeouts = 0;
    let requests_before = ctx.arctic.log.borrow().len();
    if !a.offline {
        // A timeout on one lookup is that lookup's problem; a rate limit, refusal or outage ends the asking.
        // The flag says whether the archive sent a full page, in which case a missing name proves nothing.
        let mut ask = |prefix: &str, stopped: &mut Option<String>| -> Option<(Vec<reddit_core::model::Subreddit>, bool)> {
            if stopped.is_some() {
                return None;
            }
            match ctx.arctic.subreddits_page(prefix, LOOKUP_PAGE) {
                Ok((list, rows)) => Some((list, rows >= LOOKUP_PAGE)),
                Err(Error::TooBroad(_)) => {
                    timeouts += 1;
                    None
                }
                Err(e) => {
                    *stopped = Some(e.to_string());
                    None
                }
            }
        };
        // First confirm the candidates the URLs and the cache gave, and learn their size: they are the real signal.
        let mut order: Vec<(String, usize)> =
            found.iter().filter(|(_, c)| !c.checked && c.evidence() > 0).map(|(k, c)| (k.clone(), c.urls * 3 + c.cached * 2)).collect();
        order.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
        for (key, _) in order.into_iter().take(MAX_CHECKS) {
            if let Some((list, full_page)) = ask(&key, &mut stopped) {
                let c = found.get_mut(&key).expect("key came from the map");
                c.info = list.into_iter().find(|s| s.name.to_lowercase() == key);
                c.checked = c.info.is_some() || !full_page;
                if c.checked {
                    ctx.store.save_subreddit(&key, c.info.as_ref(), ctx.now)?;
                }
            }
        }
        // Only when that left little to go on: a topic word may itself be a subreddit name ("thinkpad").
        // Tiny namesakes are ignored. These answers are remembered like the others.
        if found.values().filter(|c| c.evidence() > 0).count() < 2 {
            let words: Vec<&String> =
                terms.iter().filter(|t| t.len() >= 5 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).take(MAX_NAME_LOOKUPS).collect();
            for w in words {
                let exact = match ctx.store.subreddit(w, fresh, fresh_missing)? {
                    Some(known) => {
                        remembered += 1;
                        known
                    }
                    None => match ask(w, &mut stopped) {
                        Some((list, full_page)) => {
                            let exact = list.into_iter().find(|s| s.name.eq_ignore_ascii_case(w));
                            if exact.is_some() || !full_page {
                                ctx.store.save_subreddit(w, exact.as_ref(), ctx.now)?;
                            }
                            exact
                        }
                        None => None,
                    },
                };
                if let Some(s) = exact.filter(|s| s.subscribers.unwrap_or(0) >= 1000)
                    && let Some(c) = candidate(&mut found, &s.name)
                {
                    (c.name_match, c.checked) = (true, true);
                    c.info = Some(s);
                }
            }
        }
    }
    let requests = ctx.arctic.log.borrow().len() - requests_before;
    let mut detail = if timeouts > 0 { format!(", {timeouts} timed out") } else { String::new() };
    if remembered > 0 {
        detail.push_str(&format!(", {remembered} answers remembered from earlier runs"));
    }
    sources.push(match (&stopped, a.offline) {
        (_, true) => format!("arctic: not asked (offline){detail}"),
        (Some(e), _) => format!("arctic: {requests} requests{detail}, then stopped: {e}"),
        (None, _) => format!("arctic: {requests} requests{detail} (subreddits are matched by name only; counts there are updated infrequently)"),
    });

    let score = |c: &SubCandidate| {
        let size = c.info.as_ref().and_then(|i| i.subscribers).unwrap_or(0).max(1) as f64;
        (c.urls * 3 + c.cached * 2) as f64 + if c.name_match { 1.0 + size.log10() / 6.0 } else { 0.0 } + c.starter.map_or(0.0, |(i, _)| 1.0 - i as f64 * 0.05)
    };
    let mut list: Vec<SubCandidate> = found.into_values().filter(|c| a.nsfw || !(c.over_18 || c.info.as_ref().is_some_and(|i| i.over_18))).collect();
    list.sort_by(|x, y| score(y).total_cmp(&score(x)).then(x.name.to_lowercase().cmp(&y.name.to_lowercase())));
    list.truncate(10);
    if list.is_empty() {
        let hint = "pass the Reddit URLs a web search found (rdt subs \"<topic>\" <url> <url> ...), or name a subreddit yourself";
        return Err(Fail::new(3, format!("no subreddits suggested for \"{}\". {}; {hint}", a.topic, sources.join("; "))));
    }

    let size_note = |c: &SubCandidate| match c.info.as_ref().and_then(|i| i.posts) {
        Some(p) if p >= VERY_BUSY_POSTS => "very busy: text search will likely time out without a narrow --after/--before",
        Some(p) if p >= BUSY_POSTS => "busy: add --after to text searches",
        Some(_) => "quiet enough to search without a date range",
        None if c.checked && c.info.is_none() => "not in the archive's list (new, renamed, private, or a typo)",
        None => "size unknown",
    };
    let why = |c: &SubCandidate| {
        let mut parts = Vec::new();
        if c.urls > 0 {
            parts.push(format!("{} URL{}", c.urls, if c.urls == 1 { "" } else { "s" }));
        }
        if c.cached > 0 {
            parts.push(format!("{} cached thread{}", c.cached, if c.cached == 1 { "" } else { "s" }));
        }
        if c.name_match {
            parts.push("a topic word is its name".to_owned());
        }
        if let Some((_, word)) = c.starter {
            parts.push(format!("starter list ({word})"));
        }
        parts.join(", ")
    };
    // Suggest searching the ones that exist as far as we know. A very busy subreddit would time out the whole
    // search, so it is left out and named separately, unless nothing else is on offer.
    let posts = |c: &SubCandidate| c.info.as_ref().and_then(|i| i.posts).unwrap_or(0);
    let known: Vec<&SubCandidate> = list.iter().filter(|c| !(c.checked && c.info.is_none())).collect();
    let (too_busy, searchable): (Vec<&SubCandidate>, Vec<&SubCandidate>) = known.into_iter().partition(|c| posts(c) >= VERY_BUSY_POSTS);
    let (picked, days) = if searchable.is_empty() {
        (&too_busy, Some(30))
    } else {
        // A starter-list name of unknown size may be busy: a year's window costs nothing and saves a timed-out request.
        (&searchable, searchable.iter().take(MAX_SUBS).any(|c| posts(c) >= BUSY_POSTS || (c.starter.is_some() && c.info.is_none())).then_some(365))
    };
    let next = (!picked.is_empty()).then(|| {
        let names: Vec<&str> = picked.iter().take(MAX_SUBS).map(|c| c.name.as_str()).collect();
        let range = days.map(|d| format!(" --after {}", text::date(ctx.now - d * 86_400))).unwrap_or_default();
        format!("rdt search {} --sub {}{range}", render::shell_arg(&a.topic), names.join(","))
    });
    let left_out: Vec<String> = if searchable.is_empty() { Vec::new() } else { too_busy.iter().map(|c| format!("r/{}", c.name)).collect() };

    if a.json {
        let rows: Vec<_> = list
            .iter()
            .map(|c| {
                let i = c.info.as_ref();
                serde_json::json!({
                    "name": c.name, "urls": c.urls, "cached_threads": c.cached, "name_match": c.name_match, "checked": c.checked,
                    "in_archive": c.checked.then_some(i.is_some()), "subscribers": i.and_then(|i| i.subscribers), "archived_posts": i.and_then(|i| i.posts),
                    "over_18": i.map(|i| i.over_18), "description": i.map(|i| render::attr(&i.description)), "search_note": size_note(c),
                    "starter": c.starter.map(|(_, w)| w),
                })
            })
            .collect();
        return Ok(format!(
            "{}\n",
            serde_json::json!({"ok": true, "topic": a.topic, "as_of": text::datetime(ctx.now), "sources": sources, "subreddits": rows, "next": next, "too_busy_for_a_joint_search": left_out})
        ));
    }
    let n = ctx.nonce;
    let mut out = String::new();
    let _ = writeln!(out, "<reddit_subs_{n} topic=\"{}\" as_of=\"{}\" shown=\"{}\">", render::attr(&a.topic), text::datetime(ctx.now), list.len());
    for s in &sources {
        let _ = writeln!(out, "source: {s}");
    }
    for (i, c) in list.iter().enumerate() {
        let info = c.info.as_ref();
        let counts = match info {
            Some(s) => format!("{} subscribers  {} archived posts", s.subscribers.map_or("?".into(), compact), s.posts.map_or("?".into(), compact)),
            None => "size not looked up".to_owned(),
        };
        let _ = writeln!(out, "[{}] r/{}  {counts}  from: {}", i + 1, c.name, why(c));
        let _ = writeln!(out, "    {}", size_note(c));
        if let Some(d) = info.map(|s| render::attr(text::cut(&s.description, 140))).filter(|d| !d.trim().is_empty()) {
            let _ = writeln!(out, "    | {d}");
        }
    }
    let _ = writeln!(out, "</reddit_subs_{n}>");
    if !left_out.is_empty() {
        let _ = writeln!(out, "left out of the search below as very busy: {} (search each alone with a narrow --after/--before)", left_out.join(", "));
    }
    if let Some(next) = next {
        let _ = writeln!(out, "next: {next}");
    }
    Ok(out)
}

pub fn sources<T: Transport, C: Clock>(ctx: &Ctx<T, C>) -> Result<String, Fail> {
    let mut out = String::from("reddit   not used: Reddit has closed its API to new apps; this tool never contacts Reddit\n");
    match ctx.arctic.newest() {
        Ok(Some(t)) => {
            let lag = (ctx.now - t).max(0);
            let verdict = if lag < 3600 {
                "current"
            } else if lag < 86_400 * 3 {
                "lagging"
            } else {
                "STALE: treat 'no recent results' with suspicion"
            };
            let _ = writeln!(
                out,
                "arctic   reachable; newest post {} ({} behind): {verdict}",
                text::datetime(t),
                if lag < 120 { format!("{lag} s") } else { format!("{} min", lag / 60) }
            );
        }
        Ok(None) => out.push_str("arctic   reachable but returned no posts\n"),
        Err(e) => {
            let _ = writeln!(out, "arctic   {e}");
        }
    }
    let s = ctx.store.stats()?;
    let _ = writeln!(
        out,
        "cache    {} threads with comments, {} comments; {} archive requests in the last hour",
        s.threads_with_comments,
        s.comments,
        ctx.store.requests_since(ctx.now - 3600)?
    );
    if let Some(b) = ctx.arctic.budget() {
        let now_ms = ctx.arctic.now_ms();
        let secs = |at: u64| at.saturating_sub(now_ms).div_ceil(1000);
        let _ = write!(out, "budget   {} of {} requests left this minute, shared by every rdt run on this PC", b.left(), b.cap);
        if let Some(until) = b.blocked_until_ms {
            let _ = write!(out, "; rate limited, retry in {} s", secs(until));
        } else if let Some(t) = b.frees_at_ms {
            let _ = write!(out, "; the next slot frees in {} s", secs(t));
        }
        out.push('\n');
    }
    Ok(out)
}

pub fn doctor<T: Transport, C: Clock>(ctx: &Ctx<T, C>, path: &Path) -> Result<String, Fail> {
    let probe = Store::memory()?;
    probe.save_post(
        &reddit_core::model::parse_post(&serde_json::json!({"id": "probe", "title": "coil whine fix", "selftext": "undervolting stopped the whine"}))
            .ok_or_else(|| Fail::new(6, "parser self-check failed"))?,
        "probe",
        0,
    )?;
    let fts = probe.matches(&["whining".to_owned()], 5)?.len() == 1;
    let s = ctx.store.stats()?;
    Ok(format!(
        "storage   SQLite full-text search {}\ncache     {} ({} posts)\nhttps     client built (Windows TLS); identifies as \"{}\"\nnetwork   no requests sent\n",
        if fts { "ok (stemmed match found)" } else { "FAILED" },
        path.display(),
        s.posts,
        reddit_sources::transport::USER_AGENT
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddit_sources::transport::{FakeClock, Fixtures};

    const POSTS_IDS: &str = include_str!("../../../fixtures/arctic/posts_ids.json");
    const TREE: &str = include_str!("../../../fixtures/arctic/tree.json");
    const TREE_MORE: &str = include_str!("../../../fixtures/arctic/tree_more.json");
    const POSTS_SEARCH: &str = include_str!("../../../fixtures/arctic/posts_search.json");
    const COMMENTS_SEARCH: &str = include_str!("../../../fixtures/arctic/comments_search.json");
    const NOW: i64 = 1_794_547_000;

    fn arctic(f: Fixtures) -> Arctic<Fixtures, FakeClock> {
        Arctic::new(f, FakeClock::default())
    }

    fn ctx<'a>(a: &'a Arctic<Fixtures, FakeClock>) -> Ctx<'a, Fixtures, FakeClock> {
        Ctx { store: Store::memory().unwrap(), arctic: a, now: NOW, nonce: "ab12cd" }
    }

    fn args(reference: &str) -> ThreadArgs {
        ThreadArgs {
            reference: reference.into(),
            budget: 5000,
            focus: None,
            comment: None,
            expand: None,
            limit: 500,
            refresh: false,
            offline: false,
            full: false,
            json: false,
        }
    }

    fn sargs(query: &str, subs: &[&str]) -> SearchArgs {
        SearchArgs {
            query: query.into(),
            subs: subs.iter().map(|s| (*s).to_owned()).collect(),
            after: None,
            before: None,
            limit: 10,
            comments: false,
            pace: "normal".into(),
            offline: false,
            nsfw: false,
            explain: false,
            json: false,
        }
    }

    fn online() -> Fixtures {
        Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS).route("comments/tree", 200, &[], TREE)
    }

    #[test]
    fn thread_is_fetched_once_then_served_from_cache() {
        let a = arctic(online());
        let mut c = ctx(&a);
        let url = "https://old.reddit.com/r/AMDHelp/comments/1xsjnud/about_sapphire_pulse_9060xt_16g_coil_whine/";
        let first = thread(&mut c, &args(url)).unwrap();
        assert!(first.starts_with("<reddit_thread_ab12cd id=\"t3_1xsjnud\" sub=\"r/AMDHelp\""), "{first}");
        assert!(first.contains("<comments_ab12cd shown=\"4\" loaded=\"4\""));
        assert!(first.contains("u/user_"), "fixtures carry scrubbed names");
        let second = thread(&mut c, &args("t3_1xsjnud")).unwrap();
        assert_eq!(first, second);
        assert_eq!(a.log.borrow().len(), 2, "the second call made no request");
        c.flush_log();
        assert_eq!(c.store.requests_since(0).unwrap(), 2);
        let offline = thread(&mut c, &ThreadArgs { offline: true, ..args("1xsjnud") }).unwrap();
        assert_eq!(first, offline);
    }

    #[test]
    fn stale_cache_is_served_with_a_note_when_the_archive_fails() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let down = arctic(Fixtures::default().route("posts/ids", 429, &[("x-ratelimit-reset", "40")], "{}"));
        let mut later = Ctx { store: c.store, arctic: &down, now: NOW + FRESH_SECS + 1, nonce: "ab12cd" };
        let out = thread(&mut later, &args("1xsjnud")).unwrap();
        assert!(out.contains("note: served from the cache as fetched 2026-11-13T05:16Z; the archive is rate limiting; try again in 40 s"), "{out}");
    }

    #[test]
    fn failures_carry_the_right_exit_codes() {
        let a = arctic(Fixtures::default().route("posts/ids", 429, &[("x-ratelimit-reset", "40")], "{}"));
        let mut c = ctx(&a);
        assert_eq!(
            thread(&mut c, &args("1xsjnud")).unwrap_err(),
            Fail { code: 5, message: "the archive is rate limiting; try again in 40 s".into(), retry_after_s: Some(40), coverage: None }
        );
        assert_eq!(thread(&mut c, &ThreadArgs { offline: true, ..args("1xsjnud") }).unwrap_err().code, 4);
        assert_eq!(thread(&mut c, &args("https://www.reddit.com/r/x/s/AbCdEf")).unwrap_err().code, 2);
        assert_eq!(thread(&mut c, &args("https://example.com/")).unwrap_err().code, 2);
        let missing = arctic(Fixtures::default().route("posts/ids", 200, &[], r#"{"data":[]}"#));
        assert_eq!(thread(&mut ctx(&missing), &args("zzzzzzz")).unwrap_err().code, 3);
        let refused = arctic(Fixtures::default().route("posts/ids", 403, &[], "no"));
        assert_eq!(thread(&mut ctx(&refused), &args("1xsjnud")).unwrap_err().code, 6);
        assert_eq!(refused.log.borrow().len(), 1, "a refusal is not retried");
    }

    #[test]
    fn expand_loads_a_collapsed_branch() {
        let a = arctic(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS).route("parent_id=19sgled0apypv", 200, &[], TREE).route(
            "comments/tree",
            200,
            &[],
            TREE_MORE,
        ));
        let mut c = ctx(&a);
        let before = thread(&mut c, &args("1xsjnud")).unwrap();
        assert!(before.contains("shown=\"2\" loaded=\"2\""));
        assert!(before.contains("[+2 top-level comments not loaded; rdt thread t3_1xsjnud --refresh --limit 5000]"));
        let after = thread(&mut c, &ThreadArgs { expand: Some("t1_19sgled0apypv".into()), ..args("1xsjnud") }).unwrap();
        assert!(after.contains("loaded=\"4\""), "{after}");
    }

    #[test]
    fn linked_comment_and_focus_words_are_honoured() {
        let a = arctic(online());
        let mut c = ctx(&a);
        let out = thread(
            &mut c,
            &ThreadArgs { focus: Some("undervolt".into()), budget: 300, ..args("https://www.reddit.com/r/AMDHelp/comments/1xsjnud/slug/19sfqopne46x0/") },
        )
        .unwrap();
        assert!(out.contains("t1_19sfqopne46x0"), "the linked comment is always shown: {out}");
        let json = thread(&mut c, &ThreadArgs { json: true, ..args("1xsjnud") }).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!((v["ok"].as_bool(), v["id"].as_str(), v["comments"].as_array().map(Vec::len)), (Some(true), Some("t3_1xsjnud"), Some(4)));
    }

    #[test]
    fn an_address_naming_another_subreddit_gets_a_warning() {
        let a = arctic(online());
        let mut c = ctx(&a);
        let wrong = thread(&mut c, &args("https://mirror.example/r/buildapc/comments/1xsjnud/whatever/")).unwrap();
        assert!(wrong.contains("note: the address named r/buildapc but this id is a thread in r/AMDHelp; check it is the one you meant"), "{wrong}");
        let right = thread(&mut c, &args("https://mirror.example/r/amdhelp/comments/1xsjnud/whatever/")).unwrap();
        assert!(!right.contains("note:"));
        assert!(!thread(&mut c, &args("1xsjnud")).unwrap().contains("note:"));
    }

    #[test]
    fn resolve_dedupes_and_reports() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let refs: Vec<String> = [
            "https://redd.it/1xsjnud",
            "https://np.reddit.com/r/AMDHelp/comments/1xsjnud/x/",
            "https://www.reddit.com/r/b/comments/1abc2de/y/k3x9z1a",
            "https://www.reddit.com/r/b/s/Code",
            "https://www.reddit.com/r/b/",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let out = resolve(&c, &refs, false).unwrap();
        assert_eq!(
            out.lines().collect::<Vec<_>>(),
            [
                "t3_1xsjnud  cached",
                "t3_1abc2de  comment t1_k3x9z1a  not cached",
                "unresolved share link (open it in a browser for the full URL): https://www.reddit.com/r/b/s/Code",
                "not a thread: https://www.reddit.com/r/b/",
                "2 threads, 1 duplicates dropped"
            ]
        );
        assert_eq!(resolve(&c, &[], false).unwrap_err().code, 2);
    }

    #[test]
    fn search_merges_archive_and_cache_and_ranks() {
        let a = arctic(online().route("posts/search", 200, &[], POSTS_SEARCH).route("comments/search", 200, &[], COMMENTS_SEARCH));
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let out = search(&mut c, &SearchArgs { comments: true, explain: true, ..sargs("\"coil whine\" 9060xt", &["r/AMDHelp"]) }).unwrap();
        assert!(out.contains("source: arctic r/AMDHelp: 3 posts"), "{out}");
        assert!(out.contains("source: arctic r/AMDHelp comments: 3 matches"));
        assert!(out.contains("[1] t3_1xsjnud  r/AMDHelp  2026-11-10"), "the thread naming the card ranks first: {out}");
        assert!(out.contains("via: arctic:AMDHelp#1, arctic-comments:AMDHelp#1, cache#1"));
        assert!(out.contains("  cached\n"));
        assert!(out.contains("explain: score"));
        assert!(out.contains("next: rdt thread t3_1xsjnud"));
        let json = search(&mut c, &SearchArgs { json: true, ..sargs("coil whine", &["AMDHelp"]) }).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["results"][0]["id"], "t3_1xsjnud");
    }

    #[test]
    fn search_without_a_subreddit_uses_the_cache_and_says_why() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let calls = a.log.borrow().len();
        let out = search(&mut c, &sargs("coil whine", &[])).unwrap();
        assert!(out.contains("source: arctic: not asked. The archive searches text only inside a subreddit"));
        assert!(out.contains("t3_1xsjnud"));
        assert_eq!(a.log.borrow().len(), calls, "no request was made");
        let none = search(&mut c, &sargs("zebra crossing", &[])).unwrap_err();
        assert_eq!(none.code, 3);
        assert_eq!(search(&mut c, &SearchArgs { after: Some("last week".into()), ..sargs("x1", &[]) }).unwrap_err().code, 2);
        assert_eq!(
            search(&mut c, &SearchArgs { after: Some("2027-01-01".into()), ..sargs("coil whine", &[]) }).unwrap_err().code,
            3,
            "date filters apply to cache hits too"
        );
    }

    #[test]
    fn search_reports_a_rate_limit_with_its_wait() {
        let a = arctic(Fixtures::default().route("posts/search", 429, &[("x-ratelimit-reset", "12")], "{}"));
        let f = search(&mut ctx(&a), &sargs("coil whine", &["a", "b"])).unwrap_err();
        assert_eq!((f.code, f.retry_after_s), (5, Some(12)));
        assert_eq!(a.log.borrow().len(), 1, "the second subreddit is not tried after a rate limit");
        let f = search(&mut ctx(&a), &SearchArgs { json: true, ..sargs("coil whine", &["a", "b"]) }).unwrap_err();
        let cov = f.coverage.expect("a stopped search still says what it covered");
        assert_eq!((cov["rate_limited"].clone(), cov["not_reached"].clone()), (serde_json::json!(["a"]), serde_json::json!(["b"])));
    }

    const TIMEOUT: &str = r#"{"data":null,"error":"Timeout. Maybe slow down a bit"}"#;

    #[test]
    fn a_timed_out_subreddit_is_retried_narrower_and_the_coverage_says_so() {
        // Only a query with a date window answers; the full-range query times out in every subreddit.
        let a = arctic(Fixtures::default().route("after=", 200, &[], POSTS_SEARCH).route("posts/search", 422, &[], TIMEOUT));
        let json = search(&mut ctx(&a), &SearchArgs { json: true, ..sargs("coil whine", &["a", "b", "c", "d", "e"]) }).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let cov = &v["coverage"];
        assert_eq!((cov["answered"].clone(), cov["narrowed"].clone()), (serde_json::json!(["a"]), serde_json::json!(["a"])));
        assert_eq!(cov["timed_out"], serde_json::json!(["b", "c", "d", "e"]), "the one retry is spent; the rest are reported, not retried");
        let since = text::date(NOW - 180 * 86_400);
        assert_eq!(cov["subreddits"][0]["after"], since.as_str());
        assert!(cov["subreddits"][0]["narrowed"].as_str().unwrap().contains("older part of the range was dropped"));
        // HEAD's worst case for five subreddits was 5 + its one blind retry: never more than that.
        assert_eq!(a.log.borrow().len(), 6, "one narrower retry per run, and no blind retry on top");
        assert!(v["sources"][0].as_str().unwrap().contains("narrowed after a timeout"));
    }

    #[test]
    fn a_rate_limit_on_the_narrower_retry_is_not_retried_again() {
        let a = arctic(Fixtures::default().route("after=", 429, &[("x-ratelimit-reset", "3")], "{}").route("posts/search", 422, &[], TIMEOUT));
        let f = search(&mut ctx(&a), &SearchArgs { json: true, ..sargs("coil whine", &["a"]) }).unwrap_err();
        assert_eq!(*a.log.borrow(), [("posts/search".to_owned(), 422), ("posts/search".to_owned(), 429)], "0.2.0 sent two here; so does this");
        assert_eq!(f.coverage.unwrap()["rate_limited"], serde_json::json!(["a"]));
    }

    #[test]
    fn a_narrower_retry_that_also_times_out_is_reported() {
        let a = arctic(Fixtures::default().route("posts/search", 422, &[], TIMEOUT));
        let f = search(&mut ctx(&a), &SearchArgs { json: true, ..sargs("coil whine", &["a", "b", "c"]) }).unwrap_err();
        // a, a narrower, b, c: the same four requests the old once-per-run retry made.
        assert_eq!(a.log.borrow().len(), 4);
        let cov = f.coverage.unwrap();
        assert_eq!(cov["timed_out"], serde_json::json!(["a", "b", "c"]));
        assert_eq!(f.code, 3);
    }

    #[test]
    fn narrowing_shortens_the_window_then_drops_words() {
        let now = NOW;
        let Some(Narrow::After(d, _)) = narrower("coil whine", None, None, now) else { panic!() };
        assert_eq!(d, text::date(now - 180 * 86_400));
        let Some(Narrow::After(d, _)) = narrower("coil whine", Some("2026-01-01"), Some("2026-11-01"), now) else { panic!() };
        assert!(d.as_str() > "2026-05-01" && d.as_str() < "2026-07-01", "{d}");
        let short = narrower("rx 9070 xt freeze", Some("2026-10-20"), Some("2026-11-01"), now);
        assert_eq!(short, Some(Narrow::Text("rx 9070".into(), "searched for \"rx 9070\" only; dropped \"xt freeze\"".into())));
        assert_eq!(narrower("\"exact phrase here\"", Some("2026-10-20"), None, now), None, "a quoted phrase is not cut");
        assert_eq!(narrower("two words", Some("2026-10-20"), None, now), None);
    }

    #[test]
    fn the_budget_warning_comes_before_anything_is_sent() {
        let b = |used, blocked, frees| Budget { used, cap: 10, window_ms: 60_000, blocked_until_ms: blocked, frees_at_ms: frees };
        assert_eq!(budget_warning(&b(2, None, None), 5, 0), None);
        let low = budget_warning(&b(8, None, None), 5, 0).unwrap();
        assert!(low.starts_with("budget low: 2 of 10 requests left this minute"), "{low}");
        let full = budget_warning(&b(10, None, Some(30_500)), 1, 0).unwrap();
        assert!(full.ends_with("the next slot frees in 31 s"), "{full}");
        assert!(budget_warning(&b(0, Some(74_000), None), 1, 0).unwrap().starts_with("rate limited, retry in 74 s"));
    }

    #[test]
    fn sources_shows_the_shared_budget_and_search_reports_it() {
        let dir = std::env::temp_dir().join(format!("rdt-run-test-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.sqlite");
        let clock = FakeClock::default();
        clock.advance(NOW as u64 * 1000);
        let a = Arctic::new(Fixtures::default().route("posts/search", 200, &[], POSTS_SEARCH), clock).with_ledger(Box::new(Store::open(&path).unwrap()));
        let c = Ctx { store: Store::open(&path).unwrap(), arctic: &a, now: NOW, nonce: "ab12cd" };
        let out = sources(&c).unwrap();
        assert!(out.contains("budget   9 of 10 requests left this minute, shared by every rdt run on this PC"), "{out}");
        // Another run's 429 is seen here: the search sends nothing and says why.
        Store::open(&path).unwrap().block_until(a.now_ms() + 40_000).unwrap();
        let mut c = c;
        let f = search(&mut c, &SearchArgs { json: true, ..sargs("coil whine", &["a"]) }).unwrap_err();
        assert_eq!((f.code, a.log.borrow().len()), (5, 1), "only sources' one request was sent");
        assert!(f.coverage.unwrap()["budget_note"].as_str().unwrap().starts_with("rate limited, retry in 40 s"));
        drop(c);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cite_and_verify_quote_work_from_the_cache() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let out = cite(&c, &["t3_1xsjnud".into(), "t1_19sfqopne46x0".into(), "t1_nope".into()]).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[0].starts_with("[t3_1xsjnud] u/user_")
                && lines[0].ends_with("https://www.reddit.com/r/AMDHelp/comments/1xsjnud/about_sapphire_pulse_9060xt_16g_coil_whine/")
        );
        assert!(lines[1].ends_with("/about_sapphire_pulse_9060xt_16g_coil_whine/19sfqopne46x0/"));
        assert_eq!(lines[2], "[t1_nope] not in the cache");
        assert_eq!(cite(&c, &["t1_nope".into()]).unwrap_err().code, 3);
        assert!(verify_quote(&c, "t3_1xsjnud", "-120 mV and -10% power draw").is_ok());
        assert_eq!(verify_quote(&c, "t3_1xsjnud", "words nobody wrote").unwrap_err().code, 3);
        assert_eq!(verify_quote(&c, "t1_nope", "x").unwrap_err().code, 3);
    }

    #[test]
    fn cache_check_blanks_what_is_gone_and_distrusts_a_short_answer() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let cached = c.store.thread("1xsjnud").unwrap().unwrap().thread.comments;
        // The archive answers for all four comments: one is now removed by a moderator, one is missing, two are unchanged.
        let rows: Vec<serde_json::Value> = cached
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 3)
            .map(|(i, k)| serde_json::json!({"id": k.id, "link_id": "t3_1xsjnud", "parent_id": "t3_1xsjnud", "author": "x", "body": if i == 0 { "[removed]" } else { "still here" }}))
            .collect();
        let whole = serde_json::json!({"data": rows}).to_string();
        let short = arctic(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS).route("comments/ids", 200, &[], &whole));
        let mut later = Ctx { store: c.store, arctic: &short, now: NOW, nonce: "ab12cd" };
        // Three of four answered is not "nearly whole": the missing one is left alone and reported.
        let out = cache_check(&mut later).unwrap();
        assert!(out.starts_with("checked 1 posts and 4 comments against the archive; blanked 1 that"), "{out}");
        assert!(out.contains("1 items were missing from an incomplete answer and were left as they are"));
        let t = later.store.thread("1xsjnud").unwrap().unwrap().thread;
        let gone = t.comments.iter().find(|k| k.id == cached[0].id).unwrap();
        assert_eq!((gone.state, gone.body.as_str(), gone.author.clone()), (State::Removed, "", None));
        assert_eq!(t.comments.iter().filter(|k| k.state == State::Live).count(), 3);

        // An empty answer blanks nothing at all.
        let empty = arctic(Fixtures::default().route("posts/ids", 200, &[], r#"{"data":[]}"#).route("comments/ids", 200, &[], r#"{"data":[]}"#));
        let mut later = Ctx { store: later.store, arctic: &empty, now: NOW, nonce: "ab12cd" };
        let out = cache_check(&mut later).unwrap();
        assert!(out.contains("blanked 0 that") && out.contains("4 items were missing"), "{out}");
        assert_eq!(later.store.thread("1xsjnud").unwrap().unwrap().thread.post.state, State::Live);
    }

    #[test]
    fn a_thread_the_archive_dropped_is_blanked_not_served() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let dropped = arctic(Fixtures::default().route("posts/ids", 200, &[], r#"{"data":[]}"#));
        let mut later = Ctx { store: c.store, arctic: &dropped, now: NOW + FRESH_SECS + 1, nonce: "ab12cd" };
        let f = thread(&mut later, &args("1xsjnud")).unwrap_err();
        assert_eq!((f.code, f.message.as_str()), (3, "the archive no longer has t3_1xsjnud; its cached text has been blanked"));
        let offline = thread(&mut later, &ThreadArgs { offline: true, ..args("1xsjnud") }).unwrap();
        assert!(offline.contains("title=\"[removed]\"") && !offline.contains("u/user_") && !offline.contains("coil"), "{offline}");
        assert_eq!(search(&mut later, &SearchArgs { offline: true, ..sargs("coil whine", &[]) }).unwrap_err().code, 3);
    }

    #[test]
    fn offline_use_never_purges() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let mut later = Ctx { store: c.store, arctic: &a, now: NOW + (KEEP_DAYS + 5) * 86_400, nonce: "ab12cd" };
        assert!(thread(&mut later, &ThreadArgs { offline: true, ..args("1xsjnud") }).is_ok());
        assert!(search(&mut later, &SearchArgs { offline: true, ..sargs("coil whine", &[]) }).is_ok());
    }

    #[test]
    fn old_threads_are_purged_on_use_and_forget_works() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        assert!(cache_stats(&c).unwrap().starts_with("posts 1  (with comments 1)\ncomments 4\n"));
        assert_eq!(cache_forget(&mut c, "t3_1xsjnud").unwrap(), "deleted t3_1xsjnud and its comments from the cache\n");
        assert_eq!(cache_forget(&mut c, "t3_1xsjnud").unwrap_err().code, 3);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let mut later = Ctx { store: c.store, arctic: &a, now: NOW + (KEEP_DAYS + 1) * 86_400, nonce: "ab12cd" };
        assert_eq!(search(&mut later, &sargs("coil whine", &[])).unwrap_err().code, 3, "an online run deletes threads older than the keep window");
    }

    const SUBS: &str = include_str!("../../../fixtures/arctic/subs_prefix.json");

    fn subargs(topic: &str, urls: &[&str]) -> SubsArgs {
        SubsArgs { topic: topic.into(), urls: urls.iter().map(|u| (*u).to_owned()).collect(), offline: false, nsfw: false, json: false }
    }

    #[test]
    fn subs_ranks_by_urls_checks_size_and_proposes_a_search() {
        let big = r#"{"data":[{"display_name":"buildapc","subscribers":7400000,"over18":false,"public_description":"Planning on building a computer?","_meta":{"num_posts":4100000}}]}"#;
        let a = arctic(Fixtures::default().route("subreddit_prefix=amdhelp", 200, &[], SUBS).route("subreddit_prefix=buildapc", 200, &[], big).route(
            "subreddits/search",
            200,
            &[],
            r#"{"data":[]}"#,
        ));
        let mut c = ctx(&a);
        let urls = [
            "https://www.reddit.com/r/AMDHelp/comments/1xsjnud/a/",
            "https://old.reddit.com/r/amdhelp/comments/1x5sulf/b/",
            "https://www.reddit.com/r/buildapc/comments/1abc2de/c/",
            "https://www.reddit.com/r/Ghost_Sub/comments/1abc2df/d/",
            "https://example.com/not-reddit",
        ];
        let out = subs(&mut c, &subargs("coil whine", &urls)).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1], "source: urls: 4 of 5 name a subreddit");
        assert_eq!(lines[2], "source: cache: 0 threads on the topic");
        assert!(lines[3].starts_with("source: arctic: 3 requests (subreddits"), "{out}");
        assert_eq!(lines[4], "[1] r/AMDHelp  139k subscribers  182k archived posts  from: 2 URLs");
        assert_eq!(lines[5], "    busy: add --after to text searches");
        assert_eq!(lines[6], "    | Troubleshooting help Subreddit for all things AMD!");
        assert_eq!(lines[7], "[2] r/buildapc  7.4M subscribers  4.1M archived posts  from: 1 URL");
        assert_eq!(lines[8], "    very busy: text search will likely time out without a narrow --after/--before");
        assert!(out.contains("r/Ghost_Sub  size not looked up  from: 1 URL\n    not in the archive's list"), "{out}");
        assert!(
            out.ends_with(
                "left out of the search below as very busy: r/buildapc (search each alone with a narrow --after/--before)\nnext: rdt search 'coil whine' --sub AMDHelp --after 2025-11-13\n"
            ),
            "a missing subreddit and a very busy one are left out of the proposed search: {out}"
        );
        assert_eq!(a.log.borrow().len(), 3, "one check per candidate, and no guessing at names when the URLs already answered");
    }

    #[test]
    fn subs_uses_the_cache_and_a_topic_word_that_is_a_name() {
        let a = arctic(online().route("subreddits/search", 200, &[], SUBS));
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        let before = a.log.borrow().len();
        let offline = subs(&mut c, &SubsArgs { offline: true, ..subargs("coil whine", &[]) }).unwrap();
        assert!(offline.contains("[1] r/AMDHelp  size not looked up  from: 1 cached thread\n    size unknown"), "{offline}");
        assert!(offline.ends_with("next: rdt search 'coil whine' --sub AMDHelp\n"));
        assert_eq!(a.log.borrow().len(), before, "offline asks nothing");
        let named = subs(&mut c, &subargs("amdhelp driver timeout", &[])).unwrap();
        assert!(named.contains("[1] r/AMDHelp  139k subscribers  182k archived posts  from: a topic word is its name"), "{named}");
        assert!(!named.contains("AmdHelppp"), "a one-subscriber namesake is ignored");
        let json: serde_json::Value = serde_json::from_str(&subs(&mut c, &SubsArgs { json: true, ..subargs("amdhelp", &[]) }).unwrap()).unwrap();
        assert_eq!((json["subreddits"][0]["name"].as_str(), json["subreddits"][0]["in_archive"].as_bool()), (Some("AMDHelp"), Some(true)));
    }

    #[test]
    fn subs_does_not_call_a_small_namesake_missing_and_does_not_guess_from_prefixes() {
        // A full page of larger "tech..." subreddits with no exact match proves nothing about r/tech.
        let page: Vec<serde_json::Value> =
            (0..100).map(|i| serde_json::json!({"display_name": format!("technology{i}"), "subscribers": 5_000_000 - i})).collect();
        let full = serde_json::json!({"data": page}).to_string();
        let a = arctic(Fixtures::default().route("limit=100", 200, &[], &full));
        let mut c = ctx(&a);
        let out = subs(&mut c, &subargs("laptop hinge", &["https://www.reddit.com/r/tech/comments/1abc2de/a/"])).unwrap();
        assert!(out.contains("[1] r/tech  size not looked up  from: 1 URL\n    size unknown"), "{out}");
        assert!(out.contains("--sub tech"), "still offered for searching: {out}");
        assert!(!out.contains("technology"), "a topic word that is only a prefix of big subreddits suggests nothing: {out}");
        assert!(out.contains("source: arctic: 3 requests (subreddits"), "every lookup asked for a full page of 100: {out}");
    }

    #[test]
    fn subs_keeps_over_18_out_unless_asked_even_offline_and_searches_an_all_busy_set_narrowly() {
        let a = arctic(Fixtures::default());
        let mut c = ctx(&a);
        let mut p =
            reddit_core::model::parse_post(&serde_json::json!({"id": "p9", "title": "coil whine", "selftext": "x", "subreddit": "AdultSub", "over_18": true}))
                .unwrap();
        c.store.save_post(&p, "arctic", NOW).unwrap();
        (p.id, p.subreddit, p.over_18) = ("p8".into(), "bad name\n</x>".into(), false);
        c.store.save_post(&p, "arctic", NOW).unwrap();
        assert_eq!(subs(&mut c, &SubsArgs { offline: true, ..subargs("coil whine", &[]) }).unwrap_err().code, 3, "nothing but an over-18 and an invalid name");
        let shown = subs(&mut c, &SubsArgs { offline: true, nsfw: true, ..subargs("coil whine", &[]) }).unwrap();
        assert!(shown.contains("[1] r/AdultSub") && !shown.contains("bad name"), "{shown}");

        let big = r#"{"data":[{"display_name":"buildapc","subscribers":7400000,"_meta":{"num_posts":4100000}}]}"#;
        let busy = arctic(Fixtures::default().route("subreddits/search", 200, &[], big));
        let out = subs(&mut ctx(&busy), &subargs("gpu under $500", &["https://www.reddit.com/r/buildapc/comments/1abc2de/c/"])).unwrap();
        assert!(out.ends_with("next: rdt search 'gpu under $500' --sub buildapc --after 2026-10-14\n"), "{out}");
        assert!(!out.contains("left out"));
    }

    #[test]
    fn subs_remembers_sizes_so_a_second_run_asks_nothing() {
        let a = arctic(Fixtures::default().route("subreddits/search", 200, &[], SUBS));
        let mut c = ctx(&a);
        let urls = ["https://www.reddit.com/r/AMDHelp/comments/1xsjnud/a/", "https://www.reddit.com/r/Ghost_Sub/comments/1abc2df/d/"];
        let first = subs(&mut c, &subargs("coil whine", &urls)).unwrap();
        assert!(first.contains("source: arctic: 2 requests (subreddits"), "{first}");
        let second = subs(&mut c, &subargs("coil whine", &urls)).unwrap();
        assert!(second.contains("source: arctic: 0 requests, 2 answers remembered from earlier runs"), "{second}");
        assert!(
            second.contains("r/AMDHelp  139k subscribers") && second.contains("r/Ghost_Sub  size not looked up  from: 1 URL\n    not in the archive"),
            "{second}"
        );
        assert_eq!(a.log.borrow().len(), 2, "the second run made no request");
        let offline = subs(&mut c, &SubsArgs { offline: true, ..subargs("coil whine", &urls) }).unwrap();
        assert!(offline.contains("source: arctic: not asked (offline), 2 answers remembered") && offline.contains("r/AMDHelp  139k subscribers"), "{offline}");
        // A remembered over-18 answer hides the subreddit offline too; a "no such subreddit" is forgotten after a day.
        let adult = reddit_core::model::Subreddit { name: "AMDHelp".into(), subscribers: Some(5), over_18: true, description: String::new(), posts: None };
        c.store.save_subreddit("amdhelp", Some(&adult), NOW).unwrap();
        let hidden = subs(&mut c, &SubsArgs { offline: true, ..subargs("coil whine", &urls) }).unwrap();
        assert!(!hidden.contains("r/AMDHelp"), "{hidden}");
        let mut later = Ctx { store: c.store, arctic: &a, now: NOW + 2 * 86_400, nonce: "ab12cd" };
        let again = subs(&mut later, &subargs("coil whine", &["https://www.reddit.com/r/Ghost_Sub/comments/1abc2df/d/"])).unwrap();
        assert!(
            again.contains("source: arctic: 2 requests (subreddits"),
            "the day-old negative is asked again, plus one name lookup for a topic word: {again}"
        );
    }

    #[test]
    fn subs_ignores_a_cached_thread_that_shares_only_one_word_with_the_topic() {
        let a = arctic(online());
        let mut c = ctx(&a);
        thread(&mut c, &args("1xsjnud")).unwrap();
        // The cached thread is about coil whine on a 9060 XT; a question about 9070 XT fan curves shares only "xt".
        // Only the starter list speaks, then: the off-topic cached thread is not counted.
        let off = subs(&mut c, &SubsArgs { offline: true, ..subargs("9070 xt fan curve", &[]) }).unwrap();
        assert!(!off.contains("cached thread") && off.contains("starter list (9070)"), "{off}");
        assert_eq!(subs(&mut c, &SubsArgs { offline: true, ..subargs("sourdough proofing", &[]) }).unwrap_err().code, 3);
        assert!(subs(&mut c, &SubsArgs { offline: true, ..subargs("sapphire coil whine", &[]) }).unwrap().contains("r/AMDHelp"));
        // Plurals and filler words must not turn an on-topic thread away.
        assert!(subs(&mut c, &SubsArgs { offline: true, ..subargs("how to fix the coil whines on sapphire cards", &[]) }).unwrap().contains("r/AMDHelp"));
    }

    #[test]
    fn subs_works_from_the_starter_list_without_urls_and_asks_nothing_for_it() {
        let a = arctic(Fixtures::default());
        let out = subs(&mut ctx(&a), &SubsArgs { offline: true, ..subargs("RX 9070 XT hard freeze cyberpunk", &[]) }).unwrap();
        assert!(out.contains("starter list (rx)"), "{out}");
        assert!(out.contains("--sub radeon,AMDHelp,Amd,cyberpunkgame"), "hardware and the game first: {out}");
        assert!(out.contains(" --after "), "unknown sizes get a year's window: {out}");
        // Online, the starter names cost no lookups: only the old topic-word lookups (at most two) may be sent.
        let b = arctic(Fixtures::default().route("subreddits/search", 200, &[], r#"{"data":[]}"#));
        let json = subs(&mut ctx(&b), &SubsArgs { json: true, ..subargs("RX 9070 XT hard freeze cyberpunk", &[]) }).unwrap();
        assert!(b.log.borrow().len() <= MAX_NAME_LOOKUPS, "{:?}", b.log.borrow());
        assert!(json.contains("\"starter\":\"rx\""), "{json}");
        // With any real evidence (a URL), the list stays out of it.
        let c = arctic(Fixtures::default());
        let out =
            subs(&mut ctx(&c), &SubsArgs { offline: true, ..subargs("RX 9070 XT freeze", &["https://www.reddit.com/r/pcmasterrace/comments/abc123/x/"]) })
                .unwrap();
        assert!(out.contains("--sub pcmasterrace\n") && !out.contains("starter list"), "{out}");
    }

    #[test]
    fn subs_survives_a_rate_limit_and_says_when_it_has_nothing() {
        let a = arctic(Fixtures::default().route("subreddits/search", 429, &[("x-ratelimit-reset", "30")], "{}"));
        let mut c = ctx(&a);
        let out = subs(&mut c, &subargs("coil whine", &["https://www.reddit.com/r/AMDHelp/comments/1xsjnud/a/"])).unwrap();
        assert!(out.contains("source: arctic: 1 requests, then stopped: the archive is rate limiting; try again in 30 s"), "{out}");
        assert!(out.contains("[1] r/AMDHelp  size not looked up  from: 1 URL\n    size unknown"));
        assert_eq!(a.log.borrow().len(), 1, "no further lookups after a rate limit");
        // A lookup that times out is skipped; the ones after it still run.
        let slow = arctic(Fixtures::default().route("subreddit_prefix=amdhelp&", 422, &[], r#"{"data":null,"error":"Timeout. Maybe slow down a bit"}"#).route(
            "subreddits/search",
            200,
            &[],
            SUBS,
        ));
        let out = subs(
            &mut ctx(&slow),
            &subargs("coil whine", &["https://www.reddit.com/r/AMDHelp/comments/1xsjnud/a/", "https://www.reddit.com/r/AmdHelppp/comments/1xsjnue/b/"]),
        )
        .unwrap();
        assert!(out.contains("source: arctic: 3 requests, 1 timed out (subreddits"), "{out}");
        assert!(out.contains("r/AmdHelppp  1 subscribers"), "{out}");
        let f = subs(&mut c, &SubsArgs { offline: true, ..subargs("zebra crossing", &[]) }).unwrap_err();
        assert_eq!(f.code, 3);
        assert!(f.message.contains("pass the Reddit URLs a web search found"));
    }

    #[test]
    fn sources_reports_freshness() {
        let a = arctic(Fixtures::default().route("posts/search", 200, &[], POSTS_SEARCH));
        let out = sources(&ctx(&a)).unwrap();
        assert!(out.contains("arctic   reachable; newest post 2026-11-10T10:37Z"), "{out}");
        assert!(out.contains("lagging"));
        let down = arctic(Fixtures::default());
        assert!(sources(&ctx(&down)).unwrap().contains("arctic   the archive is unavailable"));
    }
}
