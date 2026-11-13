# reddit-search (`rdt`)

A local command-line tool that turns Reddit thread URLs into compact, citable text for a research agent, and keeps what it fetched in a searchable local cache.

Reddit closed its own API to new users (see [docs/REPORT.md](docs/REPORT.md)), so `rdt` never contacts Reddit. Threads come from the Arctic Shift archive, a volunteer service with no guarantees. If it is unreachable the tool says so and serves what the cache has.

## How it is meant to be used

1. A research agent finds threads with its own web search (`site:reddit.com ...`).
2. It passes the URLs to `rdt thread`, which prints the post and the comments worth reading.
3. To dig further, it passes the same URLs to `rdt subs`, which says which subreddits the topic lives in and how to search them, then runs `rdt search`.
4. It cites by id (`t3_...`, `t1_...`) and expands ids to links with `rdt cite`.

```
rdt thread https://www.reddit.com/r/AMDHelp/comments/1x5sulf/some_title/ --focus "psu undervolt"
rdt thread t3_1x5sulf --budget 12000          # more of the thread
rdt thread t3_1x5sulf --expand t1_abc123      # load a collapsed branch
rdt thread t3_1x5sulf --comment t1_abc123     # one comment in full
rdt resolve <url> <url> ...                   # URLs to ids, duplicates dropped, cached or not
rdt subs "coil whine undervolt" <url> <url> ...       # which subreddits to search, with sizes
rdt search "coil whine undervolt" --sub AMDHelp,buildapc --after 2026-06-01
rdt search "coil whine" --from-urls <url> <url> ...   # search the subreddits those URLs point into
rdt search "coil whine" --sub AMDHelp --comments      # also search comment text
rdt local "memory training"                   # cache only, offline
rdt cite t3_1x5sulf t1_abc123                 # full citations with permalinks
rdt verify-quote t1_abc123 "exact words"      # is this quote really in the cached text?
rdt sources                                   # is the archive reachable and current?
rdt cache stats | check | purge --older-than-days 7 | forget t3_1x5sulf
rdt ui                                        # read-only window onto the cache, http://127.0.0.1:8779/
rdt doctor                                    # offline self-check
```

Add `--wait` to any command to sit out a rate limit of up to 90 seconds and try once more, instead of stopping. Add `--json` to `thread`, `resolve`, `subs`, `search` and `local` for machine-readable output.

## The agent

`reddit-scout` is a research agent that does all of the above on its own: it searches the web, reads up to four threads with `rdt`, and returns an answer with checked citations. Its definition is installed at `%USERPROFILE%\.claude\agents\reddit-scout.md`; [docs/agents/reddit-scout.md](docs/agents/reddit-scout.md) is a copy kept with the code. Ask for it by name, or ask "what does Reddit say about ...".

## The window

`rdt ui` serves a small page on this machine only (port 8779, `--port` to change) for browsing and searching what is cached: the thread list, full threads with their comment trees, and cache search. It is read-only: it never fetches a thread and never changes the cache. The one network action is the "Check the archive" button, which makes a single status request. It answers only requests addressed to 127.0.0.1 or localhost on its own port, and thread text is placed in the page as text, never as markup. Stop it with Ctrl+C.

## Things to know

- **Search needs a subreddit.** The archive can search text only inside one subreddit, newest first, and times out on very busy ones; narrow with `--after`/`--before`. Reddit-wide discovery is the web search's job.
- **Subreddit suggestions come from URLs, not topics.** The archive knows subreddits by name only. `rdt subs` counts the subreddits named in the URLs you give it and in matching cached threads, checks up to four of them in the archive (one request each) for size, and guesses at names from the topic's words only when it has fewer than two candidates. With no URLs and nothing cached, a built-in starter list proposes subreddits from the topic's words (hardware brands, PC help, a few games and systems); those names cost no requests and are marked "starter list". Sizes there are updated infrequently, so "busy" is an estimate.
- **Scores are frozen** about 36 hours after posting. The output says `scores_as_of`.
- **One host only.** The tool talks to arctic-shift.photon-reddit.com over HTTPS and follows no redirects.
- **Rate limits, shared by the whole PC.** The archive's allowance is small: a burst of a dozen requests can lock you out for a minute or two. Every `rdt` process books each request in the cache's request log before sending it, so all runs on this PC share one pace: 2 seconds apart, at most 10 in any minute, and nothing while a rate limit the archive gave any run is still in force. A wait for a free slot of up to a minute is sat out; a longer one, or a rate limit longer than 10 seconds, ends the command with exit code 5 and the number of seconds to wait, unless `--wait` was given. `rdt sources` shows the budget left, and a search says on stderr before it starts when the budget is low. When the archive times out on a search, the first subreddit it happens to in a run is retried once, narrower (the last 180 days or the newer half of the window, else only the first two words), and the output says what was dropped; later timeouts in that run are reported, not retried, so a run never sends more than 0.2 did. `search --json` ends with a `coverage` block: subreddits asked, answered, narrowed, timed out, rate limited or not reached, and how far back each answer reached. A remembered "no such subreddit" is asked again after a day. Subreddit sizes are remembered for 30 days, so asking `rdt subs` about the same subreddits again costs no requests.
- **Cache.** `%LOCALAPPDATA%\rdt\cache.sqlite` (override with `RDT_DB`). Version 0.3 upgrades an older cache in place on first use (it gains the request ledger); a 0.2 build then refuses that cache rather than rewrite it, so update every copy of `rdt` on the PC together. A cached thread is reused for 6 hours, then re-fetched. Threads not fetched for 30 days are deleted. Text, title and author of anything the archive marks removed or deleted are never stored. A re-fetch blanks what has gone since, and `rdt cache check` does the same for the whole cache (it leaves items alone when the archive's answer looks incomplete, and says so). Offline commands never delete anything.
- **What a digest leaves out.** Short, low-score remarks that match nothing you asked for ("same here") are put last and shown only while at least half the budget is free, so a small thread still shows its short answers. `--focus` words are also tried as one phrase, and a phrase or model number counts far more than a single common word. `--full` shows everything.
- **URLs from mirror sites work.** Web searches often return Reddit threads on other hosts; any URL shaped like `/r/<name>/comments/<id>/...` is accepted for its id, and the mirror is never contacted.
- **Thread text is untrusted.** Output marks every line of user text with `| `, tags carry a per-run code, invisible characters are stripped and instruction-like text is flagged.
- **PowerShell 5.1** mangles double quotes inside arguments to programs. For a phrase query use `rdt search --% "\"coil whine\" undervolt" --sub AMDHelp`, or call `rdt` from Git Bash.

Exit codes: 0 ok, 2 bad usage, 3 nothing found, 4 not cached and offline, 5 rate limited or the PC's shared request budget used up (the stderr line says which, and how many seconds to wait), 6 source or cache error.

## Packaging for another PC

`.\dist.ps1` runs the gate, builds a release and writes `dist\rdt-<version>-windows-x64\` and a `.zip` of it with a `.sha256` beside it. The folder holds `rdt.exe`, `QUICKSTART.txt`, `LICENSE.txt`, `THIRD-PARTY.txt` (every component compiled in, with its licence) and the `reddit-scout` agent definition with a placeholder path.

The exe is a single file for Windows 10 and 11 (64-bit) that needs nothing installed: the script fails if it would need any DLL that is not part of Windows. It is built with this PC's folder paths replaced, and the script fails if any packaged file contains this PC's user name. It then unpacks the zip elsewhere and runs it with only Windows on PATH. It has not been tried on a second PC.

## Layout

- `crates/core`: parsing, cache (SQLite with full-text search), ranking, comment selection, output. No network code.
- `crates/sources`: the archive adapter behind a transport trait; tests replay the recorded responses in `fixtures/arctic` (usernames scrubbed).
- `crates/cli`: the `rdt` binary, including the window (`src/ui.rs` and `src/ui/`).
- `docs/`: research report, plan, decisions, and the research notes with sources.

To run `rdt` from any terminal, add `target\release` (or the folder you unpacked a release into) to your PATH.

Gate: `.\gate.ps1` (fmt, clippy with warnings as errors, tests). Build: `cargo build --release` after putting cargo and WinLibs on PATH as the gate does.
