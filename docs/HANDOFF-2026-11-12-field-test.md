# Handoff: rdt field test on a real research task (2026-11-12)

**Task:** find what Reddit users report about RX 9070 XT hard freezes in Cyberpunk 2077, as one source inside the pc-tune freeze investigation (`Dev\pc-tune\reports\RX 9070 XT Cyberpunk hard freezes.md`).
**Setup:** the `reddit-scout` agent ran 4 times in parallel on 4 angles (hardware/power, frame generation/driver, Cyberpunk 2.5 and mods, multi-monitor/overlays), each told to use `rdt local` first and go easy on the archive. Three runs were stopped by the user part-way and started again.
**Evidence:** the four runs' reports and notes (`Dev\pc-tune\research_notes\RX 9070 XT Cyberpunk hard freezes\reddit_*.md`) and a live `rdt sources` afterwards: archive reachable and current, 24 threads and 673 comments cached, 71 archive requests in the last hour. The runs' own claims (timeouts, "archive offline") were not re-checked one by one.

## Verdict

**Useful, with limits.** rdt turned threads into citable text with ids, dates and scores. That added two findings the web-only research had missed, and both changed the fix plan:
- **A −300 MHz max-clock cap:** the only fix reported for the exact Kernel-Power 41 / no-dump signature on a 7800X3D + 9070 XT.
- **A DDU clean install beats a plain rollback:** several owners say rolling back the driver without DDU did not stop the freezes.

The weak part is coverage. The four runs read about 20 threads in total, and two of them leaned mostly on the cache. Several subreddits the prompts named were never searched. The tool and its manners are sound; the limit is how many threads it can reach under the archive's allowance, plus how threads are found in the first place.

| | Rating | Why |
|---|---|---|
| Accuracy and citations | Good | Every finding came with an id, a permalink, a date and a score; nothing invented was found |
| Adding new information | Good | 2 findings that changed the plan, and 3 "this did not work" findings (Gen4, bigger PSU, MPO) the web research lacked |
| Coverage | Weak | About 20 threads across 4 runs. r/Amd, r/buildapc, r/techsupport, r/PcBuildHelp and r/WallpaperEngine were never searched |
| Speed | OK | 1 to 1.5 minutes per run after restart; the first runs were slower and were stopped |
| Use alongside other runs | Poor | Four parallel runs share one small allowance: timeouts, a rate limit, and one run saw the archive as "offline" |

## What worked

1. **The cache earned its keep.** The restarted runs reused threads the stopped runs had fetched (1xjudv7, 1x0r4wj, the power-cable and Kernel-Power 41 threads) at no cost. One run did useful work from the cache alone while the archive was unavailable to it.
2. **Citable output.** Permalinks and ids went straight into the report. `cite` and `verify-quote` make the citations checkable, though no run used `verify-quote` (see 6 below).
3. **Manners held.** Requests stayed on the archive, 2 s apart, and the tool stopped with exit 5 instead of hammering. Nothing contacted reddit.com.
4. **The agent stayed honest about gaps.** Every report listed what it could not search, the threads it saw only as snippets, and which findings were single anecdotes.

## Problems found, most important first

### 1. Parallel runs starve each other (tool + agent)
The allowance is per machine, not per run: "a burst of a dozen requests can lock you out for a minute or two" (README). Four agents at once used 71 requests in an hour and got timeouts (r/AMDHelp, the r/cyberpunkgame `red4ext` search) and at least one rate limit. Each run then covered less.
- **Tool:** a shared request ledger in the cache DB, so each `rdt` process sees the machine-wide recent count and paces against it rather than only against its own last request. Show the remaining budget in `rdt sources`, and print a clear "budget low, N s until reset" before a search starts, not mid-run.
- **Agent:** say in `reddit-scout.md` that parallel scouts must split subreddits between them, not overlap, or that one scout with several angles beats four scouts on one budget.

### 2. Finding threads is the weak link (agent workflow)
Step 1 of the agent is "search the web first". In this test, web search returned almost no reddit.com URLs, only mirrors and non-Reddit pages, so `rdt subs` / `--from-urls` had little to work with. The runs fell back to `rdt search --sub` on subreddits the prompt named.
- **Agent:** make `rdt search` across a short list of known subreddits the first step, and treat web search as a way to discover extra subreddits, not as the main source.
- **Tool:** a built-in topic → subreddit starter list (hardware: radeon, AMDHelp, Amd, buildapc, techsupport; games: the game's subs), so `rdt subs` works even when the web search finds nothing.

### 3. Timeouts on wide queries (tool)
Queries the archive cannot answer quickly time out. The tool retries only once per run, so one slow subreddit can end the whole search.
- When a query times out, retry it narrower before giving up: a shorter date window, then fewer terms. Report which part of the query was dropped.
- Count the retry per subreddit, not per run, and keep the results already returned from the other subreddits.

### 4. Coverage is invisible until the end (tool)
The runs only knew afterwards that r/Amd and others were never searched.
- `rdt search --json` should end with a coverage block: subreddits asked, answered, timed out or rate limited, and the date range actually covered. The agent can then cite coverage instead of reconstructing it.

### 5. "Archive offline" was ambiguous
One run reported the archive offline and did cache-only work. Minutes later `rdt sources` showed it reachable and current. Most likely it was a rate limit or timeout read as an outage. Exit codes 5 (rate limited) and 6 (archive failed) exist, but the agent's wording blurred them.
- **Tool:** print one plain line on stderr with each exit: `rate limited, retry in 74 s` vs `archive unreachable (timeout after 3 tries)`.
- **Agent:** quote that line in its gaps section rather than paraphrasing it.

### 6. Quotes were not verified
No run used `verify-quote`, yet the report quotes thread text.
- **Agent:** require `rdt verify-quote` for every direct quote before it goes into a notes file, and say so in the output.

### 7. Snippet-only threads are mixed in with read ones
Threads seen only as search snippets (t3_1xgpaoi, t3_1xoa9om, t3_1xleluj) were listed next to fully read ones, with only a note.
- **Agent:** put snippet-only threads in their own section that never counts toward a "pattern".

### 8. Stopped runs leave nothing behind
The three stopped runs wrote no notes. Their fetched threads stayed in the cache but their reasoning was lost.
- **Agent:** write the notes file early and append after each thread, so a stop leaves partial notes.

## Suggested work order

1. Shared request ledger and visible budget (problem 1, tool). The biggest gain, because every multi-angle task runs into it.
2. Agent prompt changes (problems 1, 2, 5, 6, 7, 8). Cheap, and needs no code.
3. Coverage block in `--json` (problem 4).
4. Narrower retry on timeout (problem 3).
5. Built-in subreddit starter list (problem 2, tool side).

Each tool change goes through `reddit-search-reviewer` before commit. That reviewer checks request manners, so the shared ledger especially must not let total requests go up.

## What was done (2026-11-12, rdt 0.3.0, uncommitted until the owner says yes)

All five steps of the suggested order are built, with tests (117 in all, `gate.ps1` passes). `reddit-search-reviewer` ran three passes: the first found 8 issues (1 to 6 fixed, 7 and 8 accepted as designed, see below), the second found a narrower retry that a short rate limit could stretch to a third request (fixed: the retry is sent once, no retry of its own), the third cleared it to commit with total requests never above 0.2.0. The release binary the agent uses (`target\release\rdt.exe`) is rebuilt as 0.3.0; a copy of the real cache upgraded cleanly to schema 3 and kept its threads and request history.

1. **Shared request ledger (problem 1, tool).** Cache schema 3: every request is booked in `request_log` (now to the millisecond, `at_ms`) inside a write lock before it is sent, so every `rdt` process on the PC shares one pace: 2 s apart, at most 10 in any 60 s, and nothing while a rate limit any run saw is still in force (the archive's wait, capped at 300 s machine-wide). A wait for a slot of up to a minute is sat out; a longer one is not booked and ends with exit 5 ("request budget used up ... not sent, try again in N s"). A request that cannot be booked is not sent. `rdt sources` prints a `budget` line; a search prints "budget low" or "rate limited, retry in N s" on stderr before it sends anything. The old 2 s sleep at the start of each run is gone (the ledger covers it). The cache upgrade is one way: a 0.2 build refuses a version 3 cache.
2. **Agent prompt (problems 1, 2, 5, 6, 7, 8).** `reddit-scout.md` (user copy and `docs\agents` copy, which differ only in the `rdt.exe` path): a section on the shared budget and splitting subreddits between parallel scouts; `rdt local` then `rdt search --sub` first, web search only to widen; exit lines quoted verbatim, never "archive offline"; `verify-quote` on every quote; snippet-only threads in their own section; the notes file written first and appended after each thread; Gaps cites the `coverage` block.
3. **Coverage block (problem 4).** `search --json` ends with `coverage`: subreddits asked, answered, narrowed, timed out, rate limited, budget used up, failed, not reached, skipped (over 5), whether comments were searched, the requested range, per subreddit how far back the answer reached (a full page of 25 means older matches were not seen), and the budget warning. A failed search's JSON carries it too.
4. **Narrower retry (problem 3).** When the archive times out on a subreddit's search, it is retried once narrower: the last 180 days (or the newer half of a window over 30 days), else only the first two words; the `source:` line and the coverage say what was dropped. The other subreddits' results are kept. **Deviation from the handoff:** the retry is one per run, not one per subreddit, because one per subreddit could send more requests than 0.2.0 did (the reviewer measured 10 vs 6 for five subreddits). Later timeouts are reported, not retried.
5. **Starter list (problem 2, tool).** `crates\core\src\starter.rs`: topic words → subreddits (AMD, NVIDIA, Intel, monitors, storage, laptops, networking, Windows, Linux, a few games, emulation, handhelds, general PC help). `rdt subs` uses it only when URLs and the cache gave nothing, never looks those names up (no requests), marks them "starter list (word)", and proposes a one-year `--after` for them.

Not done: problem 5's tool half beyond the existing messages (the stderr line is the error message prefixed `rdt:`; the prompt now tells the scout to quote it). Accepted review findings: if the wall clock jumps back more than a minute, bookings made "in the future" hold requests back until it catches up (fails safe); and a starter-list suggestion followed by an agent does spend requests 0.2.0 would not have suggested (the tool itself sends none for it).

## Not tested in this run
- `rdt ui`, `cache check`, `cache purge`, `verify-quote` and `--comments`.
- Reading output in a language other than English.
- A topic outside hardware and games.
