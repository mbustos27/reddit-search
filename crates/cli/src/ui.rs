//! `rdt ui`: a read-only window onto the cache, served on this machine only.
//!
//! Rules: bound to 127.0.0.1; answers only requests addressed to itself (so another site cannot reach it through a
//! DNS trick); GET only; nothing here writes to the cache or fetches a thread. The one network action is the explicit
//! "check the archive" button, which makes a single status request. Thread text leaves as JSON and is placed in the
//! page as text, never as markup; the page may load scripts and styles only from this server.
//!
//! Known limit: requests are answered one at a time, so a local program that connects and never reads its answer
//! can stall the window until it disconnects. Nothing outside this machine can connect at all.

use crate::run::{self, Ctx, Fail, SearchArgs, ThreadArgs};
use reddit_core::ids::{self, Ref};
use reddit_core::render;
use reddit_core::text;
use reddit_sources::transport::{Clock, Transport};
use serde_json::json;

pub const DEFAULT_PORT: u16 = 8779;
/// Longer addresses are refused before anything is parsed; no search needs more.
const MAX_URL: usize = 1000;
/// The header the page's script adds to its own requests.
pub const PAGE_MARKER: &str = "X-Rdt";
const INDEX: &str = include_str!("ui/index.html");
const APP_JS: &str = include_str!("ui/app.js");
const APP_CSS: &str = include_str!("ui/app.css");
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

fn plain(status: u16, body: &str) -> Reply {
    Reply { status, content_type: "text/plain; charset=utf-8", body: body.to_owned() }
}

fn data(status: u16, value: &serde_json::Value) -> Reply {
    Reply { status, content_type: "application/json; charset=utf-8", body: value.to_string() }
}

fn failed(f: &Fail) -> Reply {
    data(if f.code == 6 { 500 } else { 404 }, &json!({"ok": false, "code": f.code, "error": f.message}))
}

/// Percent-decodes one query value; `+` is a space. Bytes that are not UTF-8 become replacement characters.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if bytes.get(i + 1).copied().and_then(hex).is_some() && bytes.get(i + 2).copied().and_then(hex).is_some() => {
                out.push(hex(bytes[i + 1]).unwrap_or(0) * 16 + hex(bytes[i + 2]).unwrap_or(0));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn param(query: &str, name: &str) -> Option<String> {
    query.split('&').filter_map(|pair| pair.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| decode(v))
}

/// What the server needs to know about one request.
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub host: Option<&'a str>,
    pub origin: Option<&'a str>,
    /// The browser's own statement of where the request came from (`Sec-Fetch-Site`), when it sends one.
    pub fetch_site: Option<&'a str>,
    /// The page's script marks its requests with a header another site cannot add without asking first.
    pub from_page: bool,
}

/// True when the browser says the request came from a different site.
pub fn cross_site(fetch_site: Option<&str>) -> bool {
    fetch_site.is_some_and(|s| !s.eq_ignore_ascii_case("same-origin") && !s.eq_ignore_ascii_case("none"))
}

/// Sent with every answer, errors included.
pub fn headers(content_type: &'static str) -> [(&'static str, &'static str); 5] {
    [
        ("Content-Type", content_type),
        ("X-Content-Type-Options", "nosniff"),
        ("Content-Security-Policy", CSP),
        ("Referrer-Policy", "no-referrer"),
        ("Cache-Control", "no-store"),
    ]
}

/// The whole decision for one request: who may ask, then what they get.
pub fn answer<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, port: u16, r: &Request) -> Reply {
    if cross_site(r.fetch_site) {
        return plain(403, "requests from other sites are refused");
    }
    // Data routes answer only the page's own script. A plain link or image tag on another site cannot set the marker,
    // so even a browser that sends neither Origin nor Sec-Fetch-Site cannot be made to trigger an archive check.
    if r.url.starts_with("/api/") && !r.from_page {
        return plain(403, "data routes answer only this window's own page");
    }
    respond(ctx, port, r.method, r.url, r.host, r.origin)
}

/// Decides the answer once the caller is known to be this window. No sockets here, so every rule is testable.
pub fn respond<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, port: u16, method: &str, url: &str, host: Option<&str>, origin: Option<&str>) -> Reply {
    let me = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.is_some_and(|h| me.iter().any(|m| m.eq_ignore_ascii_case(h))) {
        return plain(403, "this server answers only requests addressed to it on this machine");
    }
    if origin.is_some_and(|o| !me.iter().any(|m| o.eq_ignore_ascii_case(&format!("http://{m}")))) {
        return plain(403, "requests from other sites are refused");
    }
    if method != "GET" {
        return plain(405, "read-only: GET is the only method");
    }
    if url.len() > MAX_URL {
        return plain(414, "address too long");
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    match path {
        "/" => Reply { status: 200, content_type: "text/html; charset=utf-8", body: INDEX.to_owned() },
        "/app.js" => Reply { status: 200, content_type: "text/javascript; charset=utf-8", body: APP_JS.to_owned() },
        "/app.css" => Reply { status: 200, content_type: "text/css; charset=utf-8", body: APP_CSS.to_owned() },
        "/api/stats" => match ctx.store.stats() {
            Ok(s) => data(200, &json!({"ok": true, "posts": s.posts, "threads": s.threads_with_comments, "comments": s.comments})),
            Err(e) => failed(&e.into()),
        },
        "/api/threads" => match threads(ctx) {
            Ok(v) => data(200, &v),
            Err(f) => failed(&f),
        },
        "/api/thread" => {
            let Some(Ref::Thread { post, .. }) = param(query, "id").map(|id| ids::parse_ref(&id)) else {
                return data(404, &json!({"ok": false, "error": "not a thread id"}));
            };
            let args = ThreadArgs {
                reference: post,
                budget: 0,
                focus: None,
                comment: None,
                expand: None,
                limit: 0,
                refresh: false,
                offline: true,
                full: true,
                json: true,
            };
            match run::thread(ctx, &args) {
                Ok(body) => Reply { status: 200, content_type: "application/json; charset=utf-8", body },
                Err(f) => failed(&f),
            }
        }
        "/api/search" => {
            let query = param(query, "q").unwrap_or_default();
            let args = SearchArgs {
                query,
                subs: Vec::new(),
                after: None,
                before: None,
                limit: 30,
                comments: false,
                pace: "timeless".into(),
                offline: true,
                nsfw: false,
                explain: false,
                json: true,
            };
            match run::search(ctx, &args) {
                Ok(body) => Reply { status: 200, content_type: "application/json; charset=utf-8", body },
                Err(f) => failed(&f),
            }
        }
        "/api/sources" => {
            let out = run::sources(ctx);
            ctx.flush_log();
            match out {
                Ok(text) => data(200, &json!({"ok": true, "lines": text.lines().collect::<Vec<_>>()})),
                Err(f) => failed(&f),
            }
        }
        _ => plain(404, "not found"),
    }
}

fn threads<T: Transport, C: Clock>(ctx: &Ctx<T, C>) -> Result<serde_json::Value, Fail> {
    let mut rows = Vec::new();
    for (id, comments) in ctx.store.recent_threads(500)? {
        let Some((p, _, _, fetched)) = ctx.store.post(&id)? else { continue };
        rows.push(json!({
            "id": p.id, "subreddit": render::attr(&p.subreddit), "title": text::strip_invisible(&p.title).0, "created": text::date(p.created),
            "comments": comments, "fetched": fetched.map(text::date), "state": p.state.as_str(),
        }));
    }
    Ok(json!({"ok": true, "threads": rows}))
}

/// Serves until the process is stopped.
pub fn serve<T: Transport, C: Clock>(ctx: &mut Ctx<T, C>, port: u16) -> Result<String, Fail> {
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| Fail::new(6, format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
    println!("rdt window: http://127.0.0.1:{port}/  (read-only; Ctrl+C to stop)");
    for request in server.incoming_requests() {
        // The window may stay open for hours: every request is judged, logged and aged against the time it arrives.
        ctx.now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(ctx.now, |d| d.as_secs() as i64);
        let header = |name: &str| request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_owned());
        let (host, origin, fetch_site) = (header("Host"), header("Origin"), header("Sec-Fetch-Site"));
        let asked = Request {
            method: request.method().as_str(),
            url: request.url(),
            host: host.as_deref(),
            origin: origin.as_deref(),
            fetch_site: fetch_site.as_deref(),
            from_page: header(PAGE_MARKER).is_some(),
        };
        let reply = answer(ctx, port, &asked);
        let mut response = tiny_http::Response::from_string(reply.body).with_status_code(reply.status);
        for (name, value) in headers(reply.content_type) {
            if let Ok(h) = tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()) {
                response.add_header(h);
            }
        }
        let _ = request.respond(response);
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reddit_core::store::Store;
    use reddit_sources::arctic::Arctic;
    use reddit_sources::transport::{FakeClock, Fixtures};

    const POSTS_IDS: &str = include_str!("../../../fixtures/arctic/posts_ids.json");
    const TREE: &str = include_str!("../../../fixtures/arctic/tree.json");
    const POSTS_SEARCH: &str = include_str!("../../../fixtures/arctic/posts_search.json");
    const HOST: Option<&str> = Some("127.0.0.1:8779");

    fn arctic() -> Arctic<Fixtures, FakeClock> {
        Arctic::new(
            Fixtures::default().route("posts/ids", 200, &[], POSTS_IDS).route("comments/tree", 200, &[], TREE).route("posts/search", 200, &[], POSTS_SEARCH),
            FakeClock::default(),
        )
    }

    /// A context with one thread cached and the request log emptied, so any later request shows.
    fn ctx(a: &Arctic<Fixtures, FakeClock>) -> Ctx<'_, Fixtures, FakeClock> {
        let mut c = Ctx { store: Store::memory().unwrap(), arctic: a, now: 1_794_547_000, nonce: "ab12cd" };
        let args = ThreadArgs {
            reference: "1xsjnud".into(),
            budget: 5000,
            focus: None,
            comment: None,
            expand: None,
            limit: 500,
            refresh: false,
            offline: false,
            full: false,
            json: false,
        };
        run::thread(&mut c, &args).unwrap();
        a.log.borrow_mut().clear();
        c
    }

    fn get(c: &mut Ctx<Fixtures, FakeClock>, url: &str) -> Reply {
        respond(c, 8779, "GET", url, HOST, None)
    }

    fn body(r: &Reply) -> serde_json::Value {
        serde_json::from_str(&r.body).unwrap()
    }

    #[test]
    fn answers_only_itself() {
        let a = arctic();
        let mut c = ctx(&a);
        for host in [None, Some("evil.example"), Some("127.0.0.1:9999"), Some("evil.example:8779"), Some("127.0.0.1.evil.example:8779")] {
            assert_eq!(respond(&mut c, 8779, "GET", "/api/stats", host, None).status, 403, "{host:?}");
        }
        assert_eq!(respond(&mut c, 8779, "GET", "/api/stats", Some("LOCALHOST:8779"), None).status, 200);
        assert_eq!(respond(&mut c, 8779, "GET", "/api/stats", HOST, Some("https://evil.example")).status, 403);
        assert_eq!(respond(&mut c, 8779, "GET", "/api/stats", HOST, Some("http://127.0.0.1:8779")).status, 200);
        assert_eq!(respond(&mut c, 8779, "GET", "/api/stats", HOST, Some("null")).status, 403);
    }

    fn asked(url: &'static str) -> Request<'static> {
        Request { method: "GET", url, host: HOST, origin: None, fetch_site: Some("same-origin"), from_page: true }
    }

    #[test]
    fn other_sites_and_bare_links_get_no_data() {
        let a = arctic();
        let mut c = ctx(&a);
        assert_eq!(answer(&mut c, 8779, &asked("/api/stats")).status, 200);
        for site in ["cross-site", "same-site", "CROSS-SITE", "anything-else"] {
            assert_eq!(answer(&mut c, 8779, &Request { fetch_site: Some(site), ..asked("/api/stats") }).status, 403, "{site}");
            assert_eq!(answer(&mut c, 8779, &Request { fetch_site: Some(site), ..asked("/") }).status, 403, "{site}");
        }
        assert!(!cross_site(None) && !cross_site(Some("none")) && !cross_site(Some("Same-Origin")));
        // An image tag or link on another site, from a browser that says nothing about where it came from:
        // no marker, so no data and, above all, no archive check.
        for url in ["/api/sources", "/api/stats", "/api/threads", "/api/thread?id=1xsjnud", "/api/search?q=coil"] {
            assert_eq!(answer(&mut c, 8779, &Request { fetch_site: None, from_page: false, ..asked(url) }).status, 403, "{url}");
        }
        assert!(a.log.borrow().is_empty());
        // The page itself and its files load without the marker (a browser navigation cannot add one).
        for url in ["/", "/app.js", "/app.css"] {
            assert_eq!(answer(&mut c, 8779, &Request { fetch_site: Some("none"), from_page: false, ..asked(url) }).status, 200, "{url}");
        }
        assert_eq!(answer(&mut c, 8779, &Request { host: Some("evil.example"), ..asked("/api/stats") }).status, 403);
    }

    #[test]
    fn every_answer_carries_the_safety_headers_and_the_page_sends_the_marker() {
        let h = headers("text/plain; charset=utf-8");
        let get = |name: &str| h.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
        assert_eq!(get("X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(get("Cache-Control"), Some("no-store"));
        assert_eq!(get("Referrer-Policy"), Some("no-referrer"));
        let csp = get("Content-Security-Policy").unwrap();
        assert!(csp.starts_with("default-src 'none'") && csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("unsafe") && !csp.contains('*') && !csp.contains("http"));
        assert!(APP_JS.contains(&format!("\"{PAGE_MARKER}\"")), "the script must send the marker the server requires");
    }

    #[test]
    fn read_only() {
        let a = arctic();
        let mut c = ctx(&a);
        for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"] {
            assert_eq!(respond(&mut c, 8779, method, "/api/thread?id=1xsjnud", HOST, None).status, 405, "{method}");
        }
        // Long after the thread was fetched, well past the 30-day keep window: browsing must still not purge it.
        c.now += 90 * 86_400;
        for url in [
            "/",
            "/app.js",
            "/api/stats",
            "/api/threads",
            "/api/thread?id=1xsjnud",
            "/api/search?q=coil+whine",
            "/api/search?q=zebra",
            "/api/thread?id=nothere1",
            "/nope",
        ] {
            get(&mut c, url);
        }
        assert!(a.log.borrow().is_empty(), "browsing the cache asks the archive nothing");
        let stats = c.store.stats().unwrap();
        assert_eq!((stats.threads_with_comments, stats.comments), (1, 4), "and changes nothing, however old the cache is");
        assert_eq!(body(&get(&mut c, "/api/thread?id=1xsjnud"))["ok"], true);
    }

    #[test]
    fn static_files_and_unknown_paths() {
        let a = arctic();
        let mut c = ctx(&a);
        let index = get(&mut c, "/");
        assert_eq!((index.status, index.content_type), (200, "text/html; charset=utf-8"));
        assert!(index.body.contains("<script src=\"/app.js\"></script>") && !index.body.contains("<script>"), "no inline script, so the policy can forbid it");
        assert!(!index.body.contains("style="), "no inline style either");
        assert_eq!(get(&mut c, "/app.js").content_type, "text/javascript; charset=utf-8");
        assert_eq!(get(&mut c, "/app.css").content_type, "text/css; charset=utf-8");
        assert!(
            !APP_JS.contains("innerHTML") && !APP_JS.contains("insertAdjacentHTML") && !APP_JS.contains("document.write"),
            "thread text is never parsed as markup"
        );
        for url in ["/app.js/../../Cargo.toml", "/../secret", "/api", "/api/thread/", "//", "/index.html"] {
            assert_eq!(get(&mut c, url).status, 404, "{url}");
        }
    }

    #[test]
    fn lists_and_shows_cached_threads() {
        let a = arctic();
        let mut c = ctx(&a);
        assert_eq!(body(&get(&mut c, "/api/stats"))["threads"], 1);
        let list = body(&get(&mut c, "/api/threads"));
        assert_eq!(
            (list["threads"][0]["id"].as_str(), list["threads"][0]["comments"].as_i64(), list["threads"][0]["subreddit"].as_str()),
            (Some("1xsjnud"), Some(4), Some("AMDHelp"))
        );
        let t = get(&mut c, "/api/thread?id=t3_1xsjnud");
        assert_eq!(t.content_type, "application/json; charset=utf-8");
        let t = body(&t);
        assert_eq!((t["ok"].as_bool(), t["comments"].as_array().map(Vec::len), t["comments_loaded"].as_i64()), (Some(true), Some(4), Some(4)));
        let missing = get(&mut c, "/api/thread?id=zzzzzzz");
        assert_eq!((missing.status, body(&missing)["ok"].as_bool()), (404, Some(false)));
        for bad in ["/api/thread", "/api/thread?id=", "/api/thread?id=..%2F..%2Fx", "/api/thread?id=https%3A%2F%2Fwww.reddit.com%2Fr%2Fa%2F"] {
            assert_eq!(get(&mut c, bad).status, 404, "{bad}");
        }
    }

    #[test]
    fn searches_the_cache_only() {
        let a = arctic();
        let mut c = ctx(&a);
        let hit = body(&get(&mut c, "/api/search?q=coil+whine%20sapphire"));
        assert_eq!(hit["results"][0]["id"], "t3_1xsjnud");
        assert_eq!(hit["query"], "coil whine sapphire");
        let none = get(&mut c, "/api/search?q=zebra");
        assert_eq!((none.status, body(&none)["ok"].as_bool()), (404, Some(false)));
        assert_eq!(get(&mut c, "/api/search").status, 404);
        assert_eq!(get(&mut c, "/api/search?q=%ZZ%").status, 404, "a broken escape is text, not a crash");
        assert_eq!(get(&mut c, &format!("/api/search?q={}", "a".repeat(2000))).status, 414);
        assert!(a.log.borrow().is_empty());
    }

    #[test]
    fn the_archive_is_asked_only_by_the_explicit_check() {
        let a = arctic();
        let mut c = ctx(&a);
        let s = body(&get(&mut c, "/api/sources"));
        assert!(s["lines"][1].as_str().unwrap().starts_with("arctic   reachable"), "{s}");
        assert_eq!((a.log.borrow().len(), c.store.requests_since(0).unwrap()), (0, 1), "exactly one request, and it is written to the request log");
    }

    #[test]
    fn query_decoding() {
        assert_eq!(param("a=1&q=coil+whine%20%22x%22&z", "q").as_deref(), Some("coil whine \"x\""));
        assert_eq!(param("q=%E2%9C%93", "q").as_deref(), Some("\u{2713}"));
        assert_eq!(param("q=%", "q").as_deref(), Some("%"));
        assert_eq!(param("q=%4", "q").as_deref(), Some("%4"));
        assert_eq!(param("q=%FF", "q").as_deref(), Some("\u{FFFD}"));
        assert_eq!(param("x=1", "q"), None);
    }
}
