//! The Arctic Shift archive (arctic-shift.photon-reddit.com): a volunteer service, no account, no guarantees.
//!
//! Manners: at most one request every 2 seconds; a rate-limit wait of up to 10 seconds is honoured once, a longer
//! one is reported with its wait time unless the caller asked for patience (`--wait`); a query the archive times out
//! on gets one retry after 5 seconds, once per run; nothing else is
//! retried here, and a refusal (401/403) is final. Confirmed against live responses on 2026-11-12:
//! `{"data": [...]}` on success, `{"data": null, "error": "..."}` with status 422 on a query timeout,
//! and `x-ratelimit-reset` (seconds) on every response.

use crate::transport::{Clock, Transport};
use reddit_core::model::{self, Comment, MoreStub, Post, Subreddit, Thread};
use serde_json::Value;
use std::cell::{Cell, RefCell};

pub const BASE: &str = "https://arctic-shift.photon-reddit.com";
pub const NAME: &str = "arctic";
const GAP_MS: u64 = 2000;
const ID_BATCH: usize = 500;
const SHORT_WAIT_S: u64 = 10;
const TIMEOUT_PAUSE_MS: u64 = 5000;

#[derive(Debug, PartialEq)]
pub enum Error {
    /// The archive has no such thread.
    NotFound(String),
    RateLimited {
        retry_after_s: u64,
    },
    /// 401 or 403. Final: the tool does not try again in any other way.
    Refused(u16),
    /// The query timed out on the archive's side; it needs a narrower date range or a quieter subreddit.
    TooBroad(String),
    /// No response, a server error, or a body that is not what the archive normally sends.
    Unavailable(String),
    /// The archive cannot do what was asked.
    Unsupported(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NotFound(id) => write!(f, "the archive has no thread t3_{id}"),
            Error::RateLimited { retry_after_s } => write!(f, "the archive is rate limiting; try again in {retry_after_s} s"),
            Error::Refused(status) => write!(f, "the archive refused the request (HTTP {status}); not retrying"),
            Error::TooBroad(msg) => {
                write!(f, "the archive timed out on this query ({msg}); wait a minute and try again, or narrow it with --after/--before or a smaller subreddit")
            }
            Error::Unavailable(msg) => write!(f, "the archive is unavailable: {msg}"),
            Error::Unsupported(msg) => write!(f, "{msg}"),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Query<'a> {
    pub subreddit: &'a str,
    pub text: &'a str,
    /// `YYYY-MM-DD`.
    pub after: Option<&'a str>,
    pub before: Option<&'a str>,
    pub limit: usize,
}

pub struct Arctic<T: Transport, C: Clock> {
    transport: T,
    clock: C,
    last_ms: Cell<Option<u64>>,
    /// The longest rate-limit wait this client sits out itself before handing the wait back to the caller.
    max_wait_s: Cell<u64>,
    /// A timed-out query gets one calmer retry, once per run: after that the archive has made its point.
    timeout_retry_left: Cell<bool>,
    /// (endpoint, status) of every request made, for the request log. Status 0 means no response.
    pub log: RefCell<Vec<(String, u16)>>,
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b',') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

impl<T: Transport, C: Clock> Arctic<T, C> {
    pub fn new(transport: T, clock: C) -> Self {
        Arctic {
            transport,
            clock,
            last_ms: Cell::new(None),
            max_wait_s: Cell::new(SHORT_WAIT_S),
            timeout_retry_left: Cell::new(true),
            log: RefCell::new(Vec::new()),
        }
    }

    /// Lets the caller choose patience: with `--wait`, a rate limit of up to this many seconds is waited out once.
    pub fn set_max_wait(&self, seconds: u64) {
        self.max_wait_s.set(seconds);
    }

    fn get(&self, endpoint: &str, params: &[(&str, String)]) -> Result<Vec<Value>, Error> {
        match self.get_once(endpoint, params) {
            // The archive's limit resets every minute. A wait within the caller's patience is honoured once here;
            // anything longer goes back to the caller with the wait time.
            Err(Error::RateLimited { retry_after_s }) if retry_after_s <= self.max_wait_s.get() => {
                self.clock.sleep_ms((retry_after_s + 1) * 1000);
                self.get_once(endpoint, params)
            }
            // "Timeout. Maybe slow down a bit" is often load at the archive rather than the query: one calmer retry,
            // and only once per run, so a loop over several subreddits cannot double its requests.
            Err(Error::TooBroad(_)) if self.timeout_retry_left.replace(false) => {
                self.clock.sleep_ms(TIMEOUT_PAUSE_MS);
                self.get_once(endpoint, params)
            }
            other => other,
        }
    }

    fn get_once(&self, endpoint: &str, params: &[(&str, String)]) -> Result<Vec<Value>, Error> {
        if let Some(last) = self.last_ms.get() {
            let since = self.clock.now_ms().saturating_sub(last);
            if since < GAP_MS {
                self.clock.sleep_ms(GAP_MS - since);
            }
        }
        let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
        let url = format!("{BASE}/api/{endpoint}?{}", query.join("&"));
        let result = self.transport.get(&url);
        self.last_ms.set(Some(self.clock.now_ms()));
        self.log.borrow_mut().push((endpoint.to_owned(), result.as_ref().map_or(0, |r| r.status)));
        let resp = result.map_err(Error::Unavailable)?;
        let json: Option<Value> = serde_json::from_str(&resp.body).ok();
        let message = json.as_ref().and_then(|j| j.get("error")).and_then(Value::as_str).unwrap_or("").to_owned();
        match resp.status {
            200 if message.is_empty() => match json.and_then(|mut j| j.get_mut("data").map(Value::take)) {
                Some(Value::Array(rows)) => Ok(rows),
                _ => Err(Error::Unavailable("unexpected response shape".into())),
            },
            429 => {
                Err(Error::RateLimited { retry_after_s: resp.header("x-ratelimit-reset").and_then(|v| v.parse::<f64>().ok()).map_or(60, |s| s.ceil() as u64) })
            }
            401 | 403 => Err(Error::Refused(resp.status)),
            200 | 400 | 422 if message.to_lowercase().contains("timeout") || message.to_lowercase().contains("timed out") => Err(Error::TooBroad(message)),
            200 | 400 | 422 => Err(Error::Unsupported(format!("the archive rejected the query: {message}"))),
            s => Err(Error::Unavailable(format!("HTTP {s}"))),
        }
    }

    pub fn posts_by_ids(&self, ids: &[String]) -> Result<Vec<Post>, Error> {
        let mut out = Vec::new();
        for chunk in ids.chunks(ID_BATCH) {
            out.extend(self.get("posts/ids", &[("ids", chunk.join(","))])?.iter().filter_map(model::parse_post));
        }
        Ok(out)
    }

    pub fn comments_by_ids(&self, ids: &[String]) -> Result<Vec<Comment>, Error> {
        let mut out = Vec::new();
        for chunk in ids.chunks(ID_BATCH) {
            out.extend(self.get("comments/ids", &[("ids", chunk.join(","))])?.iter().filter_map(model::parse_flat_comment));
        }
        Ok(out)
    }

    /// The post and up to `limit` comments of its tree.
    pub fn thread(&self, id: &str, limit: usize) -> Result<Thread, Error> {
        let post = self.posts_by_ids(&[id.to_owned()])?.into_iter().next().ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let (comments, more) = self.tree(id, None, limit)?;
        Ok(Thread { post, comments, more })
    }

    /// Replies under one comment that an earlier fetch left collapsed.
    pub fn tree(&self, post_id: &str, parent: Option<&str>, limit: usize) -> Result<(Vec<Comment>, Vec<MoreStub>), Error> {
        let mut params = vec![("link_id", post_id.to_owned()), ("limit", limit.clamp(1, 25_000).to_string())];
        if let Some(p) = parent {
            params.push(("parent_id", p.to_owned()));
        }
        Ok(model::parse_tree(&self.get("comments/tree", &params)?, post_id))
    }

    fn search_params(q: &Query, text_key: &str) -> Result<Vec<(&'static str, String)>, Error> {
        if q.subreddit.is_empty() {
            return Err(Error::Unsupported("the archive can only search text inside one subreddit; pass --sub".into()));
        }
        let mut p = vec![("subreddit", q.subreddit.to_owned()), ("limit", q.limit.clamp(1, 100).to_string()), ("sort", "desc".to_owned())];
        if !q.text.is_empty() {
            p.push((if text_key == "body" { "body" } else { "query" }, q.text.to_owned()));
        }
        for (key, value) in [("after", q.after), ("before", q.before)] {
            if let Some(v) = value {
                p.push((key, v.to_owned()));
            }
        }
        Ok(p)
    }

    /// Posts in one subreddit whose title or text matches, newest first (the archive cannot sort by relevance or score).
    pub fn search_posts(&self, q: &Query) -> Result<Vec<Post>, Error> {
        Ok(self.get("posts/search", &Self::search_params(q, "query")?)?.iter().filter_map(model::parse_post).collect())
    }

    /// Comments in one subreddit whose body matches, newest first.
    pub fn search_comments(&self, q: &Query) -> Result<Vec<Comment>, Error> {
        Ok(self.get("comments/search", &Self::search_params(q, "body")?)?.iter().filter_map(model::parse_flat_comment).collect())
    }

    /// Subreddits whose name starts with `prefix`, largest first. The archive matches names only, never topics.
    pub fn subreddits(&self, prefix: &str, limit: usize) -> Result<Vec<Subreddit>, Error> {
        Ok(self.subreddits_page(prefix, limit)?.0)
    }

    /// The same, with how many rows the archive sent before parsing: a full page means there may be more.
    pub fn subreddits_page(&self, prefix: &str, limit: usize) -> Result<(Vec<Subreddit>, usize), Error> {
        let params = [("subreddit_prefix", prefix.to_owned()), ("limit", limit.clamp(1, 100).to_string())];
        let rows = self.get("subreddits/search", &params)?;
        Ok((rows.iter().filter_map(model::parse_subreddit).collect(), rows.len()))
    }

    /// Creation time of the newest post the archive holds: tells a live archive from a frozen one.
    pub fn newest(&self) -> Result<Option<i64>, Error> {
        Ok(self.get("posts/search", &[("limit", "1".into()), ("sort", "desc".into())])?.first().and_then(model::parse_post).map(|p| p.created))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{FakeClock, Fixtures};
    use reddit_core::model::State;

    const POSTS_IDS: &str = include_str!("../../../fixtures/arctic/posts_ids.json");
    const TREE: &str = include_str!("../../../fixtures/arctic/tree.json");
    const POSTS_SEARCH: &str = include_str!("../../../fixtures/arctic/posts_search.json");
    const COMMENTS_SEARCH: &str = include_str!("../../../fixtures/arctic/comments_search.json");
    const TIMEOUT: &str = include_str!("../../../fixtures/arctic/err_timeout_422.json");

    fn arctic(f: Fixtures) -> Arctic<Fixtures, FakeClock> {
        Arctic::new(f, FakeClock::default())
    }

    #[test]
    fn thread_from_recorded_responses() {
        let a = arctic(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS).route("comments/tree", 200, &[], TREE));
        let t = a.thread("1xsjnud", 500).unwrap();
        assert_eq!((t.post.subreddit.as_str(), t.comments.len(), t.post.state), ("AMDHelp", 4, State::Live));
        let calls = a.transport.calls.borrow();
        assert_eq!(calls[0], format!("{BASE}/api/posts/ids?ids=1xsjnud"));
        assert_eq!(calls[1], format!("{BASE}/api/comments/tree?link_id=1xsjnud&limit=500"));
        assert_eq!(*a.log.borrow(), [("posts/ids".to_owned(), 200), ("comments/tree".to_owned(), 200)]);
    }

    #[test]
    fn requests_are_paced() {
        let a = arctic(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS).route("comments/tree", 200, &[], TREE));
        a.thread("1xsjnud", 50).unwrap();
        assert_eq!(*a.clock.sleeps.borrow(), [GAP_MS], "the second request waits out the gap");
        a.clock.advance(5000);
        a.posts_by_ids(&["1xsjnud".into()]).unwrap();
        assert_eq!(a.clock.sleeps.borrow().len(), 1, "no wait when enough time has passed");
    }

    #[test]
    fn missing_thread_is_not_found_without_asking_for_comments() {
        let a = arctic(Fixtures::default().route("posts/ids", 200, &[], r#"{"data":[]}"#));
        assert_eq!(a.thread("zzzzzzz", 50), Err(Error::NotFound("zzzzzzz".into())));
        assert_eq!(a.transport.calls.borrow().len(), 1);
    }

    #[test]
    fn search_encodes_the_query_and_needs_a_subreddit() {
        let a = arctic(Fixtures::default().route("posts/search", 200, &[], POSTS_SEARCH).route("comments/search", 200, &[], COMMENTS_SEARCH));
        let q = Query { subreddit: "AMDHelp", text: "\"coil whine\" & more", after: Some("2026-08-01"), before: None, limit: 3 };
        assert_eq!(a.search_posts(&q).unwrap().len(), 3);
        assert_eq!(
            a.transport.calls.borrow()[0],
            format!("{BASE}/api/posts/search?subreddit=AMDHelp&limit=3&sort=desc&query=%22coil%20whine%22%20%26%20more&after=2026-08-01")
        );
        let comments = a.search_comments(&q).unwrap();
        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].post_id, "1xsjnud");
        assert!(a.transport.calls.borrow()[1].contains("&body=%22coil"));
        assert!(matches!(a.search_posts(&Query { text: "x", limit: 5, ..Query::default() }), Err(Error::Unsupported(_))));
        assert_eq!(a.transport.calls.borrow().len(), 2, "an unsupported search sends nothing");
    }

    #[test]
    fn errors_map_and_only_a_timeout_is_retried_once() {
        type Case = (u16, &'static [(&'static str, &'static str)], &'static str, Error);
        let cases: [Case; 6] = [
            (422, &[], TIMEOUT, Error::TooBroad("Timeout. Maybe slow down a bit".into())),
            (429, &[("X-RateLimit-Reset", "56")], "{}", Error::RateLimited { retry_after_s: 56 }),
            (429, &[], "slow down", Error::RateLimited { retry_after_s: 60 }),
            (403, &[], "<html>blocked</html>", Error::Refused(403)),
            (502, &[], "bad gateway", Error::Unavailable("HTTP 502".into())),
            (200, &[], "<html>maintenance</html>", Error::Unavailable("unexpected response shape".into())),
        ];
        for (status, headers, body, want) in cases {
            let a = arctic(Fixtures::default().route("posts/ids", status, headers, body));
            assert_eq!(a.posts_by_ids(&["x".into()]), Err(want));
            let allowed = if status == 422 { 2 } else { 1 };
            assert_eq!(a.transport.calls.borrow().len(), allowed, "status {status}: a timeout gets one calmer retry, nothing else is retried");
        }
        // The calmer retry happens once per run: a second timed-out query is not retried.
        let a = arctic(Fixtures::default().route("posts/ids", 422, &[], TIMEOUT));
        assert!(a.posts_by_ids(&["x".into()]).is_err() && a.posts_by_ids(&["y".into()]).is_err());
        assert_eq!(a.transport.calls.borrow().len(), 3);
        let a = arctic(Fixtures::default());
        assert!(matches!(a.newest(), Err(Error::Unavailable(_))), "no response at all");
        assert_eq!(*a.log.borrow(), [("posts/search".to_owned(), 0)]);
    }

    #[test]
    fn a_short_rate_limit_is_waited_out_once_and_a_long_one_is_not() {
        let a = arctic(Fixtures::default().route("posts/ids", 429, &[("x-ratelimit-reset", "3")], "{}"));
        assert_eq!(a.posts_by_ids(&["x".into()]), Err(Error::RateLimited { retry_after_s: 3 }));
        assert_eq!(a.transport.calls.borrow().len(), 2, "one retry, no more");
        assert_eq!(*a.clock.sleeps.borrow(), [4000]);
        let long = arctic(Fixtures::default().route("posts/ids", 429, &[("x-ratelimit-reset", "48")], "{}"));
        assert!(long.posts_by_ids(&["x".into()]).is_err());
        assert_eq!((long.transport.calls.borrow().len(), long.clock.sleeps.borrow().len()), (1, 0));
        // With more patience (--wait), the same 48 seconds is waited out, once.
        long.set_max_wait(90);
        assert_eq!(long.posts_by_ids(&["x".into()]), Err(Error::RateLimited { retry_after_s: 48 }));
        assert_eq!(long.transport.calls.borrow().len(), 3);
        assert_eq!(long.clock.sleeps.borrow().iter().max(), Some(&49_000));
    }

    #[test]
    fn subreddits_by_name_prefix() {
        let a = arctic(Fixtures::default().route("subreddits/search", 200, &[], include_str!("../../../fixtures/arctic/subs_prefix.json")));
        let subs = a.subreddits("amdhel", 5).unwrap();
        assert_eq!(subs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["AMDHelp", "AmdHelppp"]);
        assert_eq!(a.transport.calls.borrow()[0], format!("{BASE}/api/subreddits/search?subreddit_prefix=amdhel&limit=5"));
    }

    #[test]
    fn ids_are_batched() {
        let a = arctic(Fixtures::default().route("comments/ids", 200, &[], COMMENTS_SEARCH));
        let ids: Vec<String> = (0..1001).map(|i| format!("c{i}")).collect();
        a.comments_by_ids(&ids).unwrap();
        assert_eq!(a.transport.calls.borrow().len(), 3);
    }

    #[test]
    fn newest_reads_the_latest_post_time() {
        let a = arctic(Fixtures::default().route("posts/search", 200, &[], POSTS_SEARCH));
        assert_eq!(a.newest().unwrap(), Some(1_794_307_045));
    }
}
