# Decisions that are yours

Background for each is in [REPORT.md](REPORT.md). Nothing gets built until 1 and 2 are answered, because together they decide whether there is a tool to build.

## 1. Do you already have a Reddit API app?

**Answered 2026-11-12: no, and one cannot be created.** The create-application form at `reddit.com/prefs/apps` still renders, but submitting it returned "In order to create an application or use our API you can read our full policies here", pointing to the Responsible Builder Policy. This confirms first-hand what the research reported. The Reddit OAuth source (slice 7) is out; the build now depends on decision 2 alone.

Original text, kept for the record:

Only you can check. In your own browser, signed in to Reddit, open `reddit.com/prefs/apps`.

- **An app is listed** (created before November 2025): it can serve as a source until about March 2027. To keep it working past 2027-01-12 you would register it at `developers.reddit.com/app-registration`, then type its client id and secret into `rdt auth set`. Registering means agreeing to Reddit's current developer terms, which I could not read; read them before agreeing.
- **Nothing is listed**: there is no way to get one now. Skip to decision 2.

I will not sign in, create, register or agree to anything on your behalf.

## 2. Use Arctic Shift as a source?

**Answered 2026-11-12: yes.** Version 0.1.0 was built on it.

It is the only source that works today and has no announced end. It is a volunteer archive that Reddit does not approve of. Using it calls no Reddit service and breaks no agreement of yours, but it could vanish at any time, and it holds content its authors later deleted.

- **Yes**: the tool is worth building, with or without a Reddit app. I recommend this, with the privacy rules in decision 4.
- **No**: the tool depends entirely on decision 1.

**If the answer to both 1 and 2 is no, I recommend not building this.** Research agents would keep using web-search snippets for Reddit.

## 3. May fetched threads be passed to a hosted model?

**Not answered.** The tool was built on the assumption that archive content may be read by research agents, since that was the purpose of the request. Say so if you want it otherwise.

The whole point is that a research agent reads the digests, and the agent is a hosted model. No source says whether Reddit's terms allow this for API-sourced content, and the policy reportedly restricts sharing with third parties.

- **Arctic Shift content**: not covered by any agreement of yours.
- **Reddit API content**: a grey area on your account.

Options: allow both; allow archive content only and keep Reddit API content for your own reading; or allow neither, which leaves a local reader and search tool for you alone.

## 4. How long does the cache keep things?

**Not answered; built with the recommended option.** A cached thread is reused for 6 hours, re-fetched after that when online, and deleted after 30 days without a fetch. Both numbers are constants at the top of `crates/cli/src/run.rs`.

- **Strict**: anything older than 48 hours is purged. This matches Reddit's reported guidance for API content and gives up most of the offline value.
- **Re-check (recommended)**: items are kept; when online, anything older than 48 hours is re-checked before use and blanked if it was deleted; anything not re-checked for 30 days is purged. This is my reading of how to meet the deletion duty, not something Reddit has stated.
- **Keep until purged by hand**: simplest, and the furthest from Reddit's guidance for API content.

In every mode: no command gathers one user's posts across threads, and bodies known to be deleted or removed are never shown.

## 5. Scope of the first version

Recommended 0.1.0: `thread`, `resolve`, `local`, `search`, `cite`, `verify-quote`, `cache`, `sources`, with discovery left to the agent's web search. The `research` pipeline, an agent definition and the window come after. Say if you would rather have the window sooner.

## 6. Smaller choices, with my defaults

- **Binary name**: `rdt`.
- **Cache location**: `%LOCALAPPDATA%\rdt\`, outside OneDrive so thread text is not synced to the cloud.
- **Over-18 content**: excluded by default, `--nsfw` to include.
- **A paid search API for discovery** (Brave, about $5 per 1,000 queries): not needed while agents have their own search; would need a key that you create.
- **Reviewer**: a read-only `reddit-search-reviewer` agent, like the other suites, run after each slice.
- **First commit**: nothing is committed yet. Say when you want the docs and skeleton committed locally.
