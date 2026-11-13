# Research 6: Prior art for a Reddit search / thread-fetch / SQLite-cache CLI

Fetched 2026-11-12 (web only: WebSearch + WebFetch). Repo metadata (stars, licence, `pushed_at`, archived flag) comes from the public GitHub/GitLab REST API on that date unless noted. "Last push" is the repo's `pushed_at`, which can be a bot or docs commit, not necessarily a code change. Anything I could not confirm from a primary source is listed in section 8.

---

## 0. Read this first: the platform is closing under the plan

Every project below has to be read against this timeline. It is the single most important finding, and I could only confirm it from secondary sources (reddit.com cannot be fetched by the tool).

| Date | Change | Source |
|---|---|---|
| May 2023 | Reddit revokes Pushshift's API access | timesearch README changelog (https://github.com/voussoir/timesearch) |
| Jul 2023 | API pricing; per-app (not per-user+app) rate limits | RedditArchiver README (https://github.com/Ailothaen/RedditArchiver) |
| Nov 2025 | "Responsible Builder Policy": self-service OAuth app creation closed, new tokens need admin approval | RedditArchiver README; jordanburke/reddit-mcp-server README |
| Mid-2026 | Unauthenticated `.json` requests return 403 | jordanburke/reddit-mcp-server README; roux README ("no longer serves its JSON endpoints to unauthenticated clients") |
| 2026-09-30 | Reddit announces end of RSS and of public Data API (r/redditdev + r/modnews posts) | TechCrunch 2026-09-30 (https://techcrunch.com/2026/09/30/reddit-is-killing-rss-feeds-ending-public-api-access-because-of-ai-bots/); ppc.land (https://ppc.land/reddit-to-halt-public-data-api-access-in-march-2027-as-14-000-apps-register/) |
| 2026-10-31 | New public API access requests no longer accepted | ppc.land; reddit-mcp-buddy README |
| 2026-11-13 | RSS feeds end (tomorrow relative to this fetch) | TechCrunch; reddit-mcp-buddy README |
| 2027-01-12 | Access "starts to close" for apps/users not registered at developers.reddit.com | ppc.land; TechCrunch |
| March 2027 | Public Data API access closes for all remaining apps; only partners with written agreements exempt | ppc.land; TechCrunch |
| Q4 2026 onward | Reddit testing a logged-in-only site (Q3 2026 shareholder letter, as quoted in redlib issue #609) | https://github.com/redlib-org/redlib/issues/609 |

Consequences for our design:
- An official-OAuth-only CLI has a hard dependency on (a) already holding a registered app, and (b) whatever survives March 2027. Neither TechCrunch nor ppc.land says personal read-only script apps are exempt; ppc.land says the question was asked in the thread and no admin answered.
- The tool should be built **cache-first with pluggable sources** (official API now; Arctic Shift API / dump ingest as a second source) so the local SQLite corpus keeps its value if the live route goes away.
- Do not build on unauthenticated `.json`, RSS, or spoofed app tokens: all three are dead or dying as of this fetch.

---

## 1. API client libraries

| Project | URL | Lang | Licence | Last push / release (as fetched) | Stars | Access route | State |
|---|---|---|---|---|---|---|---|
| PRAW | https://github.com/praw-dev/praw | Python | BSD-2-Clause | push 2026-11-09; v8.0.3 released 2026-08-12 | ~4,300 | Official OAuth (script, web, installed, app-only) | Active; 1 open issue. Reference implementation |
| prawcore | https://github.com/praw-dev/prawcore | Python | BSD-2-Clause | push 2026-11-10 | 28 | Official OAuth transport layer | Active |
| Async PRAW | https://github.com/praw-dev/asyncpraw | Python | BSD-2-Clause | push 2026-11-09 | 156 | Official OAuth | Active, feature parity with PRAW |
| snoowrap | https://github.com/not-an-aardvark/snoowrap | JS | MIT | push 2023-02-20 | ~1,030 | Official OAuth | **Archived 2024-03-17**, 95 open issues left |
| JRAW | https://github.com/mattbdean/JRAW | Kotlin/Java | MIT | push 2021-11-21 | 362 | Official OAuth | Not archived but dormant (Travis/Bintray/JCenter references, 37 open issues) |
| go-reddit | https://github.com/vartanbeno/go-reddit | Go | MIT per README (API says NOASSERTION) | push 2024-03-15 | 333 | Official OAuth password grant; also a `NewReadonlyClient` that behaves like a logged-out user | Dormant; the read-only client relies on unauthenticated access that now 403s |
| Reddit.NET | https://github.com/sirkris/Reddit.NET | C# | MIT | push 2023-01-27 | 505 | Official OAuth | Dormant, 82 open issues |
| roux | https://github.com/halcyonnouveau/roux | Rust | MIT | push 2026-09-08 | 115 | Official OAuth: password grant + app-only token (client id/secret) | "Not in active development" but maintained; README now states unauthenticated JSON is gone and marks `Subreddit::new`, `User::new`, `Subreddits::search` deprecated (likely 403). Open issue #73 "Improve handling of rate limiting" since 2023-08; #34 installed-app code flow since 2021. No documented morechildren support |
| redditwarp | https://github.com/Pyprohly/redditwarp | Python | MIT | push 2024-07-01 | 59 | Official OAuth; pluggable HTTP transport; sync + async | Dormant since mid-2024 but a clean design reference (typed models, formal tree/pagination structures, CLI helpers for refresh tokens). Used underneath two MCP servers |

### 1.1 PRAW design notes (read from source on 2026-11-12)

**Rate limiting (`prawcore/rate_limit.py`)** — https://raw.githubusercontent.com/praw-dev/prawcore/main/prawcore/rate_limit.py
- State: `remaining`, `used`, `next_request_timestamp_ns` (monotonic clock). Updated after every response from `x-ratelimit-remaining`, `x-ratelimit-used`, `x-ratelimit-reset`.
- If the headers are absent: decrement `remaining`, increment `used`, leave the timestamp alone.
- If `remaining <= 0`: sleep until reset, minimum 1 second.
- Otherwise the delay before the next request is
  `min(reset, max(reset - window * remaining / (remaining + used), 0), 10)` seconds.
  In words: spread remaining requests evenly over the remaining window, never sleep more than 10 s, never more than `reset`. `window_size` is a config option added in PRAW 7.8.0 (2024-10-20), default 600 s.
- `remaining` is parsed as `int(float(...))` — Reddit returns it as a float string. A Rust port must parse it as f64.
- This is proactive pacing, not reactive: it avoids ever hitting 429. bdfr issue #954 ("Does not work with PRAW 7.7.1, throws a 429 HTTP Response instead", 2024-03) is what happens when the window assumption is wrong; 7.8.0's `window_size` was the fix.

**Retries (`prawcore/sessions.py`)** — https://raw.githubusercontent.com/praw-dev/prawcore/main/prawcore/sessions.py
- `FiniteRetryStrategy`, 2 retries (3 attempts). Sleeps: 0–2 s jittered before retry 1, 2–4 s before retry 2.
- Retried: connection errors, chunked-encoding errors, read timeouts, and statuses 408, 500, 502, 503, 504, 520, 522.
- 401: clear access token; retry once if the authorizer can refresh.
- 429 is **not** retried at this layer; it maps to `TooManyRequests`.
- Explicit status-to-exception table (301/302 Redirect, 400, 401/403, 404, 409, 413, 414, 415, 429, 451, 5xx), plus `BadJSON` for unparseable bodies. Worth copying as a typed error enum.

**Auth (`prawcore/auth.py`)** — https://raw.githubusercontent.com/praw-dev/prawcore/main/prawcore/auth.py
- `ReadOnlyAuthorizer` = `grant_type=client_credentials` (application-only OAuth, needs a confidential client).
- `DeviceIDAuthorizer` = `https://oauth.reddit.com/grants/installed_client` with a device id (default `DO_NOT_TRACK_THIS_DEVICE`) for installed apps without a secret.
- `ScriptAuthorizer` = password grant, optional 2FA callback (`otp`).
- `Authorizer` = refresh-token grant with pre/post refresh callbacks.
- Expiry is tracked on the monotonic clock using a timestamp taken **before** the token request. `is_valid()` is purely local; the session refreshes before a request if invalid and again on a 401.
- PRAW 8.0.0 (2026-06-14) **removed token managers** entirely (`token_manager` kwarg and all manager classes). bdfr issue #970 ("module 'praw.reddit' has no attribute BaseTokenManager", 2024-10) shows downstream breakage from depending on that abstraction. Lesson: persist the refresh token yourself with a small, boring store; don't build a plugin system for it.

**Read-only mode and User-Agent** — https://praw.readthedocs.io/en/stable/getting_started/quick_start.html
- Read-only needs client id + secret + user agent only; adding username/password makes it authorized; `reddit.read_only = True` switches back.
- UA format: `<platform>:<app ID>:<version string> (by u/<username>)`.
- Objects are lazy; listings do nothing until iterated.

**Listings (`praw/models/listing/generator.py`)**
- Default `limit=100`. Per-request `limit` param = `request_limit or limit or 1024` (Reddit clamps to 100 in practice).
- Follows `after`; stops when yielded == limit, a batch is empty, or `after` is missing or unchanged (the unchanged-cursor check prevents infinite loops).
- The 1000 cap is not enforced by the client; it is only documented: listings "contain a maximum of 1000 items".

**Comment trees (`comment_forest.py`, `reddit/more.py`)**
- `replace_more(limit=32, threshold=0)`. `limit=None` = expand everything; `limit=0` = strip all placeholders with zero requests.
- BFS gather of all `MoreComments` into a heap. `MoreComments.__lt__` is `self.count > other.count`, so the heap pops the **largest** placeholder first — the request budget is spent where it returns the most comments.
- Each pop: if budget exhausted or `count < threshold`, drop it (returned in a "skipped" list); else fetch, decrement budget, push any new placeholders found in the response, insert comments under their parents via a `_comments_by_id` map.
- Two fetch paths:
  - `count == 0` is "continue this thread": no children ids; PRAW re-fetches `/comments/{submission}/_/{parent_id}` and takes that comment's replies.
  - Otherwise `POST /api/morechildren` with `children` (comma-joined ids), `link_id` (submission fullname), `sort`. PRAW does not chunk the id list.
- Requests run one after another; the docstring warns `TooManyRequests` can be raised when used concurrently and suggests a retry loop.
- `DuplicateReplaceException` if an id arrives twice (bdfr issue #985, 2025-04, hit this in the field). `AssertionError` if a non-root comment's parent is not in the index.
- `replace_more` is destructive and one-shot; calling it again does nothing.
- `submission.num_comments` will not match the extracted count (it includes deleted/removed/spam) — tutorial caveat, https://praw.readthedocs.io/en/stable/tutorials/comments.html
- In PRAW 8, `comment_sort` / `comment_limit` must be set before the fetch or a `ClientException` is raised (previously a silent no-op with a warning).

**Action rate limits** — https://praw.readthedocs.io/en/stable/getting_started/ratelimits.html: `ratelimit_seconds` default 5; if Reddit asks for a wait at or under that, sleep wait+1 and retry, otherwise raise. Only relevant to write actions.

**Testing** — https://praw.readthedocs.io/en/stable/package_info/contributing.html: integration tests replay recorded HTTP "cassettes" (now **vcrpy**; older PRAW used Betamax). Credentials are replaced by placeholders (`<PASSWORD>`, etc.) when recording; to re-record, delete the cassette and rerun. 100% coverage is required. Custom markers for extra placeholders and cassette naming.

---

## 2. Archival / downloader tools

| Project | URL | Lang | Licence | Last push | Stars | Access route | State |
|---|---|---|---|---|---|---|---|
| Bulk Downloader for Reddit (bdfr) | https://github.com/Serene-Arc/bulk-downloader-for-reddit | Python | GPL-3.0 | 2026-04-12 | ~2,630 | Official OAuth via PRAW; ships a **default client id/secret** in config; user token only for saved/upvoted | Degrading: 2026 issues "Cannot download anything" (#1019, 2026-09), "Program don't work" (#1014), JSONDecodeError on unauthorized session (#997), 429s (#954, #977, #991). 112 open issues |
| reddit-user-to-sqlite | https://github.com/xavdid/reddit-user-to-sqlite | Python | MIT | 2023-07-24 | 230 | **Unauthenticated `www.reddit.com/...json`**, UA literally `reddit-user-to-sqlite`; plus GDPR-export import | Abandoned since Jul 2023; route now 403s (no issue filed, inferred from the route) |
| reddit-to-sqlite | https://github.com/catherinedevlin/reddit-to-sqlite | Python | MIT | 2026-04-25 | 3 | Official OAuth (PRAW, script creds in `~/.config/reddit-to-sqlite.json`) | Tiny, alive |
| Expanse (formerly Eternity) | https://github.com/aplotor/expanse (was jc9108/expanse) | JS | AGPL-3.0 | 2025-05-30 | 430 | Official OAuth (user's own app); syncs saved/created/voted/hidden | **Archived** |
| redditwarp | see section 1 | | | | | | |
| timesearch | https://github.com/voussoir/timesearch | Python | BSD-3-Clause | 2026-10-31 | 182 | Originally Reddit timestamp search (removed 2018) then **Pushshift** + `/api/info` refresh | Historical fetch dead since 2023-06 ("Pushshift's API is currently offline"); survives via `ingest_jsonfile` (dump import) and `livestream` |
| redarc | https://github.com/Yakabuff/redarc | Python + React | MIT | 2024-02-09 | 199 | Pushshift dump ingest + PRAW workers | Dormant since Feb 2024; master "unstable" |
| RedditArchiver | https://github.com/Ailothaen/RedditArchiver | Python (HTML output) | MIT | 2026-01-24 | 100 | Official OAuth web app, user links account | README declares the project unusable for anyone without a pre-Nov-2025 token |
| Reddit-Archive-Host | https://github.com/DrPugsley/Reddit-Archive-Host | Python | none stated | 2018-09-25 | 55 | Not verified | Dead since 2018 |
| subreddit-archiver | https://github.com/arshadrr/subreddit-archiver | Python | GPL-3.0 | 2025-05-09 | 27 | Official API + Pushshift | **Archived**; README: "The Reddit API is no longer free" |
| URS | https://github.com/JosephLai241/URS | Python (+ some Rust tooling) | MIT | 2026-07-11 | ~1,050 | Official OAuth via PRAW script creds | Alive; CSV/JSON export, structured vs `--raw` comments, `--check` shows PRAW rate-limit state |
| SocialGrep | (commercial site) | n/a | proprietary | n/a | n/a | Own archive | Could not verify 2026 status |
| shreddit (note only) | https://github.com/andrewbanchich/shreddit | Rust | MIT | 2026-11-02 | 353 | Official OAuth + GDPR export | Deleter; active. Useful only as a Rust OAuth example |
| PowerDeleteSuite (note only) | https://github.com/j0be/PowerDeleteSuite | JS bookmarklet | none | 2024-11-21 | ~2,200 | Runs in the user's logged-in old.reddit session | Depends on old Reddit, which is being login-gated |
| Watchful1/PushshiftDumps | https://github.com/Watchful1/PushshiftDumps | Python | MIT | 2026-11-03 | 511 | Offline `.zst` dumps via Academic Torrents | Active. Scripts: `single_file.py`, `iterate_folder.py`, `combine_folder_multiprocess.py` |
| Arctic Shift | https://github.com/ArthurHeitmann/arctic_shift | TypeScript | **no licence** | 2026-10-11 | ~1,840 | Own archive (collection method documented separately, not read); dumps + API + web search UI + download tool | Active; one volunteer; no uptime guarantee |

### 2.1 Design notes worth keeping

**reddit-user-to-sqlite (schema is the best small reference)** — https://raw.githubusercontent.com/xavdid/reddit-user-to-sqlite/main/reddit_user_to_sqlite/sqlite_helpers.py
- Tables: `subreddits(id pk, name, type)`, `users(id pk, username)`, `comments(id pk, timestamp, score, text, user fk, subreddit fk, permalink, is_submitter, controversiality, num_awards)`, `posts(id pk, timestamp, score, num_comments, title, text, external_url, user fk, subreddit fk, permalink, upvote_ratio, num_awards, is_removed)`.
- Primary keys are the base-36 id with the `t5_`/`t2_` prefix stripped. Upserts on posts/comments/subreddits; users are insert-if-new.
- FTS via sqlite-utils `enable_fts` **with triggers** on `comments(text)` and `posts(title, text)`.
- `alter=True` so new columns appear automatically.
- What it got wrong (from its own issues, all opened June 2023 and never closed):
  - #22 no `saved_at`/fetched-at timestamp;
  - #13 "occasionally fetches wrong scores" (Reddit fuzzes scores; with no fetch timestamp or history you can't reason about it);
  - #12 needs multiple sort orders to get past 1k items;
  - #8 does not store post context for comments (no parent/thread linkage);
  - README admits it slims the raw JSON instead of keeping it, so missing fields require a refetch;
  - items with no `author_fullname` (deleted users) are **dropped** rather than stored;
  - `is_removed` is derived from `selftext == "[removed]"` at fetch time only, so an upsert of a later-removed post overwrites the good text.
- API layer: `raw_json=1`, `limit=100`, max 10 pages, `/api/info` in batches of 100 ids, error detection by JSON `"error"` key rather than HTTP status (never calls `raise_for_status`). On 429 it prints and returns partial data — silent truncation.

**reddit-to-sqlite** — incremental refresh by walking back in time until the last saved post **plus an overlap window** (7 days default for posts, 1 day for user comments; `--post_reload` / `--comment_reload` to widen) so late comments and score changes get picked up. Two tables plus a unifying `items` view. README is candid that the 1000-item window means history is never guaranteed complete and that deep chains may be missed.

**bdfr** — three verbs: `download` (media), `archive` (JSON/XML/YAML of submission + comments), `clone` (both, one pass). MD5 dedupe with optional hard links. README on the ~1000 cap: "We cannot bypass this." Rate-limit waits grow in 60 s steps up to `max_wait_time` (120 s default). Shipping a shared default client id means every user shares one app's rate limit and one point of revocation since 2023's per-app limits.

**redarc** — two Postgres databases (content and a separate `postgres_fts`), Redis queue, separate workers (`reddit_worker` fetch via PRAW, `subreddit_worker` polls hot/new/rising, `index_worker` batches rows into FTS on an interval, `image_downloader`). `status` and `progress` tables. Search API takes subreddit, time range, parent_id, link_id, sort. Limitation it states itself: ingest workers must be stopped while loading dumps; many moving parts for what a single SQLite file with FTS5 would cover at personal scale.

**timesearch** — one SQLite db per subreddit/user; `merge_db`; `offline_reading` renders threads to HTML; `ingest_jsonfile` added 2023-06-25 specifically so the tool survives without its API. It has now outlived two upstream data sources (Reddit timestamp search in 2018, Pushshift in 2023) by adding an importer each time.

**subreddit-archiver** — resumable: progress is stored in an `archive_metadata` table inside the output SQLite file, so re-running the same command continues. `update` only fetches newer posts; it never revisits old ones, so edits/removals/late comments are missed (documented limitation).

**Expanse/Eternity** — beat the 1000-item cap for *your own* saved/voted lists by syncing continuously and never deleting locally what Reddit later drops; plus CSV import from Reddit's data-request export. The only real cure for the cap is "start collecting early and keep what you have".

**RedditArchiver** — single self-contained HTML per thread. README notes 2023's switch to per-app rate limits made its per-user "query saving" design pointless.

**Arctic Shift API** — https://github.com/ArthurHeitmann/arctic_shift/blob/master/api/README.md
- `/api/posts/search`, `/api/comments/search`, `/api/comments/tree` (needs `link_id`; `limit` up to 25,000; collapses beyond limit/breadth/depth), `/api/{posts,comments,subreddits,users}/ids` (500 ids per call), aggregations.
- Full-text search **only together with** `subreddit` or `author` (or `link_id`/`parent_id` for comments), and not for very active subreddits/users. No global keyword search.
- Dynamic rate limits; 429 with `X-RateLimit-Reset` / `X-RateLimit-Reset-At`.
- Scores/comment counts are 0 or 1 at capture and are refreshed after roughly 36 hours.
- No cursor: page by moving `after`/`before` on `created_utc` yourself.
- Repo has no licence; data legality is unsettled (it runs a removal-request form).

---

## 3. Search front-ends / alternative UIs

| Project | URL | Lang | Licence | Last push | Stars | Access route | State |
|---|---|---|---|---|---|---|---|
| redlib | https://github.com/redlib-org/redlib | Rust | AGPL-3.0 | 2026-10-22 | ~4,000 | **Spoofed official-app OAuth**: imitates the Android app (headers, 24 h token refresh); chose Android because anonymous iOS sessions had content restrictions | Chronically blocked. Most-commented issues: #446 "Rate limit part ???" (2025-06, open, 485 comments); #324/#229/#301 "Failed to parse page JSON data" (2024); #318 "IP ban investigation" (2024-11); #485 "Failed to create OAuth client" (2025-09, open); #591 "Thread pages fail with 403 from Reddit" (2026-10); #609 "Reddit has decided to force everyone to log in" (2026-10-30, open). Users now fall back to a fork that takes session cookies from a throwaway account |
| libreddit | https://github.com/libreddit/libreddit | Rust | AGPL-3.0 | 2025-02-15 | ~5,200 | Unauthenticated JSON | Effectively dead since the 2023 rate limits; redlib is the successor |
| teddit | https://codeberg.org/teddit/teddit | JS (Node) | AGPL-3.0 | 2025-04-24 (instance-list cleanup) | n/a | Reddit public API, optional own-app OAuth (`use_reddit_oauth`, default off) | README: "teddit is no more actively maintained!" |
| camas / redditsearch.io | (web UIs) | JS | n/a | n/a | n/a | Pushshift (later PullPush) | Reported defunct by vendor blogs only; not verified |
| Reveddit | https://github.com/reveddit/reveddit | TypeScript | GPL-3.0 | 2026-11-12 | 143 | Pushshift archive diffed against live Reddit in the browser | Repo active; README still names Pushshift as the only source; real-world capability not verified (third parties call it "limited") |
| "Pushshift Reddit Search" UIs | various | | | | | Pushshift | Pushshift is moderator-only since 2023; PullPush is the compatible clone, described as unreliable (vendor sources) |
| rtv | https://github.com/michael-lazar/rtv | Python | MIT | 2023-02-20 | ~4,640 | Official OAuth (PRAW fork) | **Archived**; README: "THIS PROJECT IS NO LONGER MAINTAINED" |
| tuir | https://gitlab.com/ajak/tuir | Python | (MIT, inherited; not verified) | last activity 2026-04-06 | 180 | Official OAuth | rtv fork; low activity; working state not verified |
| rajatady/reddit-cli ("redditer") | https://github.com/rajatady/reddit-cli | TypeScript (Bun) | MIT | 2026-04-30 | 0 | Official OAuth, user registers own web app, loopback redirect `127.0.0.1:9780` | Tiny but the closest in intent to ours (see 4.2) |
| tamnd/reddit-cli | https://github.com/tamnd/reddit-cli | not stated | Apache-2.0 | 2026-06-20 | 1 | "public" Reddit data to JSON/JSONL; on-disk page cache + SQLite store, 24 h default TTL | Not examined in depth |

Reveddit's idea worth keeping: removal detection is a **diff between two observations** (archive vs live), and the time gap between creation and disappearance separates automod removals from later moderator removals. For us: keep the first-seen body and record state transitions instead of overwriting.

---

## 4. LLM / agent-oriented tools (2024–2026)

### 4.1 Reddit MCP servers

Star counts from GitHub search API on 2026-11-12. Status column partly from a third-party review dated 2026-11-03 (https://contextbolt.com/blog/reddit-mcp/ — written by a competing product's founder; treat as informed but interested).

| Server | URL | Lang / licence | Last push | Stars | Access route | Tools exposed | State |
|---|---|---|---|---|---|---|---|
| reddit-mcp-buddy | https://github.com/karanb192/reddit-mcp-buddy | TS / MIT | 2026-11-11 | ~890 | Official OAuth with user's keys (app-only, or + refresh token / password). Keyless mode = RSS only (ends 2026-11-13) | `browse_subreddit`, `search_reddit` (subreddit/author/time/flair filters; sort relevance/hot/top/new/comments), `get_post_details` (URL or id; comment sort, depth, link extraction), `user_analysis`, `find_subreddits`, `reddit_explain`; opt-in write tools | Most-starred dedicated server; actively tracking the shutdown timeline in its README |
| mcp-reddit | https://github.com/adhikasp/mcp-reddit | Python / MIT | 2025-05-11 | ~440 | Unauthenticated (redditwarp, no creds in config) | `fetch_hot_threads`, post content with comments | 403 in the third-party test; stale |
| reddit-mcp-server | https://github.com/jordanburke/reddit-mcp-server | TS / MIT | 2026-10-31 | ~330 | Official OAuth with keys; **keyless fallback = RSS + Arctic Shift archive** | `get_reddit_post`, `get_post_comments`, `get_more_comments`, `search_reddit`, `browse_subreddit`, `get_top_posts`, user/subreddit info, `get_my_saved`, write tools, `test_reddit_mcp_server` | Active; most resilient design of the group |
| reddit-mcp | https://github.com/Arindam200/reddit-mcp | Python / MIT | 2025-12-21 | ~300 | Official OAuth via PRAW | not enumerated | Needs keys |
| reddit-research-mcp | https://github.com/dialog-tools/reddit-research-mcp | Python / MIT | 2026-10-20 | ~265 | Hosted service (Descope OAuth to the service; PRAW behind it) | 3 meta-tools: `discover_operations`, `get_operation_schema`, `execute_operation`; operations `discover_subreddits` (semantic search over a ChromaDB index of 20k+ subreddits), `search_subreddit`, `fetch_posts`, `fetch_multiple`, `fetch_comments` | Hosted; fate after March 2027 unknown |
| mcp-server-reddit | https://github.com/Hawstein/mcp-server-reddit | Python / MIT | 2026-06-10 | 185 | Unauthenticated via redditwarp | 8 read tools; `get_post_content(post_id, comment_limit=10 [1–100], comment_depth=3 [1–10])` | 403 in the third-party test |
| eliasbiondo/reddit-mcp-server | (not fetched) | | | ~166 (third-party figure) | Unauthenticated `.json` | not enumerated | Its README warns of 403s (third-party report) |

Patterns across MCP servers:
- **Auth split is the survival predictor.** Every keyless/unauthenticated server broke in 2026; the two that survive use the user's own OAuth app. None uses spoofed tokens.
- **Tool surface converges on five verbs:** browse listing, search (with subreddit/time/sort), get post + comments (with `comment_limit` and `comment_depth`), user info, subreddit discovery. jordanburke adds an explicit `get_more_comments` so the *agent* decides whether to spend a request on expansion.
- **Output shaping:** fields-that-matter only, never raw payload (buddy); bounded comment count and depth as parameters with small defaults (Hawstein: 10 top-level, depth 3); source notes appended to every result that came from an archive rather than live Reddit (jordanburke); "as-of" timestamps on subscriber counts (buddy).
- **Caching:** in-memory LRU, 50 MB cap, TTL by type (buddy: posts 10 min, search 10 min, users 15 min, listings 5–15 min). Nobody persists; none has FTS. This is the gap our SQLite design fills.
- **Rate-limit behaviour toward the agent:** buddy returns "Retry in Ns" immediately instead of sleeping through the client's tool timeout. jordanburke retries 429 up to 3 times honouring `Retry-After`.
- **Accept permissive input:** URL, short link (redd.it), bare id, or id + subreddit (buddy notes the last saves an API call).
- **Self-test tool** (`test_reddit_mcp_server`) reporting config and auth state.
- **Progressive disclosure:** dialog's discover → schema → execute keeps the tool list tiny in context.

### 4.2 Frameworks and agent projects

| Project | URL | Access route | Thread selection / output shaping | Verdict |
|---|---|---|---|---|
| LangChain `RedditSearchRun` / `RedditSearchAPIWrapper` | https://github.com/langchain-ai/langchain-community (`utilities/reddit_search.py`) | PRAW, client id/secret/UA | One `subreddit.search(query, sort, time_filter, limit)`. Output: a flat string per post (title, user, subreddit, body, URL, category, score). **No comments fetched; post id not included in the string** | Too thin to answer anything; the answer on Reddit is usually in the comments |
| LangChain `RedditPostsLoader` | docs page moved; not re-read | PRAW | By subreddit or username, by category (hot/new/...); posts only (from memory of the loader; not re-verified today) | Same |
| LlamaIndex `RedditReader` | https://github.com/run-llama/llama_index (`llama-index-readers-reddit/.../base.py`) | PRAW, env vars incl. username/password | For each subreddit × keyword: `subreddit.search(kw, limit)`. Emits `selftext` and each **top-level** comment body as separate Documents. No `replace_more`, `MoreComments` skipped, **no metadata at all** (no id, URL, author, score, parent), no dedupe across keywords; signature default `post_limit=[10]` is a list (bug) | A catalogue of what not to do: uncitable, context-free chunks |
| GPT Researcher | https://github.com/assafelovic/gpt-researcher | n/a | Retriever directory has 21 back-ends (tavily, exa, arxiv, mcp, ...) and **no Reddit retriever**; Reddit would arrive via web search or the MCP retriever | Reddit is second-hand via search engines |
| last30days-skill | https://github.com/mvanhorn/last30days-skill (Python, MIT, ~67k stars, push 2026-11-13) | Default: **unauthenticated reddit.com** (search, listing partials, comment pages) behind one shared token bucket (1 req/s, burst 2), one retry on 429; optional paid ScrapeCreators back-fill when the free path yields fewer than 5 on-topic threads | Subreddit resolution first; results "scored by engagement, relevance, freshness" (weights not published in the docs read); comment enrichment for only the top 4 / 8 / 12 threads per sub-query by depth tier; identical requests within one run de-duplicated (about 4x fewer requests); upvote counts carried into citations; optional local SQLite store of findings | Best selection logic of the group; worst access route. Its default path is exactly what Reddit is shutting |
| Agent-Reach | https://github.com/Panniantong/Agent-Reach (Python, MIT, ~111k stars) | Reddit listed as a login-required channel; general mechanism is a background browser reusing the user's Chrome session (route for Reddit inferred, not stated) | n/a | Session-cookie scraping; not a model for us |
| rajatady/reddit-cli | https://github.com/rajatady/reddit-cli | Official OAuth, own app | Writes normalized JSON to a file and prints a **one-line summary** so payloads stay out of agent context; `--out -` for pipes; `--why` flag records intent; SQLite `history.db` logs every run with `rerun` / `fork`; `monitors.db` snapshots score/comment deltas over time; `--dry-run` | Good agent-ergonomics ideas; cache "not yet" built |
| gallery-dl (note only) | https://github.com/mikf/gallery-dl | Official OAuth with a bundled client id, user can supply own | media downloader | redarc installs it for media. Not examined today |

Thread-selection logic observed in the wild, most to least sophisticated:
1. last30days: resolve communities → search → score (engagement, relevance, freshness) → fetch comments only for top N by tier → cluster duplicates across sources.
2. dialog-tools: semantic subreddit discovery (vector index with confidence threshold) → in-subreddit search with time/sort → batch fetch.
3. MCP servers: leave selection to the model; expose sort/time filters and bounded comment depth.
4. LangChain / LlamaIndex: whatever `subreddit.search` returns first.

---

## 5. Academic / research pipelines after 2023

| Tool / source | URL | Route | State (as fetched) |
|---|---|---|---|
| Pushshift | pushshift.io | Was a third-party full archive + API | API access revoked May 2023; now moderator-only tooling (multiple secondary sources). Monthly dumps continued by others |
| Pushshift-format dumps | Academic Torrents via https://github.com/Watchful1/PushshiftDumps | `.zst` NDJSON files, monthly and per-subreddit | The de facto research corpus. Repo active 2026-11 |
| Arctic Shift | https://github.com/ArthurHeitmann/arctic_shift | Archive API + dumps + download tool for single subreddits/users | The usual recommendation for historical lookups; single maintainer; unlicensed |
| RedditHarbor | https://github.com/socius-org/RedditHarbor | Official API via PRAW into researcher-owned Supabase/Postgres | Active (push 2026-11-11, v0.4.0, MIT, 100 stars). Tables `user`, `submission`, `comment`; **only declared columns are requested and stored**; optional PII masking of `text`/`body` with Microsoft Presidio before storage; `update()` + scheduler stores `score`, `upvote_ratio`, `num_comments` as **timestamped histories**; keyword collection with AND/OR/NOT. README says nothing about the 2026–27 closure |
| 4CAT | https://github.com/digitalmethodsinitiative/4cat | Reddit datasource was Pushshift-based | Wiki lists the Reddit source as **inactive, archive-only**, unavailable since the July 2023 API pricing change (https://github.com/digitalmethodsinitiative/4cat/wiki/Available-data-sources). 4CAT itself is active |
| Communalytic | https://socialmedialab.ca/?p=22240 | Official Reddit API, keyword search for submissions | Announced Dec 2023; no 2025–26 status found |
| Reddit for Researchers | (Reddit programme) | Sanctioned access | Fast Company reports applicants hearing nothing back (https://www.fastcompany.com/91014116/reddit-researchers-bad-for-business); not addressed in the 2026 announcement coverage |

How researchers actually collect now, pieced together from the above:
- **History** = dumps (Academic Torrents) or Arctic Shift, accepting no legal or completeness guarantees.
- **Live/ongoing** = official API through PRAW with a registered app, typically writing to a database they own, re-polling to build score histories.
- Known data-quality caveats carried over from Pushshift: spotty recent coverage and undocumented gaps (UvA 4CAT field notes, https://cat4smr.humanities.uva.nl/?p=169); deletion ethics — archives keep what users later delete (Reagle, https://reagle.org/joseph/pelican/2023/reddit-pushshift-deletion.html).
- TechCrunch (2026-09-30) lists researchers among those who lose public API access in March 2027 and would need commercial deals.

---

## 6. Cross-cutting lessons

1. **Build cache-first with swappable sources; the live route is the least durable part.** timesearch lost Reddit timestamp search (2018) and Pushshift (2023) and survived only by adding importers (`ingest_jsonfile`); jordanburke/reddit-mcp-server stays useful keyless by falling back to Arctic Shift. Define a `Source` trait (official API, Arctic Shift, dump file) from day one. Section 0 is why.

2. **Tools on unauthenticated `.json` broke.** libreddit (2023 rate limits), reddit-user-to-sqlite (route dead, abandoned 2023-07), adhikasp/mcp-reddit and Hawstein/mcp-server-reddit (403 in Nov 2026 test), go-reddit's read-only client, roux's deprecated constructors ("will likely return a 403").

3. **Tools on Pushshift died in 2023.** timesearch historical fetch, subreddit-archiver (archived), 4CAT's Reddit datasource (inactive), camas/redditsearch UIs, Reveddit's core comparison. Never make a volunteer archive the *only* source; do make it an optional one.

4. **Spoofed app tokens are an arms race you lose.** redlib's top issues are all block/rate-limit/OAuth-client failures (#446 with 485 comments, #318 IP bans, #485, #591, #609). Besides being a terms problem, it is an availability problem.

5. **A shared, bundled client id is a shared failure.** bdfr ships a default id/secret; since 2023 limits are per app, so every user shares one quota (issues #954, #977, #991 are 429s) and one revocation. Require each user's own registered app, as reddit-cli and reddit-mcp-buddy do — and note that since Nov 2025 new apps need approval (RedditArchiver README), so the existing registration is an asset to protect ("do not delete it").

6. **Pace from the headers, proactively.** prawcore spreads remaining requests over the reset window with a 10 s cap and treats `remaining` as a float string; bdfr #954 shows what a wrong window assumption does, fixed by PRAW 7.8.0's `window_size` (600 s). roux has had "Improve handling of rate limiting" open since 2023 (#73) — the Rust crate does not solve this for us.

7. **Keep retries narrow and typed.** prawcore: 2 retries with jitter, only on transport errors and 408/5xx/520/522; 401 means refresh once; 429 is surfaced, not looped. reddit-user-to-sqlite detects errors by a JSON `"error"` key and returns partial data on 429 — silent truncation. Record in the cache whether a fetch was complete.

8. **Tell the agent how long to wait instead of sleeping.** reddit-mcp-buddy returns "Retry in Ns" rather than blocking past the client timeout. A CLI for an LLM agent should exit with a structured "rate-limited, retry after N" result.

9. **Expand `more` placeholders largest-first, under a budget, one request at a time.** PRAW's `replace_more(limit=32, threshold=0)` with a max-heap on `count`; `limit=0` strips placeholders for free. Its docstring warns of `TooManyRequests` under concurrent use. Store the budget used and the placeholders left unexpanded so "thread is partial" is a recorded fact (reddit-to-sqlite README admits deep chains may be missed but cannot say which).

10. **"Continue this thread" is a different fetch.** `count == 0` placeholders have no children ids; PRAW re-requests the thread rooted at the parent comment (`_continue_comments`). A port that only implements `/api/morechildren` silently loses deep chains.

11. **Insert by parent id with an id index, and make duplicates harmless.** PRAW keeps `_comments_by_id` and raises `DuplicateReplaceException` on a repeat (hit in production: bdfr #985). In SQLite, upsert keyed by fullname makes re-delivery a no-op instead of a crash; store `parent_id` and `link_id` on every comment and rebuild the tree at read time.

12. **The ~1000-item listing cap is real and unbeatable per query.** bdfr: "We cannot bypass this"; reddit-user-to-sqlite: "page 11 is an empty list"; its issue #12 proposes the only partial workaround — query multiple sorts (and time filters) and union. Expanse's answer: sync continuously and never drop locally. For search, plan on several (sort × time window) queries per question, deduped by fullname.

13. **Store the raw JSON next to parsed rows.** reddit-user-to-sqlite slims payloads and tells users to ask for missing fields (which means refetch — now impossible); RedditHarbor stores only declared columns. With the API closing, anything not saved is gone. Keep a compressed raw blob per fetch plus parsed columns; schema fixes become a local re-parse.

14. **Record `fetched_at` on every row and keep observation history for volatile fields.** reddit-user-to-sqlite #22 (no saved timestamp) and #13 (wrong scores — Reddit fuzzes them); Arctic Shift scores are 0/1 until ~36 h; RedditHarbor stores score/upvote_ratio/num_comments as timestamped histories; reddit-cli's `monitors.db` reports deltas. A citation should say "score 412 as of 2026-11-12".

15. **Never overwrite good text with `[removed]`/`[deleted]`.** reddit-user-to-sqlite computes `is_removed` from the current selftext and upserts over the old body, and drops items whose author is gone. Reveddit's whole product is the diff between two observations. Keep first-seen body, `last_seen_body_state`, and a `removed_at`/`deleted_at` transition timestamp. (Ethics counterpoint from Reagle 2023: retaining user-deleted text is a choice — fine for a private cache, not for redistribution.)

16. **Refresh with an overlap window, not just "newer than last".** reddit-to-sqlite re-walks 7 days (posts) / 1 day (comments) behind the newest saved item to catch late comments and score drift; subreddit-archiver's `update` only looks forward and documents that it misses changes to old posts.

17. **Make long jobs resumable from state stored in the database.** subreddit-archiver keeps progress in `archive_metadata` inside the output file; redarc has `status` and `progress` tables. Store cursors (`after`, time window, pending `more` ids) transactionally with the rows they produced.

18. **FTS with triggers, external-content style, in the same file.** reddit-user-to-sqlite enables FTS on `posts(title,text)` and `comments(text)` with triggers so the index cannot drift; redarc runs a second database plus an index worker on a timer and must stop workers during bulk loads. At personal scale, SQLite FTS5 with triggers (and a bulk-load mode that rebuilds once) is the simpler design.

19. **Plan schema migrations explicitly.** reddit-user-to-sqlite relies on `alter=True` auto-adding columns and has `not_null` constraints commented out — the schema is whatever the last run produced. PRAW 8.0 and prawcore both carry migration guides. Use `PRAGMA user_version` with numbered migrations and a raw-JSON re-parse path (lesson 13).

20. **Test against recorded HTTP, with secrets scrubbed at record time.** PRAW records vcrpy cassettes (Betamax before), replaces credentials with placeholders, and re-records by deleting the cassette; 100% coverage required. This matters more now: after March 2027 recorded fixtures may be the only way to test the parser at all. Capture fixtures for: a thread with nested `more`, a `count == 0` continue-thread, deleted/removed comments, a 1000-cap listing end, a 429 with headers, a 401 mid-session.

21. **Don't depend on a client library's convenience abstractions for token storage.** PRAW 8.0.0 removed token managers; bdfr #970 broke on `BaseTokenManager`. Track expiry on a monotonic clock from before the token request (prawcore), refresh on demand and once on 401.

22. **For LLM output, every chunk needs identity and context.** LlamaIndex's RedditReader emits bare comment bodies with no id, URL, author, score, or parent, top-level only; LangChain's tool omits comments and the post id. Counter-examples: last30days carries upvote counts into citations; jordanburke appends a source note to archive-derived results; buddy stamps "as of" times. Each emitted comment should carry permalink, author, score + as-of, depth, parent id, and the fetch completeness flag.

23. **Bound what goes into context and let the agent ask for more.** Hawstein defaults to 10 top-level comments at depth 3 with explicit parameters; jordanburke exposes `get_more_comments` as a separate tool; reddit-cli writes the payload to a file and prints one line. For us: `fetch` fills the cache fully; `show`/`cite` render a bounded view (top-N by score, depth D, token budget) from SQLite with no network.

24. **Select threads before fetching comments.** last30days enriches only the top 4/8/12 threads per sub-query after scoring by engagement, relevance and freshness, and de-duplicates identical requests within a run (about 4x fewer). Comment trees are the expensive part of the request budget; search results are cheap.

25. **Accept every identifier shape.** reddit-mcp-buddy takes full URLs, redd.it short links, bare ids, or id + subreddit; PRAW 7.8.0 added the mobile share-URL format. Normalize to fullname on input.

26. **Reddit search is weak; local FTS is the real search.** Arctic Shift only allows keyword search scoped to a subreddit or author; dialog-tools built a vector index of subreddits because API search is capped; LangChain/LlamaIndex get whatever `subreddit.search` returns. Use Reddit search to *discover* thread ids, then answer from the local FTS index over full comment trees.

---

## 7. What looks abandoned vs alive (quick list)

- **Archived:** snoowrap (2024-03-17), rtv, Expanse/Eternity, subreddit-archiver.
- **Dormant (no push in 18+ months):** JRAW (2021), Reddit.NET (2023-01), reddit-user-to-sqlite (2023-07), redarc (2024-02), go-reddit (2024-03), redditwarp (2024-07), libreddit (2025-02), Reddit-Archive-Host (2018).
- **Alive but route-broken:** adhikasp/mcp-reddit, Hawstein/mcp-server-reddit, bdfr (partly), redlib (cat-and-mouse), teddit (unmaintained), last30days default Reddit path (at risk).
- **Alive and on a sound route:** PRAW / prawcore / Async PRAW, roux (minimal), URS, reddit-to-sqlite, RedditHarbor, reddit-mcp-buddy, jordanburke/reddit-mcp-server, Watchful1/PushshiftDumps, Arctic Shift, shreddit.

---

## 8. Could not verify

- **Reddit's own announcement text** (r/redditdev and r/modnews posts of 2026-09-30; reported URLs reddit.com/r/redditdev/comments/1wubcvf/ and reddit.com/r/modnews/comments/1wubgvt/). reddit.com is not fetchable by the tool. Timeline in section 0 is from TechCrunch, ppc.land and two MCP server READMEs, which agree with each other.
- **Whether already-registered personal read-only OAuth script apps keep working after March 2027.** No source answers it; ppc.land says the question went unanswered by admins. This is the key open question for the project.
- **`/api/morechildren` official wording** ("one request at a time", 100-id cap). The 100-id cap is from a GitHub gist and a scraper listing, not Reddit's docs. PRAW does not chunk ids. PRAW's docstring only says `TooManyRequests` may occur under concurrent use. Treat "sequential, at most 100 ids" as a safe assumption, not a confirmed rule.
- **Search result cap of ~250** (claimed in dialog-tools README). Not confirmed elsewhere.
- **Current rate-limit numbers** (60–100 requests/min per OAuth app; RedditHarbor says 100 queries/min). Secondary sources only; read the headers.
- **Mid-2026 date for unauthenticated `.json` returning 403**: from one README (jordanburke) plus roux's undated statement.
- **PRAW's Betamax-to-vcrpy switch date**: current docs say vcrpy; the Betamax era is from prior knowledge, not re-verified.
- **LangChain `RedditPostsLoader` details**: docs URL 404'd after a redirect; the row is from prior knowledge.
- **Working state today** of Reveddit, camas, redditsearch.io, PullPush, SocialGrep, Communalytic's Reddit collector, tuir, URS, and reddit-to-sqlite: no primary confirmation; statuses for the search UIs come from vendor blogs with something to sell.
- **Eternity** as the earlier name of Expanse: from prior knowledge; the GitHub API only confirmed the jc9108 → aplotor rename and the archived flag. Archive reason not stated in the README.
- **Agent-Reach's Reddit route** (inferred as browser-session reuse) and **last30days scoring weights** (not in the docs read; ~23k characters of CONFIGURATION.md unread).
- **eliasbiondo/reddit-mcp-server** and **GeLi2001/reddit-mcp**: details only via the contextbolt review.
- **go-reddit licence**: README says MIT; GitHub API reports NOASSERTION.
- **Arctic Shift collection method and legal position**: separate document not read; repo has no licence file.
- **Star counts** are point-in-time (2026-11-12) and for last30days-skill (~67k) and Agent-Reach (~111k) are surprisingly high; reported as returned by the API, not independently checked.
- **JRAW, Reddit.NET, snoowrap internals** were not read; only metadata.
