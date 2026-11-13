# Research 2: non-API ("unofficial") routes to Reddit content

Researched 2026-11-12. Web only (WebSearch + WebFetch). No evasion considered.

## Headline

Between May and November 2026 Reddit closed, or announced the closure of, every unauthenticated machine-readable route. The older picture (".json works at ~10 req/min, RSS is fine") is out of date.

| Date | Change | Source |
|---|---|---|
| 2024-07-01 | robots.txt becomes `User-agent: * / Disallow: /`; non-Google search engines stop crawling | see Q3, Q4 |
| 2026-05-28 | r/modnews "Protecting communities from scrapers and platform abuse": unauthenticated `.json` to be shut down; 403s within days | see Q1 |
| 2026-06-11 | Logged-out RSS throttled to about 1 request/minute (unannounced) | see Q2 |
| 2026-06-30 / 07-01 | Login required for old.reddit.com, rolled out over about a month | see Q3 |
| 2026-09-30 | r/modnews + r/redditdev: RSS ends 2026-11-13; public Data API winds down | see Q2, Q5 |
| 2026-10-29 | CEO post: reddit.com to become "a primarily logged-in experience", tests outside the US first | see Q3 |
| 2026-10-31 | No new public Data API access requests accepted | see Q5 |
| **2026-11-13** | **RSS support ends (the day after this research)** | see Q2 |
| 2027-01-12 | API access starts closing for unregistered apps/users | see Q5 |
| 2027-03 (no day given) | Remaining public Data API access ends | see Q5 |

### Data point: what our own fetches got (2026-11-12)

- `https://www.reddit.com/robots.txt`, `/r/redditdev/search.json?...`, `/r/redditdev/.rss`: the WebFetch tool refused with "Claude Code is unable to fetch from www.reddit.com" (refusal at the tool/harness level, no HTTP status returned, so it does not tell us what Reddit itself would answer).
- `https://redditinc.com/policies/user-agreement`: same refusal ("unable to fetch from redditinc.com").
- `https://support.reddithelp.com/hc/en-us/articles/26410290525844-Public-Content-Policy`: HTTP 403 Forbidden.
- `web.archive.org` copies of the User Agreement and robots.txt: "unable to fetch from web.archive.org".
- Two Redlib mirrors of the r/modnews posts: DNS failure / TLS error.

Consequence: **no Reddit primary source could be read directly in this session.** Every Reddit quote below is second-hand (news articles and blogs quoting the posts). The post URLs are given so they can be checked in a browser.

---

## Q1. Public `.json` endpoints without OAuth

**Status: officially shut down for unauthenticated use. Does not work, and is not permitted.**

- Official statement. r/modnews post "Protecting communities from scrapers and platform abuse", dated 2026-05-28, URL `https://www.reddit.com/r/modnews/comments/1tq9vxo/protecting_communities_from_scrapers_and_platform/`. Quoted by secondary sources as: "Deprecating unauthenticated JSON access: We'll also be shutting down unauthenticated `.json` endpoints." / "These endpoints can be used to scrape Reddit without accountability." / "Logged-in and authenticated access won't be impacted."
  - Source for quotes: https://dev.to/tonywangca/why-reddit-blocked-unauthenticated-json-in-2026-and-how-to-still-get-reddit-data-58b9 (2026-06-15, author sells a competing data API); https://lapcatsoftware.com/articles/2026/6/3.html (2026-06-12, independent developer, links the same post).
- What is returned now. 403 Forbidden for anonymous `.json`; no deprecation window; some tools get an empty or redirect response and fail silently (dev.to, 2026-06-15). A re-test on 2026-07-11 with a browser User-Agent still got 403 (search snippet of https://crawlora.net/blog/reddit-json-api-blocked-2026, vendor blog). A later source says anonymous `.json` on www.reddit.com returns 403 with a "blocked by network security" page (https://hushread.com/blog/reddit-rss-not-working, published 2026-10-19, updated 2026-11-11, vendor blog).
- Residential vs datacenter. No longer the deciding factor: the shutdown is by authentication state, not IP type. Before May 2026, and still for HTML pages, vendor blogs describe TLS-fingerprint and IP-reputation checks that 403 default `curl`/`requests` clients and datacenter/cloud IPs even with a valid User-Agent (search snippets of bigideasdb.com/reddit-json-403-forbidden and fetchlayer.dev, 2026, vendor blogs, unverified).
- "Blocked by network security" also hits ordinary browsers at times: a widespread incident on 2026-02-11 affected normal users (https://piunikaweb.com/2026/02/11/reddit-blocked-network-security-error-comments-not-loading/, 2026-02-11).
- Rate limit. The historical unauthenticated figure (about 10 requests/minute) is moot. No current unauthenticated allowance exists.
- Terms position. Reddit's own announcement classes unauthenticated `.json` use as a scraping surface ("scrape Reddit without accountability"). Using it, or working around the block, is squarely what Reddit says it does not allow.
- "Logged-in access won't be impacted" means a browser session or OAuth. Feeding a browser session cookie into a CLI would be automated access outside the published interface; no source says Reddit permits that, and the User Agreement summaries (Q3) say it does not.

**Conflict to flag:** dev.to/crawlora/scrapebadger date the block to late May 2026 (28th-30th). Hushread says anonymous `.json` and "API-based clients" were blocked "since about October 13, 2026" (citing Lurk issue 52, NewsBlur, SubStory). Likely two waves (endpoint shutdown in May; a broader network-level block in October), but that reading is ours and unverified.

---

## Q2. RSS / Atom feeds

**Status: ends 2026-11-13. Already throttled to about one request a minute since June. Not a route to build on.**

- Official end. r/modnews "Continuing our infrastructure updates", 2026-09-30, URL `https://www.reddit.com/r/modnews/comments/1wubgvt/continuing_our_infrastructure_updates_whats/`. Reported quotes: RSS has become a "common surface for large-scale scraping and automated abuse"; "If you use RSS for feeds outside of a community you moderate, there is no replacement." Moderators are pointed to the Discord Relay Devvit app.
  - Sources: https://techcrunch.com/2026/09/30/reddit-is-killing-rss-feeds-ending-public-api-access-because-of-ai-bots/ (2026-09-30); https://thenextweb.com/news/reddit-rss-feeds-shut-down-old-reddit-ai-scraping (2026-09-30); https://tech.slashdot.org/story/26/09/30/1841213/reddit-is-killing-rss-feeds-ending-public-api-access (2026-09-30).
  - Scope is reported as platform-wide (all interfaces). Whether the per-account private feeds (`?feed=...&user=...` from `reddit.com/prefs/feeds`) also stop on 11-13 is **not stated** in any source read.
- Throttling before the end. From the afternoon of 2026-06-11 feeds returned HTTP 429. Reported limit: down from 100 requests per 10 minutes to 1 per minute; headers seen: `x-ratelimit-used: 1`, `x-ratelimit-remaining: 0.0`, `x-ratelimit-reset: 58`. No Reddit announcement of the throttle. Adding account login to the reader did not help; appending the account's `user=` and `feed=` parameters did (works on public and `/search/.rss` feeds). Source: https://lapcatsoftware.com/articles/2026/6/3.html (2026-06-12), citing r/rss threads `1u3qrij` and `1u3qqmk`.
  - Other measurements conflict: "7 of 9 URLs 429 at 3 s spacing, 200 at 65 s spacing"; "about 15-18 sequential requests before 429" (search snippets, ideafast.pro / wprssaggregator, 2026). Reddit documents no number.
- Other failures: 403 for missing/generic User-Agent or blocked server addresses; `old.reddit.com` feeds redirect to `/login/` (hushread, updated 2026-11-11, citing FreshRSS issue 8189 and Open RSS, whose old-domain feeds stopped 2026-09-11).
- Format and limits while it lasted (https://www.wprssaggregator.com/reddit-rss-feed/, published 2026-06-26, updated 2026-10/11): Atom 1.0 (`<entry>`), 25 items by default, `?limit=` up to 100; patterns `/.rss`, `/r/SUB/.rss`, `/r/SUB/new.rss`, `/r/SUB/top.rss?t=`, `/r/A+B/.rss`, `/user/NAME/.rss`, `/r/SUB/comments.rss`, `/search.rss?q=&sort=`, `/r/SUB/search.rss?q=&restrict_sr=1`. Under load Reddit may return a non-XML "loading" page.
- Content: "no scores, media or comment trees" (Lurk developer, quoted by hushread, 2026). Entry body is HTML (long-standing behaviour; not re-verified this session). Per-thread comment feeds (`/comments/ID/.rss`) were flat, not threaded; exact depth/count limit not verified.
- Official support for personal use: no official Reddit help-centre article on RSS was found in search. A third-party report says "Reddit has never promoted RSS as a first-class way to consume content" (news.lavx.hu, 2026). Reddit's 2026-09-30 post thanks RSS users and ends the feature, which settles the question.

---

## Q3. Scraping old.reddit.com HTML and new Reddit HTML

**Status: forbidden by terms, blocked by robots.txt, and increasingly behind a login. Does not suit us.**

- User Agreement. Could not be fetched. Secondary descriptions agree on the structure: crawling is conditionally allowed only in accordance with robots.txt; scraping, or access by automated means, without Reddit's prior written consent is prohibited.
  - https://ppc.land/anthropic-loses-bid-to-keep-reddits-5-scraping-claims-in-federal-court/ (2026-09-26): "Reddit's user agreement conditionally permits crawling under its robots.txt file, but prohibits scraping without written consent"; Reddit relies on Section 3 (commercial exploitation limits) and Section 7 (method restrictions on automated access).
  - Vendor blogs give conflicting revision dates: "effective July 1, 2026", "last revised May 26, 2026", "revised through March 31, 2026" (coronium.io and others, search snippets). **Exact current wording and date not verified.**
  - Archived 2019 text (University of North Texas web archive, snapshot 2019-11-21, https://webarchive.library.unt.edu/web/20191121210518/https://www.reddit.com/help/useragreement): you may not "Access, query, or search the Services with any automated system, other than through our published interfaces and pursuant to their applicable terms." The principle is long-standing.
- robots.txt. Could not be fetched. Reported content since about 2024-06-25/07-01: `User-agent: *` / `Disallow: /`, with header comments pointing to the Public Content Policy and r/reddit4researchers (Simon Willison link blog, https://feeds.simonwillison.net/2024/Jul/24/google-reddit/, 2024-07-24; circuitbulletin copy, 2024). SerpApi's 2026-08-28 court pleading says Reddit's robots.txt allows Google but blocks other engines and agents, and that Google is served a different file (https://ppc.land/reddit-faces-antitrust-counterclaims-over-60m-google-only-crawler-deal/, 2026-09-02; this is a litigant's allegation). No 2025-2026 verbatim copy was obtained.
- Reddit's stated position (r/redditdev, 2024-06-25, u/traceroo, post `1doc3pt`, quoted in search results): "if you are using an automated agent to access Reddit, regardless of what type of company you are, you need to abide by our terms and policies, and you need to talk to us." Bots without an agreement are rate-limited or blocked.
- Public Content Policy (2024-05-09; help-centre page returned 403 to us). Per coverage (Axios 2024-05-09; search snippets): commercial use needs an agreement; Reddit says it wants to keep access for researchers and "responsible non-commercial use", via r/reddit4researchers. That is a programme you apply to, not a licence to scrape.
- Internet Archive: allowed in 2024, restricted in August 2025 to the homepage only (socialmediatoday / reclaimthenet, 2025-08). Shows the direction of travel.
- old.reddit.com: login required, announced in r/modnews 2026-06-30 and rolled out over the next month (https://tech.slashdot.org/story/26/07/01/1743219/reddit-will-require-you-to-log-in-to-use-old-reddit, 2026-07-01; Digital Trends, 2026-07). Reddit called Old Reddit "the biggest source of abusive scraping on the legacy interface" (redmantu, 2026-10-18). From the 2026-09-30 post: logged-in Old Reddit access will be limited to moderators and accounts that used it in the last six months (first announced as 90 days; TechCrunch 2026-09-30).
- New reddit.com logged out: still open as of the 2026-06-30 post ("this change will not impact logged-out browsing on reddit.com"), but on 2026-10-29 Steve Huffman announced a move to "a primarily logged-in experience", to be tested "in select markets outside the U.S." in "the coming months"; the front page and pages reached "from search or other external sources" are meant to stay viewable. He said "the vast majority of unauthorized access to Reddit" comes through logged-out pages (https://ppc.land/reddit-plans-to-make-logins-the-norm-after-20-years-of-open-browsing/, 2026-10/11).
- Conclusion: HTML scraping is prohibited by terms, disallowed by robots.txt, the legacy site is login-walled, and the main site is heading the same way. Unsuitable, as expected.

---

## Q4. Web search as a discovery layer (`site:reddit.com`)

### Which engines index recent Reddit content

- Since 2024-07-01, Bing, DuckDuckGo, Mojeek and Qwant cannot index new Reddit content; Microsoft confirmed Bing stopped crawling (404 Media / The Verge, 2024-07; restated as still true in SerpApi's pleading reported by ppc.land, 2026-09-02). Older Reddit pages remain in those indexes.
- Google has a licence (reported at about $60M a year, February 2024) and full access. Reddit denied the block was caused by the Google deal and said it was talking to other engines (Engadget, 2024-07). No 2025-2026 report of a Microsoft/Bing deal was found.
- Brave: a 2024 report said Brave was the one possible exception still showing recent Reddit results (webpronews, 2024, undated in snippet). Brave's "Discussions" feature has drawn on Reddit since 2022. **Current 2026 freshness of Reddit in Brave is unverified.** Note also News Corp sued Brave on 2026-07-22 alleging disguised crawlers (search snippet), which is a reason not to assume Brave's Reddit coverage is licensed or stable.
- Kagi: no source found on how it sources Reddit results.
- Practical reading: for recent threads, only Google-backed results are reliable. Bing-backed sources (DuckDuckGo, many SearXNG setups, older "web search" tools) will mostly surface pre-July-2024 threads.
- Discovery only solves half the problem. A search hit gives a URL, title and snippet. Fetching the thread itself then runs into Q1-Q3.

### Programmatic search APIs

| Service | Status / price (source, date) | Notes |
|---|---|---|
| Google Custom Search JSON API | "closed to new customers"; existing customers "have until January 1, 2027"; 100 free queries/day, then $5 per 1,000 (https://developers.google.com/custom-search/v1/overview, fetched 2026-11-12, undated) | Dead end for a new project. New Programmable Search engines capped at 50 domains since 2026-01-20 (https://brave.com/learn/google-api-shutdown/, 2026-08-14). Successor is enterprise Vertex/Agent Search. |
| Bing Web Search API | Retired 2025-08-11; no new sign-ups (https://learn.microsoft.com/en-us/lifecycle/announcements/bing-search-api-retirement) | Replacement "Grounding with Bing Search" is Azure-agent only, no raw results, about $14 per 1,000 (third-party figure, 2026-09). Bing has little recent Reddit anyway. |
| Brave Search API | $5 per 1,000 queries, 50 req/s; "$5 in free monthly credits" per plan; card required (brave.com/learn, 2026-08-14). Aggregators say the old 2,000-query free tier was removed 2026-02 (costbench, unverified) | Official API with its own index; legitimate for personal use. Terms (reported last updated 2026-09-01) not read. Reddit freshness unverified. |
| Kagi Search API | $12 per 1,000 on kagi.com/api/pricing; an older help page still says $25 (search snippets, 2026) | Official paid API; no free tier. Reddit sourcing unknown. |
| Tavily | About 1,000 free credits/month, then about $0.008/credit (aggregators, 2026-05; unverified against tavily.com) | Official API for agents. Upstream sources not disclosed in what we read. |
| Exa | Free allowance reported as either 1,000 requests/month or $10 monthly credit (aggregators conflict, 2026) | Official API, own index. Reddit coverage unknown. |
| SerpApi / Serper-type Google scrapers | Operating, but SerpApi is a defendant in Reddit's suit and in Google's suit (Q6) | They scrape Google result pages. Reddit calls this "data laundering". Avoid: this is exactly the conduct being litigated. |
| DuckDuckGo | No official web-search API (background knowledge; not re-verified) | Unofficial libraries scrape DDG's HTML; results are Bing-backed, so weak on recent Reddit. |
| Self-hosted SearXNG | Free software (background knowledge) | It works by scraping other engines' result pages, mostly against those engines' terms, and Google commonly blocks it. Not a clean route. |
| Agent harness built-in web search | Already available in Claude Code and similar | Legitimate and zero extra cost. In this session it returned plenty of reddit.com URLs, but the harness refused to fetch reddit.com itself. Good for finding thread URLs and snippets, not for reading threads. |

Legitimate for personal use: the official paid/free-tier APIs (Brave, Kagi, Tavily, Exa) and the harness's own search tool. Not clean: SERP scrapers, DDG scraping, SearXNG.

---

## Q5. Reddit's first-party offerings

- Reddit Answers (AI search). Consumer feature only. No official API found; the only "Reddit Answers API" results are third-party Apify scrapers (search, 2026-11-12). Reported usage caps conflict (for example 10 questions/week logged out; 20 or 50/day logged in; 100/day Premium; sources 2025, may be stale). Expanded to more languages in 2025.
- Devvit / Developer Platform. This is where Reddit is sending all developers, but it is the wrong shape for us: apps are TypeScript, run inside Reddit's platform and are installed into subreddits; outbound Fetch needs allowlisting; Python is not supported; a secondary blog reads it as not fitting "research scripts, market research, data pipelines or AI agents that read Reddit from outside" (ppc.land 2026-10-04; redditapis.com blog, vendor). The migration programme pays $1,000 per eligible app but requires an app that existed before 2026-03-25 and serves communities/end users (Reddit help article "Reddit Developer Platform App Migration Program 2026 Terms", `support.reddithelp.com/hc/en-us/articles/47822311698452`, via search snippet).
- Official "search API for individuals": none exists. Search is part of the Data API (`/search`), which is being closed:
  - 2026-10-31: no new public API access requests (already passed).
  - 2027-01-12: access "starts to close" for apps and users that have not registered or "responded to our outreach" (registration page: `https://developers.reddit.com/app-registration`).
  - March 2027: public API access closes for all remaining apps and users, no day given. Exempt: partners with written agreements (accessibility apps, some mobile clients).
  - Reddit did not answer whether registered-but-unmigrated apps keep working after March 2027.
  - Sources: https://ppc.land/reddit-to-halt-public-data-api-access-in-march-2027-as-14-000-apps-register/ (2026-10-04); TechCrunch 2026-09-30; r/redditdev post "Moving data API apps to the developer platform", `https://www.reddit.com/r/redditdev/comments/1wubcvf/` (2026-09-30, not read directly).
  - **This matters beyond this note:** an OAuth "personal script" plan now has a hard horizon. If no approved app already exists, the 2026-10-31 cutoff may already block starting one. Needs checking against the official-API research.
- Commercial licensing: the only route Reddit names for external readers (social listening, research tools, AI assistants). One developer reported a $10,000 negotiation (ppc.land, 2026-10-04). Not realistic for a personal tool.
- r/reddit4researchers: the named path for academic/non-commercial research (robots.txt comments and Public Content Policy, 2024). Current state in 2026 not verified.

---

## Q6. Litigation and enforcement context

- **Reddit v. Anthropic** (filed 2025-06-04, San Francisco Superior Court, CGC-25-625892). Conduct alleged: automated bots accessed Reddit "more than one hundred thousand times" after Anthropic said (July 2024) it had stopped; access continued despite robots.txt, the User Agreement, a May 2024 cease-and-desist and a licence offer; content used commercially to train models; scraping bypasses the Compliance API that propagates user deletions. Claims: breach of contract, unjust enrichment, trespass to chattels, tortious interference, unfair competition (Cal. B&P 17200). Removed to federal court; remanded 2026-03-30 (N.D. Cal., Judge Thompson: claims not preempted by copyright). A motion to dismiss was pending in state court; no ruling found. Sources: https://ppc.land/anthropic-loses-bid-to-keep-reddits-5-scraping-claims-in-federal-court/ (2026-09-26); Crowell / Loeb client alerts (2026-04).
- **Reddit v. SerpApi, Oxylabs UAB, AWMProxy, Perplexity AI** (filed 2025-10-22, S.D.N.Y., Judge Engelmayer). Conduct alleged: the three scraping firms evaded Google's SearchGuard bot detection (masked identities, rotating IPs) to pull Reddit content from Google result pages at scale and resell it; Perplexity bought it and, per the February 2026 amended complaint, itself ran queries with SerpApi's tools. Reddit's phrase: "industrial-scale data laundering". Lead theory: DMCA anti-circumvention (17 U.S.C. 1201). Ruling 2026-07-31 (ECF 104): 1201(a)(1)(A), 1201(a)(2) trafficking and civil conspiracy survive; the 1201(b) count, unfair competition and unjust enrichment were dismissed. SerpApi answered 2026-08-28 with antitrust counterclaims (Sherman Act s.2, attacking the Google-only crawl arrangement). Case management conference was set for 2026-10-01. Oxylabs' motion: sources conflict (one says the judge signalled denial, another says still under submission in September 2026). AWMProxy status not found. Sources: https://ppc.land/reddit-faces-antitrust-counterclaims-over-60m-google-only-crawler-deal/ (2026-09-02); MediaPost 2026-08-03 (403 to us, title only); https://serpapi.com/blog/reddits-lawsuit-is-a-dangerous-attempt-to-expand-platform-power (2026-03-13, defendant's view).
- Related: **Google v. SerpApi** (N.D. Cal.) dismissed 2026-07-20, amended complaint refiled 2026-08-10 relying on Google's Reddit licence terms (ppc.land, search snippet). **News Corp v. Brave** filed 2026-07-22 over allegedly disguised crawlers (search snippet).
- No new Reddit scraping complaint in late 2026 was found.
- 2026 platform enforcement (not litigation): Rule 8 updated to cover automated abuse (think-pol.com, 2026); the `.json`, RSS, Old Reddit and API closures above; planned comment-ID randomisation (redmantu, 2026-10-18).

**What Reddit objects to, in its own framing:** (1) large-scale automated collection, (2) for commercial or AI use, (3) without an agreement, (4) continued after notice, (5) using evasion (ignoring robots.txt, masked identity, rotating IPs, going through Google results). A single-user, low-volume, read-only tool is not the kind of defendant Reddit has sued. But the contract terms make no exception for scale: unauthenticated automated access is prohibited regardless, and the realistic consequence for an individual is IP or account blocking rather than a lawsuit. The stricter risk sits with any tool that works around a block, which is what we have ruled out.

---

## Per-route verdict

| Route | Allowed by terms? | Works today (2026-11-12)? | Suits personal research? |
|---|---|---|---|
| Unauthenticated `.json` | No. Officially shut down 2026-05-28 as a scraping surface | No (403) | No |
| `.json` with a browser session cookie | Not sanctioned; automated access outside published interfaces | Possibly, technically (unverified) | No |
| RSS, logged out | Was tolerated; ends 2026-11-13 | Barely (about 1 req/min, 429s) and gone tomorrow | No |
| RSS with account `feed=`/`user=` token | Was a Reddit-provided feature; fate after 11-13 unstated | Worked in June-Oct 2026 | No (assume it ends 11-13) |
| old.reddit.com HTML | No | No (login wall) | No |
| www.reddit.com HTML | No (robots.txt `Disallow: /`, User Agreement) | Partly; bot/IP blocking; login-first planned | No |
| Third-party SERP scrapers (SerpApi, Serper) | Not Reddit's terms directly, but the conduct Reddit is suing over | Yes | No |
| SearXNG / DDG scraping | Against the engines' terms | Flaky; weak on recent Reddit | No |
| Official search APIs (Brave, Kagi, Tavily, Exa) for discovery | Yes (their terms; you are not accessing Reddit) | Yes; Reddit freshness varies and is unverified | Yes, for URLs/snippets only |
| Google Custom Search JSON API | Yes | Closed to new customers; ends 2027-01-01 | No |
| Bing Web Search API | n/a | Retired 2025-08-11 | No |
| Agent harness built-in web search | Yes | Yes for discovery; fetch of reddit.com refused here | Yes, for URLs/snippets only |
| Reddit Answers | Consumer use only; no API | In browser/app only | No (not programmable) |
| Devvit | Yes | Yes, but runs inside Reddit, TypeScript | No (wrong shape) |
| Data API (OAuth) | Yes while it lasts | New requests closed 10-31; unregistered cut from 2027-01-12; ends March 2027 | Only short-term, and only with an existing/registered app |
| Commercial licence / r/reddit4researchers | Yes | By application | Unlikely for a personal tool |

**Bottom line:** there is no allowed, working, non-API route for a CLI to fetch Reddit threads. What remains legitimate is discovery (search APIs or the harness's own search, giving URLs and snippets) and a human opening the thread in a logged-in browser. Thread fetching depends on the official Data API, which itself is scheduled to close to the public in March 2027.

---

## Could not verify

1. Current verbatim text and effective date of the Reddit User Agreement (redditinc.com and archive.org not fetchable; secondary sources give three different dates).
2. Current verbatim reddit.com/robots.txt (last verbatim copy seen is from mid-2024).
3. Public Content Policy text (help centre returned 403).
4. The r/modnews (2026-05-28, 2026-06-30, 2026-09-30) and r/redditdev (2026-09-30) posts themselves; all quotes are via TechCrunch, The Next Web, Slashdot, PPC Land and blogs. Admin author names come from PPC Land only.
5. Whether private-token RSS feeds (`reddit.com/prefs/feeds`) stop on 2026-11-13.
6. Why sources give both late May and about October 13 for the anonymous `.json` block (two waves is our guess).
7. Exact RSS entry contents and per-thread comment feed limits (moot after 11-13).
8. What Reddit said about personal read-only scripts in the 2026-09-30 r/redditdev thread, and whether registered apps keep API access after March 2027 (Reddit reportedly did not answer).
9. Whether Brave, Kagi, Tavily or Exa return recent (2025-2026) Reddit threads, and their current terms and exact free tiers (aggregator figures conflict).
10. Status of Oxylabs' and AWMProxy's motions, and of Anthropic's state-court motion to dismiss, after September 2026.
11. Whether the login-first test for reddit.com has started, and in which countries.
12. Many 2026 details come from vendors selling Reddit-data alternatives (crawlora, bigideasdb, hushread, redditapis, scrapebadger, fetchlayer). They have a commercial interest in "everything is blocked"; the core facts are corroborated by TechCrunch, The Next Web, Slashdot, PPC Land and lapcatsoftware.
13. DuckDuckGo "no official API" and the SearXNG description are background knowledge, not re-verified this session.
