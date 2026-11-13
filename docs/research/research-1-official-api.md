# Reddit official Data API: state as of 2026-11-12 (personal, read-only research CLI)

Research method: WebSearch + WebFetch only, on 2026-11-12.

## 0. Read this first

**Access limits of this research.** WebFetch was refused for every Reddit-owned host: `redditinc.com`, `www.reddit.com`, `developers.reddit.com` (tool-level block), `support.reddithelp.com` (HTTP 403), and `web.archive.org` (tool-level block). Redlib mirrors failed too (TLS errors, 403, DNS). So **no Reddit policy page, help article, r/redditdev post or reddit.com/dev/api page was read directly.** Everything about current policy comes from (a) search-engine snippets of the official pages, (b) press (TechCrunch, PPC Land), (c) third-party blogs, many of them vendors selling Reddit-data alternatives with an interest in saying "the official API is closed". The only primary sources read in full were the archived GitHub wiki pages (2015-2016) and community API notes.

Confidence labels used below:
- **[P]** primary source read directly
- **[S]** official page seen only as a search snippet, or quoted by a secondary source
- **[N]** news press read directly
- **[V]** third-party / vendor blog
- **[K]** background knowledge of the API, not re-verified today

**The headline finding (changes the whole plan):**

| Date | Event | Source |
|---|---|---|
| 2025-11-11 | Responsible Builder Policy published; self-service Data API access closed; new access needs request + approval | [V] quoting the r/redditdev admin post https://www.reddit.com/r/redditdev/comments/1oug31u/introducing_the_responsible_builder_policy_new/ via https://www.redditapis.com/blogs/reddit-data-api-2026 (2026-06-12) and https://molehill.io/blog/reddit_killed_self-service_api_keys_your_options_for_automated_reddit_integration (2026-02-11) |
| 2026-08-05 | Reddit announces it will gradually restrict all new Data API requests; third-party apps to port to the Developer Platform (Devvit); asks owners to register apps by 2026-09-30 | [V] https://www.redditapis.com/reddit-responsible-builder-policy (checked by that site 2026-08-31), citing https://www.reddit.com/r/redditdev/comments/1vgbm9c/our_plans_for_the_future_of_reddits_public_data/ ; https://creuto.com/reddit-data-api-registration-developer-platform-2026 (2026-09-18) |
| 2026-09-30 | Reddit posts timeline: RSS ends 2026-11-13; public API ends March 2027; register by 2027-01-12 | [N] https://techcrunch.com/2026/09/30/reddit-is-killing-rss-feeds-ending-public-api-access-because-of-ai-bots/ (2026-09-30), which links https://www.reddit.com/r/redditdev/comments/1wubcvf/moving_data_api_apps_to_the_developer_platform/ |
| **2026-10-31** | **New requests for public API access no longer accepted** | [N] https://ppc.land/reddit-to-halt-public-data-api-access-in-march-2027-as-14-000-apps-register/ (2026-10-04); [V] https://www.redditapis.com/blogs/reddit-data-api-october-31-cutoff-2026 (2026-10-19, updated 2026-11-05) quoting the 2026-09-30 admin post |
| 2026-11-13 | RSS feeds stop | [N] TechCrunch 2026-09-30 |
| 2027-01-12 | Access starts closing for apps/users that have not registered | [N] TechCrunch 2026-09-30; PPC Land 2026-10-04 |
| March 2027 (no day) | Public API access closes for all remaining apps and users | [N] TechCrunch 2026-09-30; PPC Land 2026-10-04 |

Consequence: **as of 2026-11-12 a person with no existing approved credentials apparently cannot obtain new official Data API access at all**, and anyone who already has working credentials has (per the announced timeline) until about March 2027, provided the app is registered. This rests on press + vendor reporting of Reddit's posts; the posts themselves could not be read.

---

## 1. App types and OAuth flows

Source for the mechanics: archived official wiki https://github.com/reddit-archive/reddit/wiki/OAuth2 **[P]** (last edited 2016-01-08; repo archived 2017-11-09, fetched 2026-11-12). No newer official OAuth reference could be read. The mechanics below were still how the API worked in 2025 **[K]**, but are unverified for today.

### App types
- **script**: runs on hardware you control, can keep a secret, only has access to the developer's own account. Gets client id + secret. [P]
- **installed app**: runs on devices you do not control, no secret issued. Only type allowed to use the implicit flow. [P]
- **web app**: runs on a server you control, can keep a secret. [P]

### Flows
| Flow | Who | Notes |
|---|---|---|
| Password grant (`grant_type=password`, username + password, Basic auth with id:secret) | script apps only | Wiki mentions it only in passing [P]. Requires the account password at every token fetch (tokens are not refreshable), so the CLI would have to store or prompt for the password. With 2FA the password field is `password:otp` [K]. |
| Application-only, `grant_type=client_credentials` | confidential clients (script, web) | No user context, no refresh token; just re-request when expired. [P] |
| Application-only, `grant_type=https://oauth.reddit.com/grants/installed_client` + `device_id` | installed apps / logged-out users | `device_id` 20-30 ASCII chars, unique per device; `DO_NOT_TRACK_THIS_DEVICE` allowed. [P] |
| Authorization code (`response_type=code`) | web, also installed/script | Authorize at `https://www.reddit.com/api/v1/authorize` with `client_id, response_type, state, redirect_uri, duration, scope`; exchange at `POST https://www.reddit.com/api/v1/access_token` with Basic auth (installed apps: empty password). `redirect_uri` must exactly match the registered one. [P] The wiki does not mention localhost; `http://localhost:<port>` redirect URIs were accepted in practice (PRAW's documented pattern) [K]. |
| Implicit (`response_type=token`) | installed apps only | No refresh token. [P] |

### Tokens
- Access tokens: wiki says 1 hour. [P] The wiki is internally inconsistent about `expires_in`. Reports since 2023 of `expires_in: 86400` (24 h) exist [K, unverified] - **always read `expires_in` from the response, never hard-code.**
- Refresh tokens: issued only with `duration=permanent` on the code flow; refresh with `grant_type=refresh_token`. App-only tokens never get one. [P] Whether refresh tokens now expire or rotate: not verified.
- Revoke: `POST https://www.reddit.com/api/v1/revoke_token`. [P]
- Scopes: `read` covers listings, search, comments, subreddit about; `identity` only for `/api/v1/me`. Scope list in the wiki: identity, edit, flair, history, modconfig, modflair, modlog, modposts, modwiki, mysubreddits, privatemessages, read, report, save, submit, subscribe, vote, wikiedit, wikiread. [P]
- Hosts: authorize/token on `www.reddit.com`; API calls on `https://oauth.reddit.com` with `Authorization: bearer TOKEN`. [P]

### Best flow for a single-user local CLI that must not store the password
1. **Application-only `client_credentials`** with an existing script app (store only client id + secret; no account password, no user context). Enough for search, comments, info, subreddit about on public content. Simplest. Community client notes say app-only grants get a lower limit (600 per 600 s vs 996 per 600 s): https://redditwarp.readthedocs.io/en/latest/user-guide/rate-limits.html [V, undated, fetched 2026-11-12].
2. **Authorization code + `duration=permanent` + localhost redirect** if user context is wanted (NSFW per account prefs, subscribed/private subs). Store the refresh token, never the password.
3. Avoid the password grant.

All of this is moot without credentials that Reddit still honours - see section 2.

---

## 2. Getting access today

### Sequence of changes
1. **Until 2025-11-10**: self-service at `reddit.com/prefs/apps`. [K]
2. **2025-11-11**: admin post "Introducing the Responsible Builder Policy"; self-service closed that day; developers, researchers and moderators must request approval. Existing access stated as unaffected. [V: redditapis.com 2026-06-12; molehill.io 2026-02-11; GitHub issue https://github.com/karanb192/reddit-mcp-buddy/issues/34 opened 2025-11-24]
3. Responsible Builder Policy (RBP) text, as quoted by a secondary site: "You must request access and get explicit approval before accessing any Reddit data" - https://support.reddithelp.com/hc/en-us/articles/42728983564564-Responsible-Builder-Policy [S, via https://prowlo.com/blog/reddit-data-api 2026-09-15; that site dates the RBP "updated June 5, 2026"; another dates it "updated October 5, 2026" (https://imisofts.com/blog/reddit-api-pricing/ 2026-10-21)].
4. Request route while it was open: support ticket form `https://support.reddithelp.com/hc/en-us/requests/new?ticket_form_id=14868593862164`; describe use case, data needed, subreddits, expected volume; lanes for developer / researcher / moderator, separate enterprise form for commercial. Reddit's stated target was about seven days. [V: molehill.io 2026-02-11; redditapis.com 2026-06-12]. Form fields not verified.
5. `prefs/apps` after Nov 2025: users reported an error beginning "In order to create an application or use our API you can read our full policies here" and a human-verification control that reset on submit. [V: molehill.io 2026-02-11; redditapis.com checked 2026-08-31]
6. **2026-08-05**: app owners asked to register at `https://developers.reddit.com/app-registration` by 2026-09-30 "to stay in good standing"; registering does not commit to migrating. [V; registration URL also linked by TechCrunch 2026-09-30 [N]]. What the registration form asks and what one agrees to: **not verified**.
7. **2026-10-31**: new requests no longer accepted. No exempt category named; whether researcher requests stay open is unknown. [N: PPC Land 2026-10-04; V: redditapis.com 2026-11-05]
8. 2027-01-12 unregistered access begins closing; March 2027 everything remaining closes. Partners with written agreements (accessibility apps, some mobile clients) may keep access. [N: PPC Land 2026-10-04]
9. The replacement, the Developer Platform (Devvit), is TypeScript code hosted by Reddit inside Reddit; Python not on the roadmap. It is not a general external read API for a local CLI. [N: PPC Land 2026-10-04; TechCrunch 2026-09-30]

### What a person would have to do today
- **No existing app/credentials**: no official path found. The request form is reported closed since 2026-10-31. Only a commercial/enterprise agreement remains (TechCrunch: tools "will need commercial deals"). Not verified whether the ticket form still physically accepts submissions.
- **Existing app created before 2025-11-11 (or approved since)**: reported to keep working at current limits; should be registered at developers.reddit.com/app-registration (the stated deadline was 2026-09-30; hard cut for unregistered begins 2027-01-12); expected to stop in March 2027. Whether a personal script app that registers late is accepted: not verified.
- Agreeing to: Developer Terms, Data API Terms, RBP, User Agreement (by using the API). Exact click-through wording not verified.

### Reports of personal-use denials / delays (all second-hand)
- Generic rejection text "Submission is not in compliance with RBP and/or lacks necessary details"; a personal analytics tool rejected and told to rebuild in Devvit (thread https://www.reddit.com/r/redditdev/comments/1p5ks3s/denied_api_access_for_simple_analytics_app_help/). [V: molehill.io 2026-02-11]
- Academic researcher rejection thread https://www.reddit.com/r/redditdev/comments/1qq614s/reddit_api_access_request_for_academic_researchers/ [V: molehill.io]
- Community member (not an admin) u/nikthefurry: nobody is getting access "for small personal use" - https://www.reddit.com/r/redditdev/comments/1oxazn8/need_api_access/ [V: redditapis.com 2026-06-12]
- August 2026: eight weeks without response on one request; an approved researcher unable to create a script app. [V: redditapis.com, checked 2026-08-31]
- GitHub issue 34 above: requester filed 2025-11-24, no outcome ever reported; maintainer wrote on 2026-10-25 that creating an app "does not guarantee access". [P for the issue page]
- No report of a personal read-only request being approved was found. Absence of evidence only; the sources are biased.

---

## 3. Rate limits

- **100 QPM per OAuth client id, free tier**: reported by many secondary sources as the Data API Wiki's wording (https://support.reddithelp.com/hc/en-us/articles/16160319875092-Reddit-Data-API-Wiki, said to be last updated 2026-05-11). [S/V: prowlo.com 2026-09-15; imisofts.com 2026-10-21]. The 100 figure was not seen in the official snippet itself.
- **Averaging window**: official wiki snippet: limits are "an average over a time window (currently 10 minutes)". [S, search snippet of the Data API Wiki, fetched 2026-11-12]. So effectively up to ~1000 requests per 600 s, bursts allowed. Community client notes observe 996 per 600 s for user grants and 600 per 600 s for application-only grants (https://redditwarp.readthedocs.io/en/latest/user-guide/rate-limits.html, undated) [V].
- **Conflict**: the archived GitHub wiki says 60 requests/minute for OAuth clients (https://github.com/reddit-archive/reddit/wiki/API, last edited 2015-12-18) [P]. Outdated; superseded by the 2023 numbers.
- **Unauthenticated: 10 QPM** from the 2023 r/redditdev announcement (https://www.reddit.com/r/redditdev/comments/14nbw6g/updated_rate_limits_going_into_effect_over_the/, 2023-06-30) [S]. Status today conflicts: one search summary claimed unauthenticated calls were blocked since 2026-05-30 (unverified, source not identified); redditapis.com (2026-06-12) says `.json` is heavily throttled but still up; a GitHub commenter (2026-06-01) said anonymous mode "does not work as well". Treat unauthenticated access as unreliable and against the RBP's approval requirement.
- **Headers** (official wording, same in the archived wiki [P] and the current Data API Wiki snippet [S]):
  - `X-Ratelimit-Used`: approximate number of requests used in this period
  - `X-Ratelimit-Remaining`: approximate number of requests left (can be a float, e.g. `596.0`)
  - `X-Ratelimit-Reset`: approximate number of **seconds until** the period ends (a duration, not a Unix timestamp)
  - Over the limit: remaining stays 0 while used keeps climbing (redditwarp notes [V]).
- **429**: returned when over the global limit, on concurrent `/api/morechildren` calls, and for user agents containing "curl" (redditwarp notes [V]). Whether Reddit sends `Retry-After` on 429: **not verified**; one vendor says yes without a Reddit citation. Design: on 429 sleep `X-Ratelimit-Reset` seconds if present, else `Retry-After` if present, else exponential backoff.
- Accounting is per client id (official wiki snippet [S]); do not run several processes on one client id.

---

## 4. User-Agent

- Required format (archived wiki [P], repeated in current Data API Wiki snippet [S]): `<platform>:<app ID>:<version string> (by /u/<reddit username>)`, e.g. `android:com.example.myredditapp:v1.2.3 (by /u/kemitche)`.
- Default UAs ("Python/urllib", "Java") are "severely restricted"; never spoof browsers or other bots (ban risk). [P, 2015-12-18]
- Current wiki snippet: Reddit can "freely throttle or block unidentified Data API users". [S]
- Observed: UA containing `curl` gets 429 (redditwarp notes [V]).
- Note: the UA must carry the Reddit username of the developer; that is the account that owns the app.

---

## 5. Endpoints

`https://www.reddit.com/dev/api` could not be fetched. Sources: community notes https://github.com/Pyprohly/reddit-api-doc-notes (files `comment_tree.rst`, `listings.rst`, `subreddit.rst`, fetched 2026-11-12, undated) [V]; PRAW docs https://praw.readthedocs.io/en/stable/ (fetched 2026-11-12, undated) [V]; archived wiki https://github.com/reddit-archive/reddit/wiki/JSON (2016-10-08) [P]; https://redreplier.com/blog/reddit-search-operators (undated) [V]; remainder [K].

### Hosts
- With a token: `https://oauth.reddit.com/...` (no `.json` suffix needed). [P]
- `https://www.reddit.com/...json` is the unauthenticated surface; do not use it with bearer tokens. [K]

### Listings (common)
- `limit` default 25, max 100 (larger values clamped). [V Pyprohly]
- `after` / `before` are fullnames (`t3_xxx`); give only one; `count` = items already seen (optional); `after: null` means last page. [V Pyprohly; P JSON wiki]
- ~1000-item cap per listing (about 10 pages of 100), search included. [K - not found in any fetched source today]
- `show=all`, `sr_detail`. [V]

### GET /search and /r/{sub}/search (scope `read`)
- `q` (max 512 chars), `sort` = relevance | hot | top | new | comments, `t` = hour | day | week | month | year | all, `restrict_sr` (bool; needed to stay inside the sub on `/r/{sub}/search`), `type` = comma list of `sr,link,user`, `limit`, `after`, `before`, `count`, `category`, `include_facets`. [V redreplier quoting dev/api; K]
- `include_over_18` (on/off): [K], not confirmed in a fetched source.
- `syntax` param: PRAW still exposes `cloudsearch | lucene | plain`, default lucene (PRAW docs [V]); the current official parameter list reportedly has no `syntax` parameter (redreplier [V]). Conflict; assume only the default (lucene-like) parser works.
- Documented field filters (Reddit help article "Available search features", as reported): `author:`, `flair:`, `self:` (true/false), `selftext:`, `site:`, `subreddit:`, `title:`, `url:`. No space after the colon; quote multi-word values, e.g. `title:"kitten gif"`. [V redreplier]
- Boolean `AND`, `OR`, `NOT` in capitals, parentheses for grouping, double quotes for exact phrase. Lowercase `and` is a plain word. [V redreplier]
- Not working / undocumented: `timestamp:` range search and the rest of the old cloudsearch syntax (`ups`, `over18`, `is_self`, `distinguished`), retired with cloudsearch (about 2017-2018 [K]); `nsfw:` circulates but is undocumented; `-term` negation undocumented. [V redreplier]. There is no date-range search in the official API; only `t=` buckets.
- Field filters apply to posts; comment search via the official Data API `/search` is not an offered `type` value. [V/K]

### GET /comments/{article} (also /r/{sub}/comments/{id}, /comments/{id}/_/{comment_id}) (scope `read`)
From Pyprohly notes [V]:
- `comment` (ID36 to focus as root), `context` 0-8 (parents shown when `comment` set), `depth` default 10 / max 10 (1 = top level only), `limit` default "seems to be" 200, max "appears to be" 500, `truncate` 0-50, `sort` = confidence | top | new | controversial | old | random | qa | live, plus `showedits`, `showmore`, `threaded`, `sr_detail`.
- Response: array of two listings: [0] the submission, [1] the comment tree. `replies` is a Listing or `""`. A `more` object, if present, is last among siblings.
- Conflict: one vendor (redditapis.com comments guide, 2026) claims `limit`/`depth` no longer change output on its endpoint. Unverified; test.

### `more` objects
- "Load more comments": `count` > 0, `children` = some ID36s; resolve through `/api/morechildren`. [V]
- **"Continue this thread"**: `count: 0`, `children: []`, `id: "_"`, `name: "t1__"`, with a `parent_id`. Resolve by calling `GET /comments/{article}?comment={parent_id without t1_}` (or `/comments/{article}/_/{id}`), not morechildren. [V Pyprohly; PRAW docs confirm such links "appear to have 0 children"]

### /api/morechildren (GET or POST) (scope `read`)
- `link_id` (fullname `t3_...`, required), `children` (comma list of ID36), `sort`, `limit_children` (bool; only the requested ids, no descendants), `depth`, `id`, `api_type=json`. [V Pyprohly; `api_type` K]
- Max ids per call: 100 [gist https://gist.github.com/davestevens/4257bbfc82b1e59eeec7085e66314215 V; PRAW: each replacement yields at most 100 comments]. Too many ids gives HTTP 400. [V]
- Response is a flat list (`json.data.things`), pre-order; `replies` always empty, so rebuild the tree via `parent_id`; may contain further `more` objects. [V]
- **Only one request at a time** per client; concurrent calls get 429. [V Pyprohly, redditwarp, PRAW]

### GET /api/info (scope `read`)
- `id` = comma list of fullnames (t1_, t3_, t5_), up to 100; or `url`; or `sr_name`. Returns a Listing; silently omits ids that do not exist. Works for non-public subreddits' basic info. [K; Pyprohly mentions the last point]

### GET /by_id/{names}
- Comma list of `t3_` fullnames only (links); up to 100. [K]

### GET /r/{sub}/about
- Returns `t5` object. Private: 403 `reason: private`; banned: 404 `reason: banned`; nonexistent: redirect/empty listing. [V Pyprohly]

### GET /subreddits/search
- `q`, listing params; `sort` (relevance | activity) and `show_users` appear to do nothing; matches name and public description. Also `GET /api/search_reddit_names?query=` (prefix match, max 50 chars, `exact`, `include_over_18`). [V Pyprohly]

### raw_json=1
- Without it, JSON string values have `<`, `>`, `&` replaced with `&lt;` `&gt;` `&amp;` (legacy behaviour), and `body_html`/`selftext_html` are entity-escaped HTML. [P JSON wiki 2016 for the escaping]
- Adding `raw_json=1` to every request turns the legacy escaping off. [K - documented on reddit.com/dev/api, not fetched today]. `body_html` still needs an HTML parser; prefer the markdown `body` / `selftext` fields.

---

## 6. Terms position for personal research use

None of the terms pages could be read. Clauses below are as quoted by secondary sources; section numbers and wording need checking in a real browser.

**Data API Terms** - https://redditinc.com/policies/data-api-terms (effective 2023-06-19; "Last Revised July 20, 2026" per https://www.redditapis.com/blogs/reddit-api-commercial-use-licensing-explained-2026, 2026-10-13, and https://vorplabs.com/agent-tools/reddit-data-api, 2026-07-27) [S]
- Fees: Reddit "reserves the right to charge fees for future use" (s. 3.1 as cited).
- Commercial use, research beyond the rate limits, or anything not expressly permitted needs a separate agreement (s. 3.1, paraphrase).
- No selling/sublicensing: "sell, lease, or sublicense the Data APIs or access thereto" (s. 3.2 as cited).
- AI training: no right to use User Content for "training a machine learning or AI model, without the express permission of rightsholders" (s. 2.4 as cited since 2023; https://redlib.hbubli.cc/r/MachineLearning/comments/12r7qi7/ 2023-04-18 via snippet).
- Retention: must not keep data beyond the approved use case and must delete what is not needed (s. 3.2, paraphrase only).
- Revocation "for any reason" and "without notice" (s. 6 as cited).

**Data API Wiki** - https://support.reddithelp.com/hc/en-us/articles/16160319875092-Reddit-Data-API-Wiki (reported last updated 2026-05-11) [S]
- Deleted content: you "must remove any user content ... that has been deleted from Reddit" (search snippet).
- Keeping deleted content "even if disassociated, de-identified or anonymized" is a violation (search snippet).
- **48-hour rule**: quoted by prowlo.com (2026-09-15) as "routinely deleting any stored user data and content within 48 hours". So the 48 h figure appears to be a recommended practice in the Wiki for complying with the deletion duty, not a Data API Terms clause. Seen only via one secondary quote; a second search did not surface it.

**Responsible Builder Policy** - https://support.reddithelp.com/hc/en-us/articles/42728983564564-Responsible-Builder-Policy (2025-11-11; reported updates 2026-06-05 and 2026-10-05) [S]
- Approval needed before any API data access, including non-commercial (quote in section 2).
- Commercial use: "you'll need to get explicit written approval" (prowlo.com quote).
- Covers "bots, AI agents, or non-human operated accounts" (https://reduck.ai/use-cases/reddit-bot-rules-ai-agents, updated 2026-10-29).
- AI training and selling/licensing data need "express written approval" (same source, paraphrase around a two-word quote).
- Reported to also bar sharing Reddit data without approval and circumventing limits (paraphrase, unverified).

**Developer Platform FAQ** - support.reddithelp.com article 14945211791892 [S via redditapis.com 2026-10-13]
- "You may not use content on Reddit as an input for any model training" (without explicit consent).
- Showing Reddit content alongside ads counts as commercial.

**Developer Terms** - https://redditinc.com/policies/developer-terms : no text obtained. Axios (2024-05-09, via snippet) says non-commercial researchers may continue if they agree not to release sensitive data or products built on Reddit content. Not verified.

**Public Content Policy** - https://redditinc.com/policies/public-content-policy (2024-05-09) [N via snippets: TechCrunch 2024-05-09 https://techcrunch.com/2024/05/09/reddit-locks-down-its-public-data-in-new-content-policy-says-use-now-requires-a-contract ; Axios 2024-05-09]
- Reddit said it wants to "preserve public access to Reddit content for researchers" and non-commercial users; commercial use including AI training requires a contract; partners must honour deletions. The 2025-2026 changes narrowed the practical route for that access.

**User Agreement** - https://redditinc.com/policies/user-agreement ("Last Revised May 26, 2026" per vorplabs.com 2026-07-27) [S]: crawling per robots.txt permitted, scraping without prior written consent prohibited (paraphrase). Reddit's Rule 8 ("Don't break the site", reported updated 2026-10-06) lists unauthorised scraping as a violation (imisofts.com 2026-10-21) [V].

### Answers to the specific questions
- **Is non-commercial use free?** Yes in price: no fee within the free limits. But since 2025-11-11 free does not mean open: approval required, and new approvals reportedly stopped 2026-10-31.
- **Caching/retention**: keep only what the use needs; remove anything deleted on Reddit; the Wiki's reported guidance is to purge stored content within 48 hours. For the CLI: short-lived cache (under 48 h), or re-validate via `/api/info` before reuse; no permanent archive of thread bodies.
- **Training ML/AI**: prohibited without permission (Data API Terms, RBP, FAQ all agree per secondary quotes).
- **Feeding fetched threads to an LLM as context for a private answer (inference, not training)**: **no source found that addresses it explicitly.** vorplabs.com and a search summary both note prompting/inference is not spelled out. Risk points: (a) the RBP reportedly bars sharing Reddit data with third parties without approval, and sending thread text to a hosted LLM API is a transfer to a third party; (b) the provider must not train on it; (c) TechCrunch (2026-09-30) lists "AI assistants" among tools that will need commercial deals. A 2024 r/redditdev thread on LLM usage exists (https://lr.ggtyler.dev/r/redditdev/comments/1ehgp6f/question_on_reddit_data_usage_with_llms, 2024-08-01) but was not readable. Treat as a grey area, not as permitted.

---

## 7. Commercial pricing (context)

- **$0.24 per 1,000 calls** originates from 2023 press: Apollo's developer was quoted $12,000 per 50 million requests (The Verge 2023-05-31 https://www.theverge.com/2023/5/31/23743993/reddit-apollo-client-api-cost ; MacRumors 2023-05-31). [N, via secondary citations]
- It is **not on any current Reddit page** per several 2026 write-ups; there is no public rate card; commercial access is by negotiated contract, and Reddit's terms say fees are at its discretion (imisofts.com 2026-10-21; redditapis.com 2026-10-13; prowlo.com 2026-09-15). [V]
- "Threshold": no published call-count threshold. The dividing line is use type (commercial, or needing more than the free limits), not volume. Whether $0.24/1K is still what Reddit charges: unknown.

---

## 8. Could not verify (primary source unreachable)

1. The actual text and dates of: RBP, Data API Terms, Developer Terms, User Agreement, Public Content Policy, Data API Wiki, Developer Platform FAQ. All quotes are second-hand.
2. The three r/redditdev admin posts (2025-11-11, 2026-08-05, 2026-09-30) - read only through press and vendor quotes. In particular the exact scope of "new requests ... no longer accepted" from 2026-10-31 and whether personal script apps are covered by the registration scheme.
3. What `reddit.com/prefs/apps` shows today and whether the request form still takes submissions.
4. What `developers.reddit.com/app-registration` asks for and what is agreed to; whether registration is still possible after 2026-09-30.
5. The "100 QPM per OAuth client id" figure in Reddit's own words (only the 10-minute averaging sentence and header wording were seen as official snippets).
6. Status of unauthenticated access (10 QPM vs blocked since 2026-05-30).
7. `Retry-After` on 429.
8. Current token lifetime (3600 vs 86400), refresh-token expiry, localhost redirect rules, password-grant availability today.
9. reddit.com/dev/api parameter lists: `include_over_18`, `syntax`, `/api/info` and `/by_id` 100-id limits, the 1000-item listing cap, `raw_json=1` wording, `/comments` limit default 200 / max 500 (community notes hedge these).
10. The exact search-operator list in the help article "Available search features".
11. Any explicit Reddit statement on LLM inference / RAG use of API data.
12. Current commercial price.

## 9. Conflicts between sources

- OAuth limit: 60/min (archived wiki 2015) vs 100 QPM averaged over 10 min (2023 onward). Use the latter.
- Closure date: Wikipedia's "Reddit API controversy" article says closure "in November 2026" (confuses the RSS end, 2026-11-13); TechCrunch/PPC Land say March 2027.
- RBP last-updated date: 2026-06-05 vs 2026-10-05 (probably both true at different times).
- Registration deadline: 2026-09-30 ("good standing") vs 2027-01-12 (access removal begins). Likely soft vs hard deadline.
- heise (2026-10-01, quoted second-hand) "registered API users are not affected" vs Reddit's timeline ending all remaining public access in March 2027.
- `syntax` search parameter: PRAW exposes it; reported absent from current official docs.
- `X-Ratelimit-Reset`: official wording says seconds remaining; one vendor claims it may be a Unix timestamp "depending on API version" (unsupported; PRAW converts it to a timestamp itself).
- `/comments` `limit`/`depth`: community notes say they work; one vendor claims no effect.
- Most 2026 sources are vendors selling Reddit-data products; they have an incentive to overstate closure. The two press sources (TechCrunch, PPC Land) agree with them on the dates, which is the main reason to believe the timeline.

## 10. Bottom line for the project

- Building a new tool on the official Data API in November 2026 is only viable if working OAuth credentials already exist on the person's account (an app listed at reddit.com/prefs/apps from before 2025-11-11, or an approved request). First action: have the person look at that page and at developers.reddit.com/app-registration in their own browser.
- Even then the announced end is March 2027, so the Reddit backend should sit behind an interface that can be swapped.
- If credentials exist: `client_credentials` app-only flow, descriptive UA, single client id, header-driven throttle (stay under about 60/min sustained), serial morechildren, `raw_json=1`, cache under 48 h, no training use, and a deliberate decision on the LLM-context question.
