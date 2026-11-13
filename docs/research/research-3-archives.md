# Research 3 — Third-party Reddit archives as search/backfill sources (state at 2026-11-12)

Method: WebSearch + WebFetch only. "Fetched" means I retrieved the page on 2026-11-12 (local; about 2026-11-13 02:45 UTC). Reddit.com, redditinc.com, web.archive.org and most Reddit mirrors could not be fetched by the tool, so several Reddit-thread primaries are cited through whoever links them. Sources marked **[vendor]** sell a competing paid Reddit data product; treat their claims about free archives as interested and unverified unless corroborated.

## Headline findings

- **Arctic Shift is the only free archive verified live and current today.** A live API call returned a post that had been ingested 5 seconds after creation; monthly dumps are published through 2026-09.
- **PullPush could not be reached** (HTTP 403 on the site, HTTP 526 on the API host) and is reported, by interested sources only, to have stopped ingesting in May 2025.
- **Watchful1's torrents (including the per-subreddit top-40k dumps) are gone from Academic Torrents.** Arctic Shift's maintainer confirmed this on 2026-10-14; a linked r/DataHoarder thread title says Reddit requested the takedown. This is the most important change for a "keep a few subreddits locally" plan.
- **Pushshift is moderator-only**, access granted by Reddit, not usable by a non-moderator.

---

## 1. Pushshift

| Claim | Source | Date |
|---|---|---|
| Access is for approved Reddit moderators only. Request goes to r/Pushshiftrequest stating communities and moderation uses; Reddit answers by inbox "within one week"; approved usernames are shared with Pushshift for verification. | https://api.pushshift.io/guide | fetched 2026-11-12, undated |
| Tokens: sign in with Reddit at auth.pushshift.io/authorize, token lasts 24 h, refresh via POST auth.pushshift.io/refresh; one active token at a time; non-approved users get "The user is not an approved moderator on Reddit." | same | same |
| Reddit revoked Pushshift's Data API access 2023-05-02; later re-enabled for verified moderators, moderation use only. | https://www.redditapis.com/blogs/best-pushshift-alternatives-2026 **[vendor]**; https://www.xpoz.ai/blog/tutorials/how-to-get-historical-reddit-data-after-pushshift/ **[vendor]** (original Reddit announcement is the r/modnews post "Reddit Data API Update: Changes to Pushshift Access", 2023-05-01, seen only as a search-result title) | 2026-06-17 / 2026-07-08 |
| Management moved to the Network Contagion Research Institute (NCRI) in Feb 2023. | r/pushshift "New management for Pushshift" (search-result snippet only) | 2023-02-21 |
| Service was still operating in March 2025: outage reports (502s), a "restoration of search service" update and an "Action Needed: Reauthorization of API access" notice from the support account. | Search-result snippets of r/pushshift posts (thread 1j9q73q; Pushshift-Support submissions) — pages themselves not fetchable | 2025-03-12 to 2025-03-31 |
| Described as moderator-only in mid-2026 by third parties; an ACL 2026 demo paper calls the project "now defunct" for research purposes. | https://www.xpoz.ai/blog/guides/reddit-api-pricing-tiers-and-alternatives/ **[vendor]** (snippet); https://aclanthology.org/2026.acl-demo.42.pdf (snippet) | 2026 |

**Is the API running in 2026?** The guide page at api.pushshift.io loaded today, so the site is up, but I could not test the authenticated API and found no dated 2026 statement from Pushshift/NCRI/Reddit. Most likely still running for moderators; not verified.

**Non-moderators:** no access at all. No research tier.

**Unverified:** a vendor claims Jason Baumgartner said in November 2025 that he no longer controls the domain (https://think-pol.com/blogs/reddit-archive-api **[vendor]**, 2026-08-19). I found no primary source.

---

## 2. PullPush (pullpush.io)

**Up in 2026? Could not confirm; evidence points to unreliable/degraded.**

| Claim | Source | Date |
|---|---|---|
| https://pullpush.io/ returned HTTP 403 to my fetch; https://api.pullpush.io/reddit/search/submission/ returned HTTP 526 (Cloudflare: invalid origin SSL certificate). | own fetches | 2026-11-12 |
| Same pattern seen by others: HTTP 502 on 2026-07-29, HTTP 429 in Aug 2026, HTTP 526 on 2026-11-10. | https://think-pol.com/blogs/reddit-deleted-content-tools-tested **[vendor]** | published 2026-07-29, "last verified" 2026-11-10 |
| PullPush returned HTTP 403 to requests from GitHub's servers. | https://github.com/misiti15/MCF/pull/21 (independent, non-vendor) | 2026-10-09 |
| Ingestion stopped May 2025; newest content returned in a 2026-07-30 test was from May 2025; a frozen archive still answers HTTP 200 with results, so staleness is silent. | https://think-pol.com/blogs/reddit-archive-api **[vendor]** | 2026-08-19 |
| Conflicting: "often missing the most recent one to two months" (i.e. lagging, not frozen). | https://www.xpoz.ai/blog/guides/open-source-social-media-scrapers-what-still-works-2026/ **[vendor]** | 2026-09-02, updated 2026-10-21 |
| Rate limits: 15 req/min soft, 30 req/min hard, about 1,000 req/hour long-term. | https://www.redditapis.com/blogs/best-pushshift-alternatives-2026 **[vendor]** (matches what pullpush.io's own docs page has stated historically; could not re-fetch it today) | 2026-06-17 |
| Endpoints `/reddit/search/comment/` and `/reddit/search/submission/`, GET, no authentication; filters by author, subreddit, date range, score. | https://jentic.com/apis/pullpush.io/pullpush.md (OpenAPI catalogue entry) | fetched 2026-11-12, undated |
| Only free archive with cross-subreddit keyword search (`q` across all of Reddit). | redditapis.com post above **[vendor]** | 2026-06-17 |
| Operator: GitHub org "pullpush-io — Reddit Archive Project"; repos include the website (fork of Wamy-Dev/PullPush-Web), an `undelete` fork and a `reveddit` fork. No named individual, no contact, no terms. Vendors describe it as a single volunteer-run origin with no SLA. | https://github.com/pullpush-io | fetched 2026-11-12, undated |
| Removal: Arctic Shift's README links removal-request forms for Pushshift and PullPush, so a PullPush form exists. | https://github.com/ArthurHeitmann/arctic_shift | fetched 2026-11-12, undated |

Parameters (from the pullpush.io docs as I know them from before; **not re-verified today because the site returned 403**): `q`, `ids`, `size` (max 100), `sort` (asc/desc), `sort_type` (`created_utc`, `score`, `num_comments`), `author`, `subreddit`, `after`, `before`; comments also `link_id`; submissions also `title`, `selftext`, `score` (e.g. `>10`), `num_comments`, `over_18`, `is_video`, `locked`, `stickied`, `spoiler`, `contest_mode`.

Coverage: inherited Pushshift 2005–2023 data plus its own collection afterwards (redditapis.com **[vendor]**, 2026-06-17). Legal position: no published terms found; it has no agreement with Reddit that I could find; no reported lawsuit or takedown against it.

---

## 3. Arctic Shift

**Up in 2026: yes, verified.**

| Claim | Source | Date |
|---|---|---|
| Live call `GET https://arctic-shift.photon-reddit.com/api/posts/search?limit=1&sort=desc` returned JSON; newest post `created_utc` 1794537829 with `retrieved_on` 1794537834 (5 s later; about 2026-11-13 02:44 UTC). Ingestion is near real-time. | own fetch | 2026-11-12 |
| Status page shows "Available"; components: New posts, New comments, API, Archiver, ID Notifier, Clients. | https://status.arctic-shift.photon-reddit.com | fetched 2026-11-12 |
| "No uptime or performance guarantees"; free; no auth. | https://github.com/ArthurHeitmann/arctic_shift/blob/master/api/README.md | fetched 2026-11-12, undated |
| Independent report of HTTP 522 from GitHub's servers (cloud IPs may be blocked or it was briefly down). | https://github.com/misiti15/MCF/pull/21 | 2026-10-09 |

### API (base `https://arctic-shift.photon-reddit.com`) — source: api/README.md above

- **ID lookups:** `/api/posts/ids`, `/api/comments/ids`, `/api/subreddits/ids`, `/api/users/ids` — `ids` comma-separated (max 500), `fields`, `md2html`.
- **Search:** `/api/posts/search`, `/api/comments/search` — `author`, `subreddit`, `after`, `before` (epoch, ISO 8601, or offsets like `2d`, `1year`), `limit` (1–100 or `auto` = 100–1000 by server capacity; default 25), `sort` (`asc`/`desc`, **by `created_utc` only**), `fields`, `format` (`json`/`rss`/`jsonfeed`), `author_flair_text`.
  - Posts only: `title`, `selftext`, `query` (title + selftext), `link_flair_text`, `url` (prefix), `url_exact`, `over_18`, `spoiler`, `crosspost_parent_id`.
  - Comments only: `body`, `link_id`, `parent_id`.
- **Comment tree:** `/api/comments/tree` — `link_id` (required), `parent_id`, `limit` (1–25000, default 50), `start_breadth`, `start_depth`; collapsed nodes are `kind: "more"`.
- **Aggregates:** `/api/posts/search/aggregate`, `/api/comments/search/aggregate` (`aggregate` = `created_utc`/`author`/`subreddit`, `frequency`, `min_count`).
- **Subreddits:** `/api/subreddits/search`, `/rules`, `/wikis`, `/wikis/list`. **Users:** `/api/users/search`, `/api/users/interactions/users`, `/api/users/interactions/subreddits`, `/api/users/aggregate_flairs`. **Other:** `/api/short_links`, `/api/time_series`.
- **Full-text restrictions:** `title` only with `author` or `subreddit`; `body` only with `author`, `subreddit`, `link_id` or `parent_id`; both "not supported with very active users or subreddits". No Reddit-wide keyword search. Syntax: `a b` (AND), `"a b"` (phrase), `a OR b`, `a -b`. Comment search is Postgres FTS and described as slow; timeouts return "Query timed out" — narrow with `after`/`before` or lower `limit`, and retry (database warm-up).
- **No score sort or score filter** — open feature request: https://github.com/ArthurHeitmann/arctic_shift/issues/56 (2026-05-30).
- **Rate limits:** dynamic, by server load and query complexity; on HTTP 429 wait for reset. Headers named in the docs: `X-RateLimit-Reset` (seconds) and `X-RateLimit-Reset-At` (timestamp). The docs ask users to "be considerate" and to use the dumps for bulk work. A vendor quotes about 120,000 requests/hour sustained from a BAScraper benchmark (redditapis.com **[vendor]**, 2026-06-17) — unverified, do not design to it.
- **Score freshness:** for about the first 36 hours `score`/`num_comments` may be 0 or 1; afterwards values match the dumps, i.e. frozen at a roughly 36-hour re-scan.

### Dumps, tools, removal

| Claim | Source | Date |
|---|---|---|
| Monthly releases, each with an Academic Torrents link and SHA256 hashes; latest is 2026-09, released 11 Oct 2026; previous months released on the 7th–12th of the following month. | https://github.com/ArthurHeitmann/arctic_shift/releases | fetched 2026-11-12 |
| 2026-09 torrent: 74.36 GB (`RC_2026-09.zst` 51.95 GB comments, `RS_2026-09.zst` 22.41 GB submissions); author RaiderBDev; still live on Academic Torrents. | https://academictorrents.com/details/98b6dbad155750c4d5f65d5923e0c2d11f46da85 | fetched 2026-11-12, undated |
| Full link list for every month from 2023-10; aggregate 2005-06 to 2025-12 magnet is 3,804,096,351,995 bytes (about 3.8 TB). | https://github.com/ArthurHeitmann/arctic_shift/blob/master/download_links.md | fetched 2026-11-12, undated |
| Download tool for a single user or smaller subreddit: https://arctic-shift.photon-reddit.com/download-tool ; search UI at `/search`. | repo README | fetched 2026-11-12, undated |
| Helper `scripts/processFiles.py` reads .zst, .zst_blocks, .jsonl/.ndjson; needs Python 3.10+ and `zstandard`. | repo README | same |
| Removal requests via a Google Form linked in the README; contact by email or Discord. No formal terms of use published. | repo README | same |
| Known data-quality issues: some comments in RC dumps have a numeric `parent_id`. | https://github.com/ArthurHeitmann/arctic_shift/issues/88 | 2026-10-27 |
| Unofficial Hugging Face Parquet mirror `open-index/arctic`: 2005-12 to 2026-02, about 15.7 B items, 1.3–1.48 TB, last updated 2026-08-06, not gated, not affiliated with Arctic Shift; "no PII scrubbing". | https://huggingface.co/datasets/open-index/arctic | fetched 2026-11-12 |

---

## 4. Academic Torrents / monthly dumps

**Lineage:** Pushshift (stuck_in_the_matrix) files through early 2023; from 2023-04 collected by RaiderBDev (Arctic Shift); Watchful1 extracted, split per subreddit and re-packaged (Watchful1's own attribution, quoted in a search-result snippet of his r/pushshift post, 2024-02-07).

### Watchful1's torrents: taken down

| Claim | Source | Date |
|---|---|---|
| "That torrent was created by Watchful1. They have taken down all their torrents." (Arctic Shift maintainer, replying to "Top 40k subreddits torrent is down") | https://github.com/ArthurHeitmann/arctic_shift/issues/81 | 2026-10-14 |
| Maintainer linked the r/DataHoarder thread "reddit_requests_takedown_of_all_reddit_data_from…" and passed on an "intermediate magnet link" for the 2005-06 to 2025-12 aggregate. | https://github.com/ArthurHeitmann/arctic_shift/issues/42 ; thread URL https://www.reddit.com/r/DataHoarder/comments/1vajw0x/reddit_requests_takedown_of_all_reddit_data_from/ (not fetchable) | 2026-08-05 |
| Watchful1 announced on 2026-07-30 that the archive was coming down "at Reddit's request". | https://www.xpoz.ai/blog/guides/open-source-social-media-scrapers-what-still-works-2026/ **[vendor]** — only source for the date and wording | 2026-09-02 |
| Corroboration: Watchful1 torrent pages return HTTP 404 — hashes 30dee5f0… (the one still linked from his README), 1614740a… ("Subreddit comments/submissions 2005-06 to 2024-12") and 3d426c47… (aggregate 2005-06 to 2025-12). | own fetches of academictorrents.com/details/&lt;hash&gt; | 2026-11-12 |
| Last known per-subreddit sets: top 40k subreddits through 2023-12 (posted 2024-02-07) and through 2024-12 (posted 2025-02-20); a 2025-12 version is referenced by a third party. | search-result titles of r/pushshift threads 1akrhg3 and 1itme1k; https://github.com/misiti15/MCF/pull/21 | 2024–2025; 2026-10-09 |

Conflict: the MCF pull request dated 2026-10-09 still describes fetching the per-subreddit torrent from Academic Torrents, five days before the maintainer said all were down. Either the swarm still worked from an existing magnet/peers, or the PR was written from stale notes. The data is still in circulation among peers but has no listed home; relying on it means relying on leftover seeders of something its publisher withdrew.

"Top 20k" appears in older posts; every recent reference says top 40k.

### What is still published

- Arctic Shift's monthly Reddit-wide torrents (see section 3): latest **2026-09**, about 70–80 GB per month, not split by subreddit. Total history about 3.8 TB compressed through 2025-12 (magnet size), vendors quote 3.28–3.97 TB.
- Hugging Face mirror `open-index/arctic` (through 2026-02), queryable per month with DuckDB.

### Format

- zstandard-compressed NDJSON, one JSON object per line: `RC_YYYY-MM.zst` (comments), `RS_YYYY-MM.zst` (submissions).
- **Long window required.** Watchful1's reader uses `zstandard.ZstdDecompressor(max_window_size=2**31)` (https://github.com/Watchful1/PushshiftDumps/blob/master/scripts/single_file.py, fetched 2026-11-12). The CLI equivalent is `zstd -d --long=31`; a default decoder refuses these files. Budget up to 2 GiB of RAM for the window.
- Read in a streaming fashion; do not unpack to disk (Arctic Shift README recommends this). A chunk can end mid UTF-8 character — handle it (issue #80 in arctic_shift, 2026-10-12).

### Tools

- https://github.com/Watchful1/PushshiftDumps (MIT, repo still up): `single_file.py`, `iterate_folder.py`, `combine_folder_multiprocess.py` (multi-process filter by subreddit/author etc. into a new .zst).
- Arctic Shift `scripts/processFiles.py`; community zst→Parquet converter (arctic_shift issue #50, 2026-05-02).

### Feasibility of a handful of subreddits locally

- **Easiest today, without torrents:** Arctic Shift's download tool or paged `/api/posts/search` + `/api/comments/search` with `subreddit` and `after`/`before`, then index locally (SQLite FTS5 or Tantivy). Fine for small and medium subreddits; very active subreddits will be slow and may time out on text queries, but plain date-paged export works.
- **From monthly dumps:** each month is a 70–80 GB download that must be stream-filtered for your subreddits. Sensible only for keeping a short rolling window current, not for backfilling years (3.8 TB).
- **Per-subreddit torrents:** were the ideal fit (select only the files you want), but were withdrawn in mid-2026. Not a dependable input any more.
- A locally built index gives what no API here offers together: full-text over comments across your chosen subreddits, date ranges and score sort — but scores are frozen at archive time.

---

## 5. Other archive / search options

| Option | State | Source | Date |
|---|---|---|---|
| Reveddit | Working; shows moderator-removed content, using Reddit's live API (so it now depends on Reddit, not an archive). A second vendor says it requires moderator verification — **conflict, unresolved**; reveddit.com/about/faq would not render in the fetcher. | think-pol.com/blogs/reddit-deleted-content-tools-tested **[vendor]**; redditapis.com **[vendor]** | 2026-11-10; 2026-06-17 |
| Unddit | Domain does not resolve. Was a front end over Pushshift, then PullPush. | think-pol **[vendor]** | 2026-11-10 |
| Removeddit / Ceddit | Dead; removeddit.com now serves unrelated affiliate content. | think-pol **[vendor]**; xpoz **[vendor]** | 2026-11-10 |
| camas / redditsearch.io | Camas subdomain does not resolve; redditsearch.io listed as defunct. Both were Pushshift front ends. | think-pol; xpoz **[vendor]** | 2026 |
| PullPush's own search UI (search-new.pullpush.io) | Same backend and same availability problems as PullPush. | updownradar.com/status/search-new.pullpush.io (user-report based) | mid-2026 |
| Firehose equivalent | None found. Reddit has no public firehose; Arctic Shift's near-real-time API is the closest free thing. | absence of evidence, not proof | — |
| Internet Archive Wayback | Since August 2025 Reddit lets the Wayback Machine crawl only the homepage — no post, comment, subreddit or profile pages. Older snapshots remain, patchy. No search across them. | https://gizmodo.com/reddit-is-blocking-the-wayback-machine-from-archiving-posts-2000641546 ; https://9to5mac.com/2025/08/12/reddit-blocks-non-profit-wayback-machine-from-archiving-the-site/ | 2025-08-12 |
| Common Crawl | Reddit changed robots.txt in mid-2024 to shut out crawlers without an agreement; CCBot obeys robots.txt. So new coverage should have stopped around then — inferred, I found no source measuring Reddit pages in Common Crawl. | https://techcrunch.com/2024/06/25/reddits-upcoming-changes-attempt-to-safeguard-the-platform-against-ai-crawlers ; https://commoncrawl.org/faq | 2024-06-25; undated |
| Hugging Face | Many Reddit-derived datasets exist, including the 1.3 TB `open-index/arctic` mirror (live 2026-08-06). I found no documented Reddit-initiated takedown on Hugging Face. Older datasets that pointed at Pushshift's file host (e.g. ELI5) broke and were marked defunct. | https://huggingface.co/datasets/open-index/arctic ; https://huggingface.co/datasets/defunct-datasets/eli5/discussions/3 | 2026-08-06; undated |
| "Pushshift successors" | In practice two: PullPush (API clone, cross-subreddit search, unreliable) and Arctic Shift (API + dumps, current). Everything else is a front end over one of them or a paid vendor. | sections above | — |

Reddit-side context that affects any fallback-to-Reddit path (vendor-reported, citing r/modnews posts I could not fetch): unauthenticated `.json` endpoints announced for shutdown in late May 2026 (r/modnews thread 1tq9vxo), and login required for old.reddit.com announced end of June 2026 (r/modnews thread 1ujtebf) — https://think-pol.com/blogs/reddit-is-closing **[vendor]**.

---

## 6. Terms / legal position

| Claim | Source | Date |
|---|---|---|
| Reddit's Public Content Policy (2024-05-09) says use of public Reddit data at scale requires an agreement, and that those holding it should honour user deletions and Reddit removals. | https://techcrunch.com/2024/05/09/reddit-locks-down-its-public-data-in-new-content-policy-says-use-now-requires-a-contract ; Reddit's r/reddit announcement (snippet) | 2024-05-09 |
| Reddit's developer data-protection terms require deleting Reddit personal data and derivatives on request (10 days quoted). | r/redditdev post (search snippet) | 2024-03-04 |
| Reddit cut Pushshift off for terms violations in May 2023. | section 1 | 2023-05 |
| Reddit sued Anthropic (June 2025) and SerpApi, Oxylabs, AWMProxy and Perplexity (October 2025, S.D.N.Y.); the latter mostly survived motions to dismiss in July 2026. These target commercial scrapers and AI use, not archive hobbyists. | https://www.engadget.com/social-media/reddit-sues-perplexity-and-three-other-companies-for-allegedly-using-its-content-without-paying-205136436.html ; https://www.law360.com/articles/2508228/reddit-ai-data-scraping-case-mostly-survives-dismissal ; https://news.bloomberglaw.com/ip-law/reddit-advances-ai-scraping-suit-against-perplexity-serpapi | 2025-10; 2026-07 |
| Reddit blocked the Internet Archive (Aug 2025), citing AI companies scraping via the Wayback Machine. | section 5 | 2025-08 |
| First action against a volunteer archive: Reddit reportedly requested removal of Reddit data dumps from Academic Torrents; Watchful1 withdrew all his torrents (mid-2026). | section 4 | 2026-07/08 to 2026-10-14 |
| No lawsuit or public cease-and-desist against Arctic Shift or PullPush found. Arctic Shift's monthly torrents were still listed on Academic Torrents today. | own fetches | 2026-11-12 |

I could not fetch the Data API Terms themselves (redditinc.com blocked). The widely reported position — the terms forbid redistributing or sublicensing API-derived data and require honouring deletions — is stated here from general knowledge and the secondary sources above, not from the text.

### For a personal user who only queries a third-party archive

Straight version of the grey area:

- **You are not party to Reddit's API terms** if your tool never calls Reddit, so there is no contract of yours being broken by a query. The archive operator is the one in tension with Reddit's terms and policy. Reddit's enforcement so far has been against operators and commercial resellers; I found no case against an end user of an archive. This is an observation, not legal advice.
- **You are, though, using data Reddit says may not be redistributed**, and after the 2026 Academic Torrents request it is clear Reddit now objects to volunteer dumps too, not just AI companies. Building on these sources is building on something Reddit is actively trying to shut.
- **Privacy is the real ethical issue.** Archives hold posts and comments that authors later deleted and that moderators removed; Arctic Shift ingests within seconds, before most deletions. A read-only personal tool should: not surface or highlight deleted/removed items as a feature, not profile individual users, keep results local, and respect that operators offer removal forms (a locally cached copy will not reflect later removals unless refreshed).
- **Scores are frozen** (Arctic Shift: about 36 hours after posting; dumps the same). Ranking by score from an archive under-weights slow-burn threads; a "top" sort is only approximate.
- **Staleness can be silent.** A frozen archive still returns HTTP 200 with results. Always check the newest `created_utc` a source returns before trusting a "no recent results" answer.
- **Any of these can vanish without notice** — single volunteer operators, no SLA, and now demonstrated legal pressure. Treat them as optional; the tool must work when they are absent.
- **Be a considerate client:** low request rates, honour 429 and the reset headers, cache, identify the client generically, no bulk harvesting through the API (use dumps for that, as Arctic Shift asks).

---

## 7. Verdict table

| Source | Date-range search | Comment full-text | Cross-subreddit keyword | Current data | Verdict |
|---|---|---|---|---|---|
| Pushshift | yes | yes | yes | yes (reportedly) | **(c) Not suitable** — moderators only, access granted by Reddit for moderation use |
| PullPush | yes | yes | yes | no or lagging (reported frozen May 2025); unreachable today | **(c) for now; at most a last-resort optional fallback** behind a health check and a freshness probe. Never primary. |
| Arctic Shift API | yes | yes, but only within one subreddit / author / thread, not for very active ones | no | yes, seconds behind | **(b) Best optional fallback/backfill** for date-ranged and historical search, comment search within a subreddit, thread and ID lookups. Not (a): no guarantees, no Reddit-wide text search, no score sort, sits under visible legal pressure. |
| Arctic Shift monthly dumps (Academic Torrents) | n/a (offline) | yes, once indexed locally | yes, once indexed | about 4–6 weeks behind (2026-09 latest) | **(b) Backfill for a local index** of chosen subreddits; heavy (70–80 GB/month) |
| Watchful1 per-subreddit torrents | n/a | yes, once indexed | — | withdrawn 2026 | **(c) Not suitable to depend on** — taken down; would have been the best local-backfill input |
| Hugging Face `open-index/arctic` | n/a (Parquet/DuckDB) | yes, by scan | yes, by scan | through 2026-02 | **(b) Possible backfill**, unofficial third-party mirror, could disappear |
| Reveddit / Unddit / Removeddit / camas / redditsearch.io | — | — | — | — | **(c)** dead, or (Reveddit) a removed-content viewer, not a search source |
| Wayback Machine | no | no | no | frozen at Aug 2025 for threads | **(c)** for search; occasional single-URL lookup of an old thread only |
| Common Crawl | no | no | no | effectively pre-mid-2024 | **(c)** |

Reddit's official API has no date-range search and no comment search (stated by every source above; consistent with the API as documented). Those two capabilities are exactly what Arctic Shift (scoped) and a local dump index provide.

**Recommended shape:** Reddit's own API/search as primary; Arctic Shift API as an opt-in, clearly labelled fallback for date ranges, older material and in-subreddit comment search; an optional local index for a few chosen subreddits filled through Arctic Shift's API/download tool (or monthly dumps for the rolling window). Keep PullPush as a disabled-by-default adapter at most.

---

## Could not verify

1. **Why and exactly when Watchful1 withdrew his torrents.** The "at Reddit's request, announced 2026-07-30" wording is from one vendor blog; the primary evidence is a maintainer's statement that they are down (2026-10-14), a linked r/DataHoarder thread whose title says Reddit requested the takedown (2026-08-05 link; thread not fetchable), and 404s today. Whether Reddit used a DMCA notice, a letter or an informal request is unknown, as is whether Academic Torrents or Arctic Shift received a request themselves.
2. **PullPush's real state.** Site and API host both errored for me; "frozen since May 2025" (THINKPOL) and "lagging one to two months" (Xpoz) conflict and both come from competitors. Its documented rate limits and parameter list were not re-read today. Operator identity and any terms are unknown.
3. **Whether Pushshift's moderator API is still running in 2026.** Guide page loads; last dated first-party activity I found is March 2025.
4. **Baumgartner "no longer controls the domain" (Nov 2025)** — single vendor claim, no primary.
5. **Arctic Shift numeric rate limits and actual header values.** Docs name `X-RateLimit-Reset` and `X-RateLimit-Reset-At` only; whether `X-RateLimit-Remaining`/`-Limit` are also sent needs a live header check (the fetch tool does not show headers). The 120,000 req/hour figure is vendor-quoted.
6. **Reveddit's current mechanism** (live Reddit API vs moderator verification) — two vendors disagree; its own FAQ would not load.
7. **Common Crawl's actual Reddit coverage** — inferred from robots.txt policy, not measured.
8. **Hugging Face takedowns of Reddit datasets at Reddit's request** — none found; absence of evidence only.
9. **Exact text of Reddit's Data API Terms** on redistribution and deletion — site not fetchable; summarised from secondary sources.
10. **Wayback coverage after August 2025** — web.archive.org not fetchable; based on news reports from the time.
11. **Academic Torrents listing/search page** sits behind a browser check, so I could not list all Reddit torrents, seed counts or dates added; only individual detail pages were checked.
12. Dates printed on several vendor pages are internally inconsistent (footers newer than bylines); their "tested on" dates are taken as stated.
