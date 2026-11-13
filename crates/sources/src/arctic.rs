//! The Arctic Shift archive (arctic-shift.photon-reddit.com): a volunteer service, no account, no guarantees.
//!
//! Manners: at most one request every 2 seconds; a rate-limit wait of up to 10 seconds is honoured once, a longer
//! one is reported with its wait time unless the caller asked for patience (`--wait`); a query the archive times out
//! on gets one retry after 5 seconds, once per run; nothing else is
//! retried here, and a refusal (401/403) is final. Confirmed against live responses on 2026-11-12:
//! `{"data": [...]}` on success, `{"data": null, "error": "..."}` with status 422 on a query timeout,
//! and `x-ratelimit-reset` (seconds) on every response.
//!
//! Machine-wide: with a `Ledger` (the cache's request log), every request is booked before it is sent, so all `rdt`
//! processes on this PC share one pace: 2 s apart, at most 10 in any minute, and nothing at all while the archive's
//! last rate limit is still running. A booking further off than the caller will wait is not made and nothing is sent.

use crate::transport::{Clock, Transport};
use reddit_core::model::{self, Comment, MoreStub, Post, Subreddit, Thread};
use reddit_core::store::{Booking, Budget, Pacing, Store};
use serde_json::Value;
use std::cell::{Cell, RefCell};

pub const BASE: &str = "https://arctic-shift.photon-reddit.com";
pub const NAME: &str = "arctic";
const GAP_MS: u64 = 2000;
const ID_BATCH: usize = 500;
const SHORT_WAIT_S: u64 = 10;
const TIMEOUT_PAUSE_MS: u64 = 5000;
/// The machine-wide pace every rdt process keeps. A dozen requests in a burst has locked us out for a minute or two;
/// the archive's limit resets on the minute.
pub const PACING: Pacing = Pacing { gap_ms: GAP_MS, cap: 10, window_ms: 60_000 };
/// How long a request waits for a free slot in the machine-wide budget before giving up (longer with `--wait`).
const SLOT_WAIT_MS: u64 = 60_000;
/// The longest rate-limit wait believed from the archive's header.
const MAX_REPORTED_WAIT_S: u64 = 3600;
/// The longest a rate limit seen by one run holds back every other run on this PC.
const MAX_SHARED_BLOCK_S: u64 = 300;

/// Machine-wide bookkeeping of requests, shared by every `rdt` process: the cache's request log.
pub trait Ledger {
    /// `clock` is read once the ledger's lock is held. An `Err` means the request could not be booked.
    fn book(&self, clock: &dyn Fn() -> u64, endpoint: &str, max_wait_ms: u64) -> Result<Booking, String>;
    fn settle(&self, id: i64, status: u16);
    fn block_until(&self, until_ms: u64);
    fn budget(&self, now_ms: u64) -> Option<Budget>;
}

impl Ledger for Store {
    fn book(&self, clock: &dyn Fn() -> u64, endpoint: &str, max_wait_ms: u64) -> Result<Booking, String> {
        self.book_request_with(clock, NAME, endpoint, PACING, max_wait_ms).map_err(|e| e.to_string())
    }
    fn settle(&self, id: i64, status: u16) {
        let _ = self.settle_request(id, status);
    }
    fn block_until(&self, until_ms: u64) {
        let _ = Store::block_until(self, until_ms);
    }
    fn budget(&self, now_ms: u64) -> Option<Budget> {
        Store::budget(self, now_ms, PACING).ok()
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    /// The archive has no such thread.
    NotFound(String),
    RateLimited {
        retry_after_s: u64,
    },
    /// Not sent: the requests every rdt process on this PC made in the last minute used up the shared budget.
    Busy {
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
            Error::Busy { retry_after_s } => write!(
                f,
                "request budget used up: {} requests a minute are shared by every rdt run on this PC; not sent, try again in {retry_after_s} s",
                PACING.cap
            ),
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
    /// Books every request machine-wide when set; it also records them, so `log` is then for this run only.
    ledger: Option<Box<dyn Ledger>>,
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
            ledger: None,
        }
    }

    /// Paces every request against all rdt processes on this machine, through the ledger.
    pub fn with_ledger(mut self, ledger: Box<dyn Ledger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    pub fn has_ledger(&self) -> bool {
        self.ledger.is_some()
    }

    /// The machine-wide budget right now, when there is a ledger.
    pub fn budget(&self) -> Option<Budget> {
        self.ledger.as_ref().and_then(|l| l.budget(self.clock.now_ms()))
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    /// The caller retries timed-out searches itself, narrower and per subreddit: the blind once-per-run retry is
    /// switched off so a timeout is never retried twice.
    pub fn take_timeout_retry(&self) {
        self.timeout_retry_left.set(false);
    }

    /// The calm pause before a narrower retry of a query the archive timed out on.
    pub fn pause_after_timeout(&self) {
        self.clock.sleep_ms(TIMEOUT_PAUSE_MS);
    }

    /// Seconds from now until `at_ms`, rounded up.
    fn secs_until(&self, at_ms: u64) -> u64 {
        at_ms.saturating_sub(self.clock.now_ms()).div_ceil(1000)
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
        let mut booked = None;
        if let Some(ledger) = &self.ledger {
            let clock = || self.clock.now_ms();
            // A request the ledger cannot book is not sent: unbooked, other processes could not pace against it.
            match ledger.book(&clock, endpoint, self.max_wait_s.get().saturating_mul(1000).max(SLOT_WAIT_MS)) {
                Ok(Booking::Slot { id, send_at_ms }) => {
                    let now = self.clock.now_ms();
                    if send_at_ms > now {
                        self.clock.sleep_ms(send_at_ms - now);
                    }
                    booked = Some(id);
                }
                Ok(Booking::Blocked { until_ms }) => return Err(Error::RateLimited { retry_after_s: self.secs_until(until_ms).max(1) }),
                Ok(Booking::Busy { free_at_ms }) => return Err(Error::Busy { retry_after_s: self.secs_until(free_at_ms).max(1) }),
                Err(e) => return Err(Error::Unavailable(format!("not sent: the request could not be booked in the cache's request log ({e})"))),
            }
        }
        let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
        let url = format!("{BASE}/api/{endpoint}?{}", query.join("&"));
        let result = self.transport.get(&url);
        let status = result.as_ref().map_or(0, |r| r.status);
        self.last_ms.set(Some(self.clock.now_ms()));
        self.log.borrow_mut().push((endpoint.to_owned(), status));
        if let (Some(ledger), Some(id)) = (&self.ledger, booked) {
            ledger.settle(id, status);
        }
        let resp = result.map_err(Error::Unavailable)?;
        let json: Option<Value> = serde_json::from_str(&resp.body).ok();
        let message = json.as_ref().and_then(|j| j.get("error")).and_then(Value::as_str).unwrap_or("").to_owned();
        match resp.status {
            200 if message.is_empty() => match json.and_then(|mut j| j.get_mut("data").map(Value::take)) {
                Some(Value::Array(rows)) => Ok(rows),
                _ => Err(Error::Unavailable("unexpected response shape".into())),
            },
            429 => {
                // The header is seconds to the minute's reset; a value far beyond that (a timestamp, "inf") is not
                // believed, so one odd answer can never lock this PC out for long.
                let retry_after_s = resp
                    .header("x-ratelimit-reset")
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|s| s.is_finite())
                    .map_or(60, |s| s.ceil().clamp(0.0, MAX_REPORTED_WAIT_S as f64) as u64);
                // Every other rdt process on this machine holds off until the archive's window resets.
                if let Some(ledger) = &self.ledger {
                    ledger.block_until(self.clock.now_ms().saturating_add(retry_after_s.min(MAX_SHARED_BLOCK_S) * 1000));
                }
                Err(Error::RateLimited { retry_after_s })
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

    /// The same, sent once with no retry of any kind: for a retry the caller already chose to make.
    pub fn search_posts_once(&self, q: &Query) -> Result<Vec<Post>, Error> {
        Ok(self.get_once("posts/search", &Self::search_params(q, "query")?)?.iter().filter_map(model::parse_post).collect())
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

    fn shared_cache(tag: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rdt-src-test-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("c.sqlite")
    }

    fn with_ledger(f: Fixtures, path: &std::path::Path, start_ms: u64) -> Arctic<Fixtures, FakeClock> {
        let a = Arctic::new(f, FakeClock::default()).with_ledger(Box::new(Store::open(path).unwrap()));
        a.clock.advance(start_ms);
        a
    }

    const T0: u64 = 1_794_547_000_000;

    #[test]
    fn two_runs_share_one_pace() {
        let path = shared_cache(line!());
        let a = with_ledger(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS), &path, T0);
        let b = with_ledger(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS), &path, T0);
        a.posts_by_ids(&["x".into()]).unwrap();
        b.posts_by_ids(&["x".into()]).unwrap();
        assert_eq!((a.clock.sleeps.borrow().len(), b.clock.sleeps.borrow().clone()), (0, vec![GAP_MS]), "the second process waits for the first one's gap");
        let budget = a.budget().unwrap();
        assert_eq!((budget.used, budget.left()), (2, PACING.cap - 2));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_rate_limit_seen_by_one_run_stops_the_others_without_a_request() {
        let path = shared_cache(line!());
        let a = with_ledger(Fixtures::default().route("posts/ids", 429, &[("x-ratelimit-reset", "48")], "{}"), &path, T0);
        let b = with_ledger(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS), &path, T0);
        assert_eq!(a.posts_by_ids(&["x".into()]), Err(Error::RateLimited { retry_after_s: 48 }));
        b.clock.advance(3000);
        assert_eq!(b.posts_by_ids(&["x".into()]), Err(Error::RateLimited { retry_after_s: 45 }));
        assert!(b.transport.calls.borrow().is_empty(), "nothing sent while the archive's limit runs");
        // With patience (--wait), the rest of the limit is waited out once and the request then goes.
        b.set_max_wait(90);
        assert_eq!(b.posts_by_ids(&["x".into()]).unwrap().len(), 1);
        assert_eq!(b.transport.calls.borrow().len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn an_odd_rate_limit_header_cannot_lock_the_pc_out() {
        for (i, (header, reported)) in [("1794547000", MAX_REPORTED_WAIT_S), ("inf", 60), ("NaN", 60), ("-5", 0)].into_iter().enumerate() {
            let path = shared_cache(line!() * 10 + i as u32);
            let a = with_ledger(Fixtures::default().route("posts/ids", 429, &[("x-ratelimit-reset", header)], "{}"), &path, T0);
            assert_eq!(a.posts_by_ids(&["x".into()]), Err(Error::RateLimited { retry_after_s: reported }), "header {header}");
            let blocked = Store::open(&path).unwrap().blocked_until(0).unwrap();
            assert!(blocked.is_some_and(|t| t <= T0 + MAX_SHARED_BLOCK_S * 1000), "header {header}: {blocked:?}");
            std::fs::remove_dir_all(path.parent().unwrap()).ok();
        }
    }

    struct BrokenLedger;
    impl Ledger for BrokenLedger {
        fn book(&self, _: &dyn Fn() -> u64, _: &str, _: u64) -> Result<Booking, String> {
            Err("disk I/O error".into())
        }
        fn settle(&self, _: i64, _: u16) {}
        fn block_until(&self, _: u64) {}
        fn budget(&self, _: u64) -> Option<Budget> {
            None
        }
    }

    #[test]
    fn a_request_the_ledger_cannot_book_is_not_sent() {
        let a = Arctic::new(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS), FakeClock::default()).with_ledger(Box::new(BrokenLedger));
        assert!(matches!(a.posts_by_ids(&["x".into()]), Err(Error::Unavailable(m)) if m.contains("could not be booked")));
        assert!(a.transport.calls.borrow().is_empty());
    }

    #[test]
    fn the_clock_is_read_after_the_lock_is_taken() {
        let path = shared_cache(line!());
        let s = Store::open(&path).unwrap();
        // A caller that waited for the lock books from the moment it got it, not from when it began to wait.
        let reads = std::cell::Cell::new(0);
        let clock = || {
            reads.set(reads.get() + 1);
            T0 + 5000
        };
        assert!(matches!(s.book_request_with(&clock, NAME, "x", PACING, 0).unwrap(), Booking::Slot { send_at_ms, .. } if send_at_ms == T0 + 5000));
        assert_eq!(reads.get(), 1);
        drop(s);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_full_minute_is_waited_for_or_reported_but_never_exceeded() {
        let path = shared_cache(line!());
        let a = with_ledger(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS), &path, T0);
        for _ in 0..PACING.cap {
            a.posts_by_ids(&["x".into()]).unwrap();
        }
        // The 11th request in the minute waits for the first to leave the window: 60 s after it, never sooner.
        a.posts_by_ids(&["x".into()]).unwrap();
        assert_eq!(a.clock.now_ms(), T0 + 60_000);
        // Another process that finds the minute full and would have to wait past its patience sends nothing.
        let b = with_ledger(Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS), &path, T0 + 60_000);
        let store = Store::open(&path).unwrap();
        for i in 0..PACING.cap {
            assert!(matches!(store.book_request(T0 + 60_000 + 2000 * (i + 1), NAME, "x", PACING, u64::MAX).unwrap(), Booking::Slot { .. }));
        }
        assert!(matches!(b.posts_by_ids(&["x".into()]), Err(Error::Busy { .. })));
        assert!(b.transport.calls.borrow().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
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
