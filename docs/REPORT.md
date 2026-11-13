# Reddit search tool: research report

Researched 2026-11-12. Six web-only research passes, whose full notes with every source and date are in [research/](research/). This report gives the findings, the recommendation and the rejected alternatives. The build plan is in [PLAN.md](PLAN.md) and the choices that are yours are in [DECISIONS.md](DECISIONS.md).

## 1. The finding that changes the brief

The brief assumed the official Reddit Data API would be the foundation. It cannot be, for long. During 2026 Reddit closed or scheduled the closure of every programmatic route to its content:

| Date | Change | Source |
|---|---|---|
| 2024-07-01 | robots.txt disallows everything; only Google keeps crawling under a licence | Simon Willison 2024-07-24; 404 Media / The Verge 2024-07 |
| 2025-11-11 | Self-service app creation ends; API access needs a request and approval (Responsible Builder Policy) | r/redditdev post, quoted by molehill.io 2026-02-11 |
| 2026-05-28 | Unauthenticated `.json` endpoints shut down; 403 within days | r/modnews post, quoted by lapcatsoftware 2026-06-12 |
| 2026-06-30 | old.reddit.com requires login | Slashdot 2026-07-01 |
| 2026-07 / 08 | Per-subreddit data dumps withdrawn from Academic Torrents, reportedly at Reddit's request | Arctic Shift maintainer, GitHub issue 81, 2026-10-14 |
| 2026-10-31 | No new public API access requests accepted | PPC Land 2026-10-04 |
| 2026-11-13 | RSS ends, with no replacement for non-moderators | TechCrunch 2026-09-30 |
| 2027-01-12 | API access starts closing for apps that have not registered | TechCrunch 2026-09-30 |
| March 2027 | Public Data API closes for everyone without a written partner agreement | TechCrunch 2026-09-30; PPC Land 2026-10-04 |

I read the TechCrunch and PPC Land articles myself and they agree on the dates. What nobody could read is Reddit's own text: every Reddit-owned site refused our fetcher, so the announcements and all terms are quoted second-hand. Many of the detailed 2026 sources are vendors who sell Reddit data and gain from saying "everything is closed"; the two press sources are the reason to believe the timeline.

Confirmed first-hand on 2026-11-12: submitting the create-application form at reddit.com/prefs/apps returns "In order to create an application or use our API you can read our full policies here", with a link to the Responsible Builder Policy. No app could be created.

What this means:

- **If you have no Reddit API app already, you cannot get one.** There is nothing to sign up for. The step "create a Reddit app and key" from the brief no longer exists.
- **If you do have one** (created at reddit.com/prefs/apps before November 2025), it reportedly still works, should be registered before 2027-01-12, and stops in March 2027. That is about four months of use.
- **After March 2027** the only routes to Reddit thread text are a commercial contract with Reddit, a third-party archive, or a person reading in a logged-in browser.
- The replacement Reddit offers, the Developer Platform (Devvit), is TypeScript that runs inside Reddit. It is not a way to read Reddit from a local tool.

## 2. Access routes, one by one

| Route | Allowed by Reddit's terms | Works today | Verdict |
|---|---|---|---|
| Official Data API with OAuth | Yes, with an approved app | Only with an existing app; ends March 2027 | Use if you already have an app, as a time-limited source |
| Unauthenticated `.json` | No; Reddit calls it a scraping surface | No, 403 | Rejected |
| RSS | Ends 2026-11-13 | Throttled to about one request a minute, then gone | Rejected |
| old.reddit or www HTML | No; terms bar automated access outside published interfaces | Login wall or blocking | Rejected |
| `.json` with a browser session cookie | No | Unknown | Rejected |
| Spoofing the official app's tokens (what redlib does) | No; it is evasion | Instances are blocked continually | Rejected |
| Web search for discovery (`site:reddit.com`) | Yes; it is the search engine's service | Yes, for URLs and snippets only | Use, through the agent's own search tool |
| Search APIs (Brave, Kagi, Tavily, Exa) | Yes | Yes; how fresh their Reddit results are is unverified | Not needed while the agent harness has search |
| SERP scrapers (SerpApi and similar), SearXNG | The conduct Reddit is suing over, or against the engines' terms | Yes | Rejected |
| Pushshift | Moderators only | Not for us | Rejected |
| PullPush | Grey; no terms | Unreachable today (403 and 526); ingest reported frozen or lagging | Rejected for now |
| Arctic Shift API | Not Reddit's service, so you break no contract; Reddit objects to archives like it | Yes, verified live, seconds behind | Your call; see below |
| Arctic Shift monthly dumps | Same grey area | Yes, through 2026-09, 70–80 GB a month | Too heavy for this tool |
| Reddit Answers | Consumer feature | No API | Rejected |
| Commercial licence | Yes | By negotiation; one developer reported a $10,000 figure | Not realistic |

### 2.1 Official Data API (only if an app already exists)

These mechanics come from the archived official wiki (2015–2016) and community notes, because current docs could not be fetched. Treat them as a starting point to test against the live API.

- **App types and flows.** A "script" app has a client id and secret. For a single-user tool that must not store the account password, use the application-only grant (`grant_type=client_credentials`): no user context, no refresh token, a new token whenever the old one expires. The authorization-code flow with `duration=permanent` and a localhost redirect is the alternative if user context is ever needed. Avoid the password grant. Scope `read` is enough. Read `expires_in` from each response; sources disagree on 1 hour versus 24.
- **Hosts.** Tokens from `https://www.reddit.com/api/v1/access_token`; API calls to `https://oauth.reddit.com`.
- **Rate limit.** 100 queries a minute per client id, averaged over 10 minutes. Headers `X-Ratelimit-Used`, `X-Ratelimit-Remaining` (a float such as `596.0`) and `X-Ratelimit-Reset` (seconds until the window ends, not a timestamp). Application-only grants reportedly get 600 per 600 seconds. Whether `Retry-After` is sent on 429 is unverified.
- **User-Agent.** `<platform>:<app id>:<version> (by /u/<username>)`. Generic agents are throttled or blocked.
- **Search.** `/search` and `/r/{sub}/search` with `q`, `sort` (relevance, hot, top, new, comments), `t` (hour to all), `restrict_sr`, `limit` up to 100, `after`. It searches post titles and bodies only. There is no comment search and no date-range search (`timestamp:` went with the old search backend around 2018). Listings stop near 1,000 items. Operators: `title:`, `selftext:`, `author:`, `subreddit:`, `flair:`, `url:`, `site:`, `self:`, capitalised `AND` `OR` `NOT`, quotes.
- **Threads.** `/comments/{id}` returns the post and a comment tree; `limit` reportedly defaults to 200 and tops out at 500, `depth` at 10. Collapsed branches arrive as `more` objects. Those with ids go to `/api/morechildren` (up to 100 ids, one request at a time or you get 429). Those with `count: 0` and id `_` ("continue this thread") need a second `/comments/{id}?comment={parent}` call.
- **Batch check.** `/api/info` takes up to 100 fullnames and silently omits ones that no longer exist. That makes it the cheap way to find out whether cached items were deleted.
- **`raw_json=1`** on every request, or bodies arrive with `&amp;` style escaping.

Terms, as quoted by secondary sources: free for non-commercial use within the limits; content deleted on Reddit must be removed from what you hold; the Data API Wiki reportedly recommends purging stored content within 48 hours as the way to meet that; no training of models on the data. **No source says whether passing fetched threads to a hosted model as context for a private answer is allowed.** The policy reportedly bars sharing Reddit data with third parties without approval, and a hosted model is a third party, so this is a grey area and a decision for you.

### 2.2 Web search as discovery

A `site:reddit.com` web search finds threads by what the comments say, which Reddit's own search cannot do. Since July 2024 only Google-backed indexes carry recent Reddit content. In this session the harness's own search returned plenty of reddit.com links, and its fetch tool refused to open any of them. So discovery is solved and free; reading the thread is the missing half, and it is the only half a tool of ours would add.

### 2.3 Arctic Shift

A volunteer-run archive (github.com/ArthurHeitmann/arctic_shift) and the only free one confirmed alive: a live call returned a post ingested five seconds after it was created.

- **What it offers.** Fetch by id (up to 500), a whole comment tree for a thread (`/api/comments/tree`, up to 25,000 comments), post and comment search with real date ranges, and comment full-text search. No account or key.
- **Limits.** Full-text search works only inside one subreddit, author or thread, and not for very busy ones; there is no Reddit-wide keyword search. Results sort by date only. Scores freeze about 36 hours after posting. Rate limits are dynamic, with `X-RateLimit-Reset` headers. No uptime promise.
- **Where it stands.** You would not be calling Reddit at all, so no Reddit contract of yours is involved, and no action against an end user of an archive was found. But the archive itself exists against Reddit's wishes, Reddit pressed a volunteer to withdraw dumps this year, and the service could disappear without notice. It also keeps posts their authors later deleted, which is a privacy matter the tool has to handle on its side (section 7).

This is the only source that both works now and will still work after March 2027. It is not a route Reddit allows; it is a route Reddit does not control. Whether to use it is your decision, not something the design should assume.

## 3. Recommendation: access strategy

**Build a smaller tool than the brief describes, and only if at least one thread source is approved.**

What a tool of ours can add is: fetch a thread's full comment tree from a URL or id, pick the informative comments, cache it locally with full-text search, and hand an agent compact text it can cite. Discovery stays with the agent's web search, which is already allowed and already works.

Sources, in the order the tool would try them:

1. **Local cache.** Always first. Instant and offline.
2. **Reddit OAuth**, only if you already have an app. Time-limited to about March 2027. Gives Reddit's own search and live scores.
3. **Arctic Shift**, only if you opt in. Gives thread trees by id, date-ranged search and in-subreddit comment search.
4. **Nothing.** The tool says "no thread source is configured or reachable", lists the URLs it could not read, and exits with a distinct code. It never falls through to a blocked route.

If you have no Reddit app and decline Arctic Shift, there is no thread source and **I recommend not building the tool**. Research agents would keep using web-search snippets for Reddit, and you would open the important threads yourself.

If either source is approved, the tool is worth building, because the cache, ranking and digest code is the bulk of the work and does not depend on which source feeds it.

No evasion anywhere: no cookies, no spoofed clients, no proxies, no retry tricks against a 403.

## 4. Search quality

**Reddit's own search is weak in known ways**: posts only, no comment text, no date ranges, about 1,000 results at most, and no documented stemming or typo handling. A September 2026 audit of Reddit's AI search found it favours early, high-score, top-level comments and under-selects first-person reports, which are what fault research most needs.

What improves on it, and how well supported each idea is:

- **Web-search discovery plus URL intake** (well supported). The agent passes the Reddit URLs it found; the tool normalises every URL shape to a thread id (`/comments/{id}`, `redd.it/{id}`, comment permalinks, old/np/m/amp hosts) and reports share links (`/s/{code}`) as unresolved rather than guessing.
- **Several queries and sorts, merged with Reciprocal Rank Fusion** (RRF, k = 60; Cormack et al. 2009). RRF needs only ranks, which suits lists from Reddit, an archive and web search that have no comparable scores. The evidence for multi-query gains is modest (MRR@5 0.855 against 0.821 in one study).
- **Local re-ranking with BM25** over the fetched threads, with the title weighted above the body and comments. Exact matches on model numbers and error codes get a fixed bonus, so the tokenizer must keep `B650E-E` and `0x80070057` whole.
- **Bounded priors** on top of text relevance: log score, log comment count, recency with a selectable half-life (fast-moving topics about 180 days, normal about 2 years, timeless none), and "solved" markers from the original poster. A 12-point thread in a small hardware subreddit can be the only right one, so votes never outrank a strong text match. Vote counts are fuzzed and, from an archive, frozen.
- **Deduplication** in order of cost: same id, cross-post parent, same outbound link, same author and title, then near-identical body text.
- **Comment selection** under a token budget: top-level comments by score with a per-branch cap; any comment matching the query's identifiers regardless of score; the original poster's follow-ups; one possible correction under each chosen comment; ancestors of everything chosen; bots, AutoModerator and deleted bodies collapsed to a count.

The weights, half-lives and selection rules are reasoned starting values, not validated on data. The plan keeps them few, visible through `--explain`, and adjustable.

**Output for an agent.** Anthropic's published guidance on tools for agents supports concise and detailed modes, truncation messages that name the next command, and source-tagged documents. Evidence on JSON versus tagged text is weak and about tables. The proposed default is plain text: one header line per comment (id, author, score, date, depth, parent), the body verbatim underneath, the permalink pattern stated once, and a closing line that accounts for everything omitted. `--json` gives the same content for programs. Budgets: about 2,000 tokens for a result list and 5,000 for a thread digest by default, with a hard cap of 20,000 (Claude Code caps tool output at 25,000). Thread text is untrusted: the envelope says it is user-written data, invisible characters are stripped, instruction-like text is flagged rather than removed, and links inside comments are never fetched.

## 5. Rust stack

The first five rows were compiled and tested on this PC (rustc 1.99.1, `x86_64-pc-windows-gnu`, WinLibs GCC 16.2) in the skeleton: gate passed, 73 crates in the tree, a 2.9 MB release binary, 50-second clean release build.

| Need | Choice | Why | Rejected |
|---|---|---|---|
| HTTP | `ureq` 3.5 with `native-tls` (Windows SChannel), `json`, `gzip` | Blocking, small, no C code for TLS | `reqwest` 0.13: pulls tokio and hyper and defaults to aws-lc-rs, which wants NASM or prebuilt objects on GNU. `hyper`: far too low-level |
| Storage and search | `rusqlite` 0.42 `bundled` (SQLite 3.54.1, FTS5 on) | One file, BM25 with column weights, stemming, snippets; builds with GCC alone | `tantivy`: more index machinery and a C zstd by default, not justified below a few million comments. `redb` plus hand-rolled BM25: more code for less |
| Models | `serde`, `serde_json` | Standard | — |
| Credentials | `windows-sys` calling `CredReadW`/`CredWriteW` directly | Verified to link; about 60 lines | `keyring` 4: now a wrapper over several crates, unverified on GNU |
| CLI | `clap` 4 derive | Subcommands and help for free | `lexopt`: lighter, but more hand-written parsing than the command list deserves |
| Async | None | The server's quota caps throughput and `morechildren` is one at a time, so async buys nothing | `tokio` |
| Rate limiting | Hand-written, about 70 lines | Reads the three headers as floats; delay rule borrowed from PRAW | `governor`, `backoff`: generic, blind to Reddit's headers |
| Reddit client | Our own | — | `roux` 2.3: maintained, but reqwest and tokio, password grant only, no `morechildren`, no rate limiting. `redlib`: AGPL and built on spoofed tokens |
| Text | Keep Reddit's markdown as written | Agents read markdown well, and verbatim text makes quotes checkable | `pulldown-cmark`, `comrak`: no crate handles Reddit's dialect, and stripping loses information |
| Later window | `tiny_http` on 127.0.0.1 | Same as the other suites | — |
| Tests | A transport trait with recorded fixtures; plain `assert_eq!` on rendered text | No test server and no async | `wiremock` (needs tokio) |

Not yet compiled here: `time` (dates), `tiny_http` and a fixture snapshot crate. Dates can be done by hand if `time` gives trouble.

**Where the GNU toolchain bites:** anything that drags in `aws-lc-sys` (reqwest's and rustls's defaults), crates needing cmake or bindgen, and `gcc` having to be on PATH for the bundled SQLite build, which `gate.ps1` handles. `rusqlite` 0.42.0 and `ureq` 3.5.2 are days old, so `Cargo.lock` should be committed.

**One thing the skeleton caught:** serde's `#[serde(other)]` cannot swallow the `data` object of an unknown `kind` in Reddit's envelope, so `Thing` needs a hand-written deserializer. Reddit adding a new kind would otherwise fail a whole listing. Other quirks the model must absorb: `edited` is `false` or a timestamp, `replies` is `""` or a listing, numeric fields can be null, and deleted authors and bodies arrive as `[deleted]` and `[removed]`.

## 6. How others built this

Full tables with repository links and dates are in [research/research-6-prior-art.md](research/research-6-prior-art.md).

- **PRAW (Python)** is the reference client and still maintained. Worth copying: its rate-limit delay rule (spread the remaining requests over the remaining window, never sleep more than 10 seconds, surface a 429 instead of retrying), its `replace_more` order (largest collapsed branch first, under a request budget), and its recorded-cassette tests with credentials scrubbed.
- **reddit-user-to-sqlite and reddit-to-sqlite** show a workable SQLite schema with FTS and upserts by id. Their mistakes: no `fetched_at`, raw JSON thrown away, and items by deleted authors dropped.
- **Tools built on unauthenticated `.json`** (libreddit, several MCP servers) broke in 2026. **Tools built on Pushshift** (timesearch, 4CAT's Reddit source) broke in 2023. **redlib**, built on spoofed app tokens, spends its issue tracker on blocks. The lesson is the same each time: a tool tied to one route dies with it.
- **Agent-facing tools** mostly do no query expansion, fusion, re-ranking or deduplication. reddit-mcp-buddy cuts bodies at 500 characters with no marker and silently drops collapsed branches. LlamaIndex's reader emits bare comment text with no author, score or date. Those are the failures to avoid.
- **Abandoned or archived:** snoowrap, rtv, JRAW, Reddit.NET, go-reddit, redditwarp, redarc, Expanse.

## 7. Risks and limits

- **Access can end.** The Reddit source has an announced end. Arctic Shift has no guarantee and is under visible pressure. The cache keeps working whatever happens, which is the main reason to build cache-first. Recorded fixtures may become the only way to test the parsers.
- **Account risk.** With an existing app, the realistic risk is the app or account being blocked, not legal action. Reddit's lawsuits have targeted large commercial collectors that kept going after notice and used evasion.
- **What you would need to do yourself.** Open reddit.com/prefs/apps in your own browser and see whether an app is listed. If one is, register it at developers.reddit.com/app-registration and type its client id and secret into the tool's prompt. I will not create, register or sign in to anything. If no app is listed, there is no step to take.
- **Privacy of the cache.** It holds other people's usernames and words. Planned handling: stored only on this PC, no command that collects one user's posts across threads, deleted and removed bodies blanked when the tool learns of them, and a `purge` command.
- **Retention.** For content from the Reddit API the terms require removing what Reddit users delete, and Reddit's guidance reportedly says 48 hours. That conflicts with an offline cache that lasts. The plan offers a strict mode and a re-check mode; see DECISIONS.
- **Archive content.** Arctic Shift returns posts whose authors later deleted them. The tool should not present that as a feature: where an item's state is known to be deleted or removed, the body is not shown.
- **Stale answers.** A frozen archive still answers with results. Every result carries its source and fetch time, and `rdt sources` checks the newest item a source returns before trusting it.
- **Sending threads to a hosted model.** Unaddressed by any source found, and plausibly within what Reddit's policy restricts for API-sourced data. Your decision.
- **Unverified ground.** Reddit's own announcements and terms, most endpoint limits, and Arctic Shift's exact rate-limit headers were not read first-hand. Slice 6 or 7 of the plan starts with a handful of live calls to confirm them before anything is built on top.

## 8. Alternatives rejected

- **Build on the official API as the foundation**: closed to new users, ends March 2027.
- **Scrape HTML, `.json` or RSS, or reuse a browser session**: forbidden and blocked.
- **Adopt redlib's approach**: evasion.
- **Run a local copy of the dumps**: 70–80 GB a month, the convenient per-subreddit files were withdrawn, and it deepens the grey area.
- **Pay for a search or Reddit-data API**: discovery is already free through the agent, and the vendors reselling Reddit data are the companies Reddit is suing or their peers.
- **Drive your logged-in browser with an agent to read threads**: automated access outside Reddit's published interfaces, on your own account. Not recommended.
- **Do nothing**: the right answer if neither thread source is approved.
