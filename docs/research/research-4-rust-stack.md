# Research 4 — Rust stack for a Reddit OAuth CLI on `x86_64-pc-windows-gnu`

Researched 2026-11-12, web only (crates.io API, docs.rs, GitHub, sqlite.org, Microsoft Learn). Nothing was compiled; every "builds on GNU" statement is from docs/build scripts, not from a test build. Version numbers and dates come from the crates.io JSON API (`https://crates.io/api/v1/crates/<name>`) read on 2026-11-12 unless another URL is given.

Legend: **[V]** verified on a primary source today, **[I]** inference from verified facts, **[M]** from memory / secondary source only (also listed in "Could not verify").

---

## 0. Target baseline

- `x86_64-pc-windows-gnu` is still **Tier 1 with host tools** ("64-bit MinGW, Windows 10+"); `x86_64-pc-windows-gnullvm` is Tier 2 with host tools; `i686-pc-windows-gnu` is Tier 2 without host tools. [V] https://doc.rust-lang.org/nightly/rustc/platform-support.html (read 2026-11-12)
- Because we want bundled SQLite, a C compiler (WinLibs GCC) is needed regardless. So the real line is not "zero C" but "C that the `cc` crate can build with plain GCC, with no cmake / NASM / perl / bindgen".

---

## 1. HTTP client

### reqwest
- Current **0.13.5** (2026-09-08), MIT OR Apache-2.0, MSRV 1.85. [V] crates.io API; https://docs.rs/crate/reqwest/0.13.5/source/Cargo.toml.orig
- Default features: `default-tls, charset, http2, system-proxy`; `default-tls = ["rustls"]`; `rustls = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]`. [V] same
- Changelog 0.13.0: "rustls is now the default TLS backend, instead of native-tls"; "rustls crypto provider defaults to aws-lc instead of ring"; `rustls-tls` renamed `rustls`; root features removed, `rustls-platform-verifier` used by default; `query` and `form` are now opt-in features. [V] https://github.com/seanmonstar/reqwest/blob/master/CHANGELOG.md
- Selecting something else:
  - `rustls-no-provider` = rustls without a crypto provider; you then add `rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }` yourself and call `rustls::crypto::ring::default_provider().install_default()` at start-up. [V for the feature; I for the exact recipe]
  - `native-tls` = SChannel on Windows (native-tls 0.2.18 depends on `schannel` on `target_os = "windows"`, OpenSSL only on non-Windows/non-Apple). No C build at all on Windows. [V] https://docs.rs/crate/native-tls/latest/source/Cargo.toml
- Other flags: `blocking`, `json`, `gzip`, `brotli`, `zstd`, `deflate`, `cookies`, `socks`, `stream`, `multipart`, `hickory-dns`. [V] https://docs.rs/reqwest/latest/reqwest/
- Cost: `blocking` is a wrapper over the async client, so tokio + hyper + h2 + tower-http are always in the tree. Heavy for this job.

### aws-lc-rs / aws-lc-sys on windows-gnu
- aws-lc-sys **0.47.0** (2026-10-28). Features include `prebuilt-nasm`, `bindgen`, `fips`. [V]
- Official requirements: `x86_64-pc-windows-gnu` non-FIPS needs "C/C++ compiler and NASM"; **CMake is only required for FIPS**; non-FIPS uses universal pre-generated bindings (no bindgen). FIPS is not supported on windows-gnu. [V] https://aws.github.io/aws-lc-rs/requirements/windows.html and https://aws.github.io/aws-lc-rs/platform_support.html
- Prebuilt NASM objects are used only when: no NASM on PATH, not FIPS, target is x86_64 msvc or gnu, and `AWS_LC_SYS_PREBUILT_NASM=1` or the `prebuilt-nasm` feature is on. `AWS_LC_SYS_NO_ASM` exists but is for debug only. [V] same page
- rustls **0.23.46** `aws_lc_rs` feature = `["dep:aws-lc-rs", "webpki/aws-lc-rs", "aws-lc-rs/aws-lc-sys", "aws-lc-rs/prebuilt-nasm"]`, so going through rustls (and therefore reqwest 0.13 default) turns `prebuilt-nasm` on. [V] crates.io API for rustls
- Net: reqwest-default on windows-gnu **should** build with GCC only, no cmake/NASM [I]. But it compiles a large C library with GCC, and there are third-party failure reports on this target (a Feb 2026 R win-builder log failing in aws-lc-sys 0.35.0 for x86_64-pc-windows-gnu; a Nix cross-compile report; a note that MinGW does not honour AWS-LC's `#pragma comment(lib, "bcrypt.lib")`). None confirmed as open upstream issues. [M] via search results, e.g. https://win-builder.r-project.org/incoming_pretest/deltalakeR_0.2.0_20260206_014118/Windows/00install.out
- rustls 0.24.0-dev.3 (2026-11-12, pre-release) has no `ring`/`aws_lc_rs` features at all (providers move out of the main crate) — expect feature names to change again when 0.24 ships. [V] crates.io API

### ring on windows-gnu
- Current **0.17.14** (2025-03-11) — no release for 20 months. Licence "Apache-2.0 AND ISC". [V]
- BUILDING.md: packaged crates.io releases need no perl; "for Windows x86 and x86-64, the crate includes precompiled object files for the assembly modules, so no macro assembler is needed"; a C toolchain (not C++) is required. It says MSVC targets are "supported best" and does not discuss GNU. [V] https://github.com/briansmith/ring/blob/main/BUILDING.md
- build.rs (0.17.14): the Windows/x86_64 asm target is chosen on arch + OS only; target env is not consulted; packaged builds link the pregenerated NASM `.o` files; C flags are chosen by `compiler.is_like_msvc()`, with GCC getting `-fvisibility=hidden -std=c1x -Wall ...`. So GCC + prebuilt objects is the designed path for windows-gnu. [V] https://docs.rs/crate/ring/0.17.14/source/build.rs
- One GNU-specific report surfaced (ring discussion #2775, wrong C flags on `--target x86_64-pc-windows-gnu`), seen only through a mirror; status unknown. [M]

### ureq
- Current **3.5.2** (released today, 2026-11-12), MIT OR Apache-2.0, MSRV 1.85. [V]
- `default = ["rustls", "gzip"]`; `rustls = ["rustls-no-provider", "_ring", "rustls-webpki-roots"]` → **rustls + ring + bundled Mozilla roots** by default. Docs warn it "does not guarantee to default to ring indefinitely" and that a process-default provider you install is respected. [V] https://docs.rs/crate/ureq/3.5.2/source/Cargo.toml.orig, https://docs.rs/ureq/latest/ureq/
- Other features: `native-tls` (never default; must be selected on the agent with `TlsConfig::builder().provider(TlsProvider::NativeTls)`), `platform-verifier` (`RootCerts::PlatformVerifier`, works with both providers), `json` (`send_json` / `read_json`), `brotli`, `charset`, `cookies`, `socks-proxy`, `multipart`, `win-system-proxy`. [V]
- Non-optional deps are only `base64`, `ureq-proto`, `log`, `utf8-zero`, `percent-encoding`. Blocking I/O, no tokio, `forbid(unsafe)`. [V]
- Timeouts: `timeout_global`, `timeout_per_call`, `timeout_connect`, `timeout_recv_response`, `timeout_recv_body`, ... all default `None` — **set them**. Proxy: HTTP CONNECT always available, env `HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY` read by the default agent. [V]
- ureq 3 returns `Err(StatusCode)` for 4xx/5xx by default; set `http_status_as_error(false)` so 429/401 bodies and headers can be read. [M — from ureq 3 docs as I remember them; check `Config`]

### Others
| Crate | Version / date | Notes |
|---|---|---|
| attohttpc | 0.32.0 (2026-11-05), MPL-2.0 | Alive. Default `tls-native` + `compress`; rustls variants with explicit `-ring` flavours. Viable alternative; MPL licence. [V] |
| minreq | 3.0.0 (2026-06-15), ISC | Alive. `https-rustls`, `https-native-tls`, `json-using-serde`, `proxy`. Very small, fewer knobs (no gzip feature listed). [V] |
| isahc | 2.0.2 (2026-10-21), MIT | Alive again, but libcurl-based (C + possibly cmake) — reject for this target. [V version; I on build] |
| hyper directly | — | Overkill: you would rebuild pooling, TLS glue, redirects, decompression. |

### Recommendation
**ureq 3.5 with its default rustls+ring backend, plus `json`; keep `native-tls` as a documented fallback.** Reasons: blocking by design, 5 mandatory deps, no tokio, ring needs only GCC (prebuilt asm objects), avoids aws-lc entirely. If ring ever fails on the WinLibs toolchain, switch features to `default-features = false, features = ["native-tls", "gzip", "json", "platform-verifier"]` and set `TlsProvider::NativeTls` — SChannel, zero C. Guard against silent provider drift with `cargo tree -i aws-lc-sys` in CI/checks.

---

## 2. Is async worth it at 60–100 req/min?

No.
- The Tokio tutorial's own "When not to use Tokio" section says Tokio helps when many things happen at once, and that for single requests a blocking client is simpler. [V] https://tokio.rs/tokio/tutorial
- ureq's docs: it "uses blocking I/O instead of async I/O, because that keeps the API simple and keeps dependencies to a minimum". [V] https://docs.rs/ureq/latest/ureq/
- Our throughput ceiling is the server quota (about 1–1.7 requests/second), not client concurrency. One request at ~200–500 ms round trip already saturates the quota from a single thread.
- `morechildren` must be sequential anyway: the open-sourced Reddit code's docstring says "you may only make one request at a time to this API endpoint" and takes a per-user lock with `timeout=0`, aborting with **429** on contention. [V, old code] https://raw.githubusercontent.com/reddit-archive/reddit/master/r2/r2/controllers/api.py
- A single global rate-limit state is trivially correct on one thread; with async tasks it needs shared state and care.
- If parallelism is ever wanted (e.g. fetch 3 threads' first pages at once): 2–3 `std::thread::scope` workers pulling from a channel and sharing one `Mutex<RateState>` is enough. SQLite writes stay on one thread.
- Cost of async: tokio + hyper + h2 + tower in the build, `#[tokio::main]`, async-coloured traits, longer compile, bigger binary — for no throughput gain.

---

## 3. Existing Reddit crates

| Crate | Latest | Licence | State | Verdict |
|---|---|---|---|---|
| **roux** | 2.3.0 (2026-09-08; before that 2.2.15 2025-07-10, 2.2.14 2025-01-08) | MIT | README: "not in active development but is still being maintained". 6 open issues. Async by default (tokio/reqwest), `blocking` feature via maybe-async. Default TLS feature `native-tls`, optional `rustls`. OAuth: **password grant** (`username/password/login`) and app-only `client_credentials`; no refresh-token / installed-app code flow seen. Docs list modules for subreddit, user, me, inbox, saved, submission, comment, moderator, and a `MaybeReplies` enum ("Replies can be more comments or an empty string"). No morechildren, no rate-limit handling visible in the docs. README notes unauthenticated JSON endpoints now likely 403. [V] crates.io API, https://github.com/halcyonnouveau/roux, https://docs.rs/roux/latest/roux/ | **Read for models, don't depend.** Pulls reqwest+tokio, lacks our auth flow, morechildren and rate limiting. |
| roux-stream | 0.2.0 (2023-05-18) | MIT | Stale add-on for roux polling streams. [V] | Skip. |
| rraw | 1.2.1 (2023-07-07) | MIT | Async, stale 3+ years. [V] | Skip. |
| orca | 0.7.0 (2018-12-16) | MPL-2.0 | Dead. [V] | Skip. |
| snoo | 0.0.4 (2020-05-17) | MIT | Dead. [V] | Skip. |
| rawr | 0.1.1 (2016-08-03) | MIT | Dead. `new-rawr` yanked. [V] | Skip. |
| reddit-rs | 0.1.1 (2022-01-17) | — | Dead, no repo link. [V] | Skip. |
| others seen | mr_splashy_pants 0.1.39 (2021), tranquility 0.0.8 (2020), rsreddit 0.1.3 (2020), redbot (2019), reddit-api 0.1.0 (2021) | — | All dead. [V] crates.io search | Skip. |
| new 2024–2026 | `reddit` 0.6.0 (2026-10-16, offline subreddit viewer, github.com/reddit-rs/reddit), `reddit-cli` 0.2.3 (2026-10-08), `redtab` 0.1.1 (2026-11-09, TUI reader), `shreddit` 1.1.12 (2026-05-25, history deleter), plus wrappers for third-party paid "Reddit data gateways" (crawlora-reddit, sylvia-api, reddit-insights) | varies | Tiny download counts (10–200); applications, not libraries. [V] https://crates.io/api/v1/crates?q=reddit+api | Not dependencies. `reddit` and `reddit-cli` may be worth a skim for prior art (not reviewed). |
| **redlib** (successor of libreddit; libreddit crate last 0.30.1, 2023-03-17) | README references v0.40.0 | **AGPL-3.0** | Active (1,411 commits, 186 open issues). | **Read only, and mind the licence: do not copy code.** |

redlib notes:
- Auth: README and `src/oauth.rs` describe **OAuth token spoofing** — impersonating the official Android app's client id, generated device ids, mimicked app headers/user agent, token refresh on a timer and forced "rollover" to a fresh token when the remaining quota drops below 10. This is rate-limit evasion and violates the API rule against spoofing; **we will not copy it**. [V] https://github.com/redlib-org/redlib, https://raw.githubusercontent.com/redlib-org/redlib/main/src/oauth.rs, .../src/client.rs
- It now also builds BoringSSL from source (`boring-sys2`) — another reason it is not a dependency candidate for this target. [V] README
- Instructive bits: it parses Reddit JSON with **`serde_json::Value` indexing and defaults** rather than typed structs (`data["upvote_ratio"].as_f64().unwrap_or(1.0)`, `data["created_utc"].as_f64()`); it parses `x-ratelimit-remaining` as **`f32` then rounds**; it treats top-level `{"error": <int>, "reason": "quarantined"|"gated"|"private"|"banned", "message": ...}` and `data.is_suspended` as distinct error cases; media resolution order is `preview.reddit_video_preview` → `secure_media` → `crosspost_parent_list`; gallery items are looked up by `media_id` in `media_metadata`; flair richtext parts use `e`/`t`/`u` keys. [V] `src/utils.rs`, `src/client.rs`

---

## 4. Rate limiting / backoff

Facts:
- Headers: `X-Ratelimit-Used` ("approximate number of requests used in this period"), `X-Ratelimit-Remaining`, `X-Ratelimit-Reset` ("approximate number of seconds to end of period"). The archived wiki says OAuth clients get 60 requests/minute; User-Agent must be `<platform>:<app ID>:<version> (by /u/<username>)`; spoofing prohibited. [V, archived] https://github.com/reddit-archive/reddit/wiki/API
- Current figure of 100 queries/minute per OAuth client id averaged over a 10-minute window: [M] — the Reddit Data API wiki (support.reddithelp.com) returned 403 to the fetcher.
- Float values: PRAW's prawcore parses `int(float(headers["x-ratelimit-remaining"]))` while taking used/reset as ints; redlib parses remaining as `f32`. Parse all three as `f64` and floor. [V] https://raw.githubusercontent.com/praw-dev/prawcore/main/prawcore/rate_limit.py, redlib `src/client.rs`
- prawcore algorithm worth copying: if `remaining <= 0` sleep `max(1 s, reset)`; otherwise spread remaining requests over the remaining window, capped at 10 s. [V]

Crates:
| Crate | Version | Fit |
|---|---|---|
| governor | 0.10.4 (2025-12-16), MIT | GCRA limiter; client-side estimate only, does not know the server's counters. Unnecessary. [V version] |
| leaky-bucket | 1.1.2 (2024-05-22) | Async/tokio oriented. No. [V version] |
| backoff | 0.4.0 (2021-12-14) | Unmaintained for 5 years. No. [V] |
| backon | 1.6.0 (2025-10-18), Apache-2.0 | Has `BlockingRetryable` with `std::thread::sleep`; **default features include `tokio-sleep`** → use `default-features = false, features = ["std", "std-blocking-sleep"]` if used at all. [V] https://docs.rs/backon/latest/backon/ |
| reqwest-middleware / reqwest-retry | reqwest-retry 0.9.1 (2026-02-05) | reqwest/async only. Irrelevant with ureq. [V version] |

**Recommendation: hand-roll (~60–80 lines).** The server tells us the truth in headers; a generic limiter only guesses.
```rust
struct RateState { remaining: f64, reset_at: Instant, min_gap: Duration }

fn before_request(s: &RateState) {
    let now = Instant::now();
    if s.remaining < 1.0 && s.reset_at > now { sleep(s.reset_at - now + Duration::from_millis(250)); }
    // else: spread: sleep(min((reset_at-now)/remaining, 10 s)).max(min_gap)
}
fn after_response(s: &mut RateState, h: &Headers) {
    let f = |n| h.get(n).and_then(|v| v.trim().parse::<f64>().ok());
    if let (Some(rem), Some(reset)) = (f("x-ratelimit-remaining"), f("x-ratelimit-reset")) {
        s.remaining = rem.floor(); s.reset_at = Instant::now() + Duration::from_secs_f64(reset);
    } else { s.remaining -= 1.0; }           // headers absent (cached/CDN responses): count locally
}
// 429: honour Retry-After (integer seconds; fall back to x-ratelimit-reset, then 60 s), max 3 tries.
// 5xx / timeouts: exponential backoff 1,2,4,8 s + jitter, max 4 tries. 401: refresh token once, retry once.
// Never retry 403/404.
```

---

## 5. serde modelling of Reddit JSON quirks

Sources: archived official JSON wiki https://github.com/reddit-archive/reddit/wiki/JSON [V]; PRAW source/docs [V]; redlib source [V]; a 2015 gist with real `more` objects https://gist.github.com/Dakta/4d5835872b3be47c8f19 [V]. Items marked [M] are well known in practice but I could not get a primary page for them today.

| Quirk | Evidence |
|---|---|
| `kind` + `data` envelope; kinds `Listing`, `more`, `t1` comment, `t2` account, `t3` link, `t4` message, `t5` subreddit, `t6` award | Wiki lists `Listing`, `more`, `t1`, `t2`, example `t4_...`. [V]; full t1–t6 table [M] |
| `edited`: `false` or epoch seconds; "some old comments may show `true`" | Wiki, comment `edited` is type "special". [V] |
| `created` / `created_utc`: floats like `1331042771.0`, "never have a non-zero fraction" | Wiki. [V] |
| `replies`: Listing object, or `""` when there are none | Wiki documents the empty string for messages; roux models it as `MaybeReplies` ("more comments or an empty string"). [V] |
| `likes`: `true` / `false` / `null` (null also when not logged in) | Wiki. [V] |
| `more`: `children` (list of id36 strings), `count`, `parent_id`, `id`, `name` | Wiki documents `children`, `id`, `name`; gist shows `count`/`parent_id`. [V] |
| "Continue this thread": `count: 0`, `id: "_"`, `name: "t1__"`, `children: []` | Gist example. PRAW: `if self.count == 0: # Handle "continue this thread"` → refetches `/comments/{link}/_/{parent_comment_id}` instead of morechildren. [V] https://raw.githubusercontent.com/praw-dev/praw/main/praw/models/reddit/more.py |
| morechildren params | PRAW posts `children` (comma-joined), `link_id` (fullname), `sort`. Old Reddit code: validators `link_id`, `sort`, `children`, `id`; one request at a time. [V]. `api_type=json`, `limit_children`, `depth`, and the 100-ids-per-call cap: [M] |
| `distinguished`: `null`, `"moderator"`, `"admin"`, `"special"` | Wiki. [V] |
| `stickied` (bool), `score_hidden` (comment, bool) | Wiki. [V] Links use `hide_score` (redlib reads it). [V] |
| `author`: account name; `null` for promoted links; `"[deleted]"` for deleted accounts | Wiki for null. [V]; `"[deleted]"` and missing `author_fullname` on deleted/suspended authors [M] |
| `body` / `selftext` `"[deleted]"` (user) vs `"[removed]"` (mod/admin/filter) | [M] |
| `removed_by_category` on links: null or strings such as `moderator`, `deleted`, `author`, `reddit`, `automod_filtered`, `anti_evil_ops`, `copyright_takedown`, `content_takedown`, `community_ops` | redlib checks `== "moderator"`. [V for that one value]; rest [M] — treat as open string set |
| HTML entities: without `raw_json=1`, `&`, `<`, `>` come back as `&amp;` `&lt;` `&gt;` in all string fields; `body_html`/`selftext_html` are escaped HTML ("you must unescape"), `selftext_html` nullable | Wiki for the `_html` fields. [V]; `raw_json=1` semantics [M] (reddit.com/dev/api blocked); redlib does append `raw_json=1`. [V] |
| `media` / `secure_media` / `media_embed`: null, `{}` or provider-specific objects | Wiki: "Object" for streaming video. [V]; shape variability [M] |
| `preview.images[0].source{url,width,height}`, `preview.reddit_video_preview` | redlib. [V] |
| `gallery_data.items[].media_id` (+ `id`, optional `caption`) and `media_metadata{media_id: {status, e, m, s{u|gif|mp4,x,y}, p[]}}`; either may be null/missing; entries with `status != "valid"` lack `s` | redlib looks up `media_id`, checks `image/gif`. [V]; field list [M]. `media_metadata` also appears on comments/selftext for inline images and giphy. [M] |
| `crosspost_parent_list`: array of full link objects; `crosspost_parent` fullname | redlib reads it as media fallback. [V] |
| Numeric fields sometimes null or absent: `num_reports`, `view_count`, `upvote_ratio`, `likes`, `approved_at_utc`, `banned_at_utc`, `num_crossposts`, `subreddit_subscribers`, `score` when hidden | redlib defaults `upvote_ratio` with `unwrap_or(1.0)`. [V]; rest [M] |
| Errors arrive as 200/403/404 with `{"error": 403, "reason": "private", "message": ...}`; `/api/*` errors as `{"json": {"errors": [[code, msg, field]]}}` | redlib for the first shape. [V]; second [M] |
| PRAW: attributes "are subject to change on Reddit's end" and the tables "may not be complete" | [V] https://praw.readthedocs.io/en/stable/code_overview/models/submission.html |

### Techniques (sketches)

Principle: type only the ~20 fields we use, default everything, keep the raw object alongside so nothing is lost and schema drift never breaks ingestion.

```rust
// Envelope: adjacently tagged on kind/data, with a catch-all.
#[derive(Deserialize)]
#[serde(tag = "kind", content = "data")]
enum Thing {
    Listing(Listing),
    #[serde(rename = "t1")] Comment(Box<Comment>),
    #[serde(rename = "t3")] Link(Box<Link>),
    #[serde(rename = "more")] More(More),
    #[serde(other)] Unknown,            // t2/t4/t5/t6, future kinds (unit variant; data is skipped)
}
#[derive(Deserialize)]
struct Listing { #[serde(default)] after: Option<String>, #[serde(default)] before: Option<String>,
                 #[serde(default)] children: Vec<Thing> }

#[derive(Deserialize)]
struct More { #[serde(default)] count: i64, #[serde(default)] parent_id: String,
              #[serde(default)] id: String, #[serde(default)] children: Vec<String> }
impl More { fn is_continue_thread(&self) -> bool { self.count == 0 || self.id == "_" } }
```
```rust
// edited: false | true | 1700000000.0
fn de_edited<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    #[derive(Deserialize)] #[serde(untagged)] enum E { B(bool), F(f64) }
    Ok(match Option::<E>::deserialize(d)? { Some(E::F(t)) => Some(t), Some(E::B(true)) => Some(0.0), _ => None })
}
// replies: "" | Listing-thing | null | missing
fn de_replies<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Thing>, D::Error> {
    #[derive(Deserialize)] #[serde(untagged)] enum R { T(Thing), S(String) }
    Ok(match Option::<R>::deserialize(d)? { Some(R::T(Thing::Listing(l))) => l.children, _ => Vec::new() })
}
// "null or wrong type means default" for flaky numerics
fn de_lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where D: Deserializer<'de>, T: DeserializeOwned + Default {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}
#[derive(Deserialize)]
struct Comment {
    id: String, #[serde(default)] name: String,
    #[serde(default)] author: Option<String>,            // "[deleted]" stays a string; map to None later
    #[serde(default)] author_fullname: Option<String>,   // absent for deleted accounts
    #[serde(default)] body: String,
    #[serde(default)] parent_id: String, #[serde(default)] link_id: String,
    #[serde(default)] created_utc: f64,
    #[serde(default, deserialize_with = "de_edited")] edited: Option<f64>,
    #[serde(default, deserialize_with = "de_lenient")] score: i64,
    #[serde(default)] score_hidden: bool, #[serde(default)] stickied: bool,
    #[serde(default)] distinguished: Option<String>,
    #[serde(default)] likes: Option<bool>,
    #[serde(default, deserialize_with = "de_replies")] replies: Vec<Thing>,
    #[serde(flatten)] extra: serde_json::Map<String, serde_json::Value>,  // media_metadata, awards, flair...
}
```
Notes:
- `#[serde(default)]` handles a **missing** key but not an explicit `null` for non-`Option` types; use `Option<T>` or `de_lenient` for fields that can be `null`.
- Untagged enums give poor error messages and buffer the value; keep them inside small `deserialize_with` helpers as above rather than on big types.
- `#[serde(flatten)]` + adjacently tagged enums both buffer through serde's `Content`; fine at our volumes. If `flatten` proves awkward, parse twice: `Value` first (stored raw in SQLite), then `from_value` for the typed view. Storing the raw JSON per thing is the best insurance.
- `media`, `secure_media`, `preview`, `gallery_data`, `media_metadata`, `crosspost_parent_list`, `poll_data`: keep as `Option<serde_json::Value>` and extract with small accessor functions (redlib style), never as rigid structs.
- `created_utc` as `f64` → store `as i64`.
- Always send `raw_json=1`; still run an entity decode on `title`/`body` defensively only if a literal `&amp;`/`&lt;`/`&gt;` check says the flag was ignored.
- Reddit nests replies deeply; serde_json's default recursion limit is 128 levels — each comment level costs several levels (thing → data → replies → data → children). Reddit normally truncates depth (~10) with `more`, so this is fine, but do not disable the limit; handle the error. [I]

---

## 6. Local storage and search

### rusqlite + bundled SQLite
- rusqlite **0.42.0** (2026-11-11, yesterday), MIT; libsqlite3-sys **0.40.0** (same day). README: bundled source is **SQLite 3.54.1**; minimum supported system SQLite 3.45.3. [V] crates.io API; https://github.com/rusqlite/rusqlite
- Being one day old, consider pinning **0.41.1** (2026-10-28) for the first weeks, or accept 0.42 and watch the tracker. [I]
- `bundled` = `libsqlite3-sys/bundled` → compiles `sqlite3.c` with the **`cc` crate** (no cmake, no bindgen: `bundled_bindings`). [V] features map
- Bundled compile flags (build.rs): `SQLITE_ENABLE_FTS5`, `FTS3`, `FTS3_PARENTHESIS`, `JSON1`, `RTREE`, `STAT4`, `DBSTAT_VTAB`, `COLUMN_METADATA`, `LOAD_EXTENSION`, `API_ARMOR`, `USE_URI`, `THREADSAFE=1`, `DEFAULT_FOREIGN_KEYS=1`, `SOUNDEX`. **FTS5 is on by default in bundled builds**; there is no `fts5` cargo feature. Also `-D_POSIX_THREAD_SAFE_FUNCTIONS` with the comment "cross compile with MinGW"; `HAVE_LOCALTIME_R` only for non-Windows. [V] https://github.com/rusqlite/rusqlite/blob/master/libsqlite3-sys/build.rs
- Note `SQLITE_DEFAULT_FOREIGN_KEYS=1` — foreign keys are enforced by default in bundled builds, unlike stock SQLite.
- Avoid `bundled-sqlcipher*` (OpenSSL), `buildtime_bindgen`, `session`/`preupdate_hook` (need bindgen → libclang). [V]
- windows-gnu specific open issues: none searched for specifically (see "Could not verify"). The MinGW define in build.rs shows the target is considered.

### FTS5 features we would use — all [V] https://www.sqlite.org/fts5.html
- Ranking: `bm25(ft, w0, w1, ...)` (k1 = 1.2, b = 0.75; **lower is better**, so `ORDER BY rank` ascending). Persist weights with `INSERT INTO ft(ft, rank) VALUES('rank', 'bm25(10.0, 1.0)')`.
- `snippet(ft, col, start, end, ellipsis, max_tokens≤64)`, `highlight(ft, col, start, end)`.
- External content: `fts5(title, body, content='comments', content_rowid='rowid')` + the three standard triggers; `INSERT INTO ft(ft) VALUES('rebuild')` to resync. Text is stored once.
- Contentless-delete: `content='', contentless_delete=1` needs SQLite ≥ 3.43.0 (bundled 3.54.1 satisfies). No `snippet()` from the index itself, so external-content is the better fit for us.
- Tokenizers: `unicode61` (`remove_diacritics 0|1|2`, `tokenchars`, `separators`), `porter` wrapper (`tokenize='porter unicode61 remove_diacritics 2'`), `trigram` (`case_sensitive`, `remove_diacritics`; accelerates LIKE/GLOB; queries under 3 chars match nothing). Trigram's minimum version (3.34) is [M] but far below the bundled version.
- `prefix='2 3'` for fast prefix queries; `detail=column|none` to shrink the index (loses phrase/NEAR); `'optimize'` and incremental `'merge'` maintenance commands.
- Escape user queries: wrap each term in double quotes (doubling inner quotes) before `MATCH`, or FTS5 syntax characters (`-`, `:`, `*`, `^`, `NEAR`) will raise errors. [M]

### tantivy
- **0.27.0** (2026-10-17), MIT, **MSRV 1.88**. Default features `mmap, stopwords, lz4-compression, columnar-zstd-compression, stemmer`. [V] crates.io API; https://docs.rs/crate/tantivy/0.27.0/source/Cargo.toml
- `lz4-compression` uses `lz4_flex` (pure Rust). `zstd-compression` (C `zstd` crate) is **not** default. `columnar-zstd-compression` → `tantivy-columnar/zstd-compression` → `sstable/zstd-compression`, which most likely pulls the C zstd library through `cc` [I — did not open tantivy-sstable's manifest]. Disable defaults to stay pure Rust.
- ~40 mandatory direct deps (rayon, regex, time, uuid, typetag, datasketches, bon, ...) plus `winapi` on Windows. README: supports Linux, macOS, Windows; BM25 "same as Lucene"; startup < 10 ms. No statement about windows-gnu. [V] https://github.com/quickwit-oss/tantivy
- Costs: separate index directory with segments, a writer thread pool and commit/merge lifecycle, a second source of truth to keep in sync with the row store, much larger compile and binary. Binary-size figure not found. [M]

### redb / fjall / sled + hand-rolled BM25
- redb **4.5.0** (2026-11-05), fjall **3.1.13** (2026-10-25), sled stable **0.34.7** with 1.0 alphas stalled since 2024-10-11 (avoid sled). [V]
- Pure Rust, but we would write tokenizer, postings, BM25, snippets, phrase queries, deletes and compaction ourselves, and lose ad-hoc SQL (which the LLM-facing JSON queries will want: by subreddit, author, date range, score).

### Recommendation (10^4–10^6 comments)
**rusqlite `bundled` + one FTS5 external-content table.** One file, transactions, SQL filters joined with full-text rank, `snippet()` for agent-friendly excerpts, builds with GCC alone. A million comments (~0.5–1 GB of text) is comfortably inside FTS5's range. Suggested DDL: `comments(rowid, id, link_id, parent_id, author, created_utc, score, body, raw_json)`; `comments_fts(title UNINDEXED?, body, content='comments', content_rowid='rowid', tokenize='porter unicode61 remove_diacritics 2', prefix='2 3')`; WAL mode; `PRAGMA synchronous=NORMAL`; batch inserts in one transaction per thread fetched; `'optimize'` after big imports. Add a trigram table only if substring search becomes a real need (roughly triples index size).

---

## 7. Text cleaning

- pulldown-cmark **0.13.6** (2026-10-30), MIT; default features `getopts, html` (turn off: `default-features = false`). Options include TABLES, STRIKETHROUGH, FOOTNOTES, TASKLISTS, GFM, SUPERSCRIPT, SUBSCRIPT, WIKILINKS, MATH; **no spoiler option**. [V] https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html
- comrak stable **0.59.0**, **1.0.0-rc.1** published today; BSD-2-Clause; default features pull `cli` (clap), `syntect-onig` (**Oniguruma = C library**) and `bon` → must use `default-features = false`. Extensions: `superscript` is `^text^` and `spoiler` is `||text||` (Discord style) — **neither matches Reddit** (`^word`, `^(group)`, `>!text!<`). [V] crates.io API; https://docs.rs/comrak/latest/comrak/options/struct.Extension.html
- markdown (markdown-rs) **1.0.0** (2025-04-23), MIT, pure Rust, CommonMark/GFM/MDX; no Reddit extensions. [V]
- Entity decoding: html-escape **0.3.1** (2026-11-08), htmlize **1.1.0** (2026-04-13; `unescape` feature pulls phf tables). [V] With `raw_json=1` only three entities ever matter; three `str::replace` calls (`&lt;`, `&gt;`, then `&amp;` last) need no crate.
- No crate implements Reddit's dialect (snoomark): `>!spoiler!<`, `^super` / `^(super group)`, bare `r/sub` and `u/user` autolinks, `![gif](giphy|ID)` / `![img](media_id)` embeds resolved via `media_metadata`, `&#x200B;` zero-width paragraphs, backslash-escaped underscores in URLs. [M for the syntax list; consistent with redlib stripping `\_`/`%5C`]

### Markdown as-is vs plain text for an LLM
Keep the markdown. LLMs read markdown natively, and it carries meaning that stripping destroys: `>` quote blocks show which words belong to someone else, lists and tables keep structure, code fences mark literal text, links keep their targets, `>!...!<` marks spoilers. Stripping risks merging a quoted sentence into the author's own voice, which is exactly the misattribution an agent must not make. Do only light, lossless-ish normalisation for output:
1. request `raw_json=1` (decode the three entities only if needed);
2. remove `&#x200B;` / U+200B and trailing whitespace, collapse 3+ blank lines;
3. rewrite `![gif](giphy|ID|variant)` → `[gif]`, `![img](media_id)` → `[image]` (or the resolved URL from `media_metadata`);
4. optionally unescape `\_` inside URLs;
5. leave everything else untouched, and emit `"[deleted]"`/`"[removed]"` as explicit null-with-reason fields rather than body text.

For the **FTS index** a cleaned column helps ranking slightly (URLs otherwise become junk tokens): either index the raw body (simplest; unicode61 already drops punctuation) or derive plain text with pulldown-cmark by concatenating `Event::Text`/`Code` and dropping link destinations (~30 lines, pure Rust). Start with raw; add the derived column only if search quality demands it.

---

## 8. Secret storage on Windows

- Win32 limits: `CredentialBlobSize` "cannot be larger than CRED_MAX_CREDENTIAL_BLOB_SIZE (5*512) bytes" = **2560 bytes**; generic `TargetName` up to 32767 chars; `UserName` up to 513; `Comment` 256; persist values SESSION / LOCAL_MACHINE / ENTERPRISE (Enterprise roams with a roaming profile). [V] https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw (page updated 2025-11-25)
- A Reddit refresh token is far below that limit (tens of characters; exact length [M]). If the secret is stored as UTF-16 the usable length halves — store UTF-8 bytes.
- **keyring** is now **4.2.0** (2026-08-29); features are just `cli`, `v1`, `default`. v4 is a convenience wrapper: real work moved to **keyring-core 1.0.1** (2026-10-13) plus per-platform store crates; the docs say apps wanting control should "link to keyring-core and specific store crates instead". The v3-era `windows-native` feature flag no longer exists in v4 (keyring 3.6.3 had it, pulling `windows-sys` 0.60 + `byteorder`). [V] crates.io API; https://docs.rs/keyring/latest/keyring/; https://docs.rs/crate/keyring/3.6.3/source/Cargo.toml
- **windows-native-keyring-store 1.1.0** (2026-05-24): generic credentials in Credential Manager; deps `keyring-core`, `windows-sys ^0.61.2`, `byteorder`, `zeroize`, optional `regex` (default feature `search`); default persistence **Enterprise** (settable to Local); usage `keyring_core::set_default_store(windows_native_keyring_store::Store::new().unwrap())`. Warning: operations on the same entry from several threads are not ordered. docs.rs builds it only for msvc. [V] https://docs.rs/windows-native-keyring-store/latest/windows_native_keyring_store/
- windows-gnu: all of the above are pure Rust over `windows-sys`, whose import libraries ship for the gnu target, so they should build with no C at all. [I] No GNU-specific issue found; no explicit confirmation either.
- Options compared:
  - *keyring-core + windows-native-keyring-store* — ~4 small crates, no unsafe in our code, shows up in the Windows Credential Manager UI where the user can inspect/delete it.
  - *Direct `CredWriteW`/`CredReadW`/`CredDeleteW`/`CredFree` via `windows-sys` (`Win32_Security_Credentials`, `Win32_Foundation`)* — ~60–80 lines of unsafe FFI, one dependency that is already in the tree transitively. Fewest deps; we own the UTF-16 and pointer handling.
  - *DPAPI `CryptProtectData` → file in %LOCALAPPDATA%* — no size limit, same user-bound protection, but we manage the file, and it is invisible in Credential Manager. Only worth it for secrets > 2560 bytes.
- All three protect against other users and offline disk reads, **not** against other processes running as the same user. [M — standard Windows behaviour]

**Recommendation:** store only the **refresh token** in Credential Manager (target such as `<app-name>/reddit/refresh_token`, persistence Local, UTF-8 blob). Keep the client id in the ordinary config file — for an "installed app" it is not a secret. Never cache the access token on disk (1 h lifetime [M]); keep it in memory. Start with keyring-core + windows-native-keyring-store (`default-features = false` to drop regex); swap to direct `windows-sys` later if dependency count matters.

---

## 9. CLI and misc

| Need | Options (version, date) | Pick |
|---|---|---|
| Args | clap **4.6.7** (2026-09-14); lexopt 0.3.2 (2026-02-28); pico-args 0.5.0 (2022-06-04, dormant); argh 0.1.19 (2026-03-16) [V] | **clap derive** with trimmed features (`default-features = false, features = ["std","derive","help","usage","error-context"]`). An agent-facing CLI with several subcommands benefits from generated `--help` and strict validation. If compile time matters more: lexopt (no proc macros). Compile-time numbers not measured/found. |
| Time | jiff **0.2.43** (2026-11-11, still 0.x; bundles a tz database on Windows by default); time **0.3.55** (2026-08-01); chrono **0.4.45** (2026-06-04) [V] | We only convert epoch seconds ↔ RFC 3339 UTC and parse `--since` dates: **`time`** with `formatting`/`parsing`/`macros`, or jiff with `default-features = false, features = ["std"]`. Either is pure Rust. |
| URLs | url **2.5.8** (2026-01-05); percent-encoding **2.3.2** (2025-08-21, already a ureq dep) [V] | `url` for building the authorize URL and parsing the redirect query (`form_urlencoded`). |
| Paths | directories **6.0.0** (2025-01-12); dirs **7.0.0** (2026-09-05) [V] | Windows-only tool: `std::env::var_os("LOCALAPPDATA")` — no crate. |
| OAuth redirect listener | tiny_http **0.12.0** (2022-10-06 — no release in 4 years; 58 open issues) [V] | For one redirect: `std::net::TcpListener` on `127.0.0.1:<port>`, read the request line, parse `code`/`state`, answer a fixed HTML page (~40 lines). Add tiny_http (pure Rust, no TLS features) only when the later local window needs routing. Bind 127.0.0.1 only; verify `state`. |
| Open browser | open **5.4.5** (2026-10-22); webbrowser **1.2.7** (2026-11-11) [V] | `open` (tiny), or zero-dep `Command::new("rundll32").args(["url.dll,FileProtocolHandler", url])`. Always also print the URL. |
| zstd dumps | zstd **0.14.4** (2026-11-12; wraps the C library via `cc`); ruzstd **0.9.1** (2026-10-11, pure-Rust decoder, MIT) [V] | Decode-only → **ruzstd** behind an optional cargo feature. Reddit archive dumps use a 2 GB long-distance window; ruzstd's handling/perf with that window is unverified. Fallback: `zstd` crate (needs only GCC [I]). |
| Compression of responses | flate2 via ureq `gzip` (default backend miniz_oxide, pure Rust [M]) | keep `gzip`. |
| Tests | wiremock 0.6.5 (2025-08-24; async, tokio); httpmock **0.9.0** (2026-10-20; sync API exists but `tokio` and `hyper` are non-optional deps); mockito **1.7.2** (2026-02-02); insta **1.49.0** (2026-10-03) [V] | **Trait-based transport** (`trait Transport { fn send(&self, req: Req) -> Result<Resp> }`), a `FixtureTransport` that maps method+path+query to recorded JSON files and canned headers (including rate-limit headers and 429s), and `insta` snapshots (`features = ["json"]`) of the normalised output. No network, no tokio in dev-deps. A `--record` debug flag that writes sanitised live responses into the fixture folder keeps fixtures honest (strip tokens and usernames). |

---

## 10. Recommended Cargo.toml (minimal-C GNU build)

Only C compiled: SQLite amalgamation and ring's small C/prebuilt-asm core — both via `cc` with plain GCC. No cmake, NASM, perl, bindgen, OpenSSL.

```toml
[package]
edition = "2021"
rust-version = "1.85"          # ureq 3.5 MSRV

[dependencies]
# HTTP: blocking, rustls + ring (ureq default), gzip, JSON
ureq       = { version = "3.5", features = ["json"] }                 # default = rustls(ring) + gzip
# fallback if ring misbehaves on GNU (SChannel, zero C):
# ureq     = { version = "3.5", default-features = false, features = ["native-tls", "platform-verifier", "gzip", "json"] }

serde      = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# storage + FTS5 (bundled SQLite 3.54.1 with 0.42; FTS5 on by default)
rusqlite   = { version = "0.42", features = ["bundled"] }             # or pin "=0.41.1" for a few weeks

# CLI
clap       = { version = "4.6", default-features = false, features = ["std", "derive", "help", "usage", "error-context"] }

# time + URL
time       = { version = "0.3", features = ["formatting", "parsing", "macros"] }
url        = "2.5"

# refresh token in Windows Credential Manager
keyring-core                = "1.0"
windows-native-keyring-store = { version = "1.1", default-features = false }

# consent URL
open       = "5.4"

# optional
pulldown-cmark = { version = "0.13", default-features = false, optional = true }   # plain-text column for FTS
ruzstd         = { version = "0.9", optional = true }                              # archive dumps
tiny_http      = { version = "0.12", optional = true }                             # later local window

[dev-dependencies]
insta = { version = "1.49", features = ["json"] }
```
Hand-rolled instead of crates: rate limiter/backoff, OAuth redirect listener (std `TcpListener`), `%LOCALAPPDATA%` lookup, entity decode, random `state` (use `getrandom` if a CSPRNG crate is wanted; it is already transitively present via ring [I]), base64 for the Basic auth header on the token endpoint (`base64` is already a ureq dependency; add it directly or hand-roll).

Deliberately absent: tokio, reqwest, hyper, aws-lc-rs, openssl, tantivy, governor, backoff, roux, comrak, chrono, directories.

### Where GNU bites
1. **aws-lc-sys** — reqwest 0.13 and plain `rustls` default to it. On windows-gnu it needs GCC plus NASM or the prebuilt-NASM objects; compiles a big C tree; third-party failure reports exist. Avoid by using ureq (ring) or native-tls. Check with `cargo tree -i aws-lc-sys` after every dependency bump; rustls 0.24 will reshuffle provider features.
2. **ring** — fine in principle (prebuilt Windows asm objects, C via GCC, no perl/NASM for crates.io releases), but upstream says MSVC is "supported best", the last release is March 2025, and newer GCC versions can surface warnings/flag issues. Building ring from **git** does need perl + NASM — never use a git dependency on it.
3. **cmake / bindgen crates** — anything pulling `cmake`, `bindgen` (needs libclang), `openssl-sys` vendored (perl), `boring-sys`, `onig_sys`, `curl-sys` (isahc), `zstd-sys` with odd flags. Offenders seen in this research: comrak default features (`syntect-onig`), redlib's `boring-sys2`, rusqlite `bundled-sqlcipher*`/`buildtime_bindgen`/`session`, tantivy's default `columnar-zstd-compression`.
4. **cc needs gcc on PATH** — libsqlite3-sys and ring call `gcc.exe`/`ar.exe` through the `cc` crate. WinLibs must be on PATH in every shell that runs cargo (and the same GCC should be the linker rustc uses; mixing the rustup self-contained MinGW linker with a much newer WinLibs runtime can cause link errors). [I]
5. **docs.rs / CI coverage** — most Windows-specific crates (windows-native-keyring-store, keyring) only build docs and CI for msvc; GNU works through `windows-sys` import libs but is rarely tested upstream. Smoke-test the credential read/write early.
6. **Static linking/runtime DLLs** — depending on the WinLibs flavour (UCRT vs MSVCRT, posix vs win32 threads) the exe may need `libgcc_s_seh-1.dll`/`libwinpthread-1.dll`; prefer static (`-C target-feature=+crt-static`) and test on a clean PATH. [M]
7. **Fresh releases** — rusqlite 0.42.0 (yesterday), ureq 3.5.2 (today), zstd 0.14.4 (today), comrak 1.0.0-rc.1 (today). Commit `Cargo.lock`; do not chase same-day releases.
8. **MSRV creep** — ureq 1.85, reqwest 1.85, tantivy 1.88. Keep the GNU toolchain current.

---

## Could not verify

- No test build was run (by instruction). All "builds with GCC only" claims rest on build scripts/docs.
- **Official Reddit pages were unreachable** (`www.reddit.com/dev/api` blocked for the fetcher; `support.reddithelp.com` Data API wiki → 403). Therefore unverified from primary sources: the current 100 QPM / 10-minute-average limit; `raw_json=1` wording; current `/api/morechildren` parameters (`api_type`, `limit_children`, `depth`) and the 100-children-per-call limit; access-token lifetime; current User-Agent rule wording. What I have is the **archived** wiki/code (60 req/min; one morechildren request at a time, 429 on contention) and PRAW/redlib behaviour.
- Whether Reddit actually returns `Retry-After` on 429 — not confirmed; implement the fallback chain regardless.
- `removed_by_category` value list; `gallery_data` / `media_metadata` field shapes (`status`, `e`, `m`, `s`, `p`); `author_fullname` absence for deleted accounts; `[deleted]` vs `[removed]` semantics; full t1–t6 kind table; Reddit markdown dialect details — from memory/third-party code, not from a Reddit document. Validate against recorded fixtures.
- roux source models (raw GitHub path 404'd): how it types `edited`, whether any comment-tree/morechildren/search-by-query method exists beyond what docs.rs listed. Last commit date and release notes for 2.3.0 not seen.
- ring discussion #2775 (GNU build flags) — seen only via a mirror; status unknown. aws-lc-rs windows-gnu failures — third-party logs only; no upstream issue number confirmed.
- rusqlite / libsqlite3-sys open issues specific to windows-gnu — not searched in the tracker.
- keyring-core + windows-native-keyring-store building on windows-gnu — inferred, not confirmed.
- tantivy: binary size, windows-gnu status, and whether `columnar-zstd-compression` truly links C zstd (inferred from feature chain).
- `windows-sys` latest version: API response was truncated; 0.61.2 (2025-10-06) reported and consistent with windows-native-keyring-store's `^0.61.2`.
- ureq 3 `http_status_as_error` name/behaviour; flate2 default backend; mockito's internal use of tokio; pulldown-cmark superscript syntax; FTS5 trigram minimum SQLite version (3.34); clap vs lexopt compile-time numbers; tiny_http last commit date; ruzstd behaviour on long-window Reddit dumps; `zstd` crate building cleanly with WinLibs GCC.
- redlib latest release date and last commit date (page did not show them).
