//! `rdt`: turns Reddit thread URLs into citable text for a research agent, with a local cache.
//!
//! Exit codes: 0 ok, 2 bad usage, 3 nothing found, 4 no source available, 5 rate limited, 6 source error.

mod run;
mod ui;

use clap::{Parser, Subcommand};
use reddit_core::store::Store;
use reddit_sources::arctic::Arctic;
use reddit_sources::transport::{Https, SystemClock};
use run::{Ctx, Fail, SearchArgs, SubsArgs, ThreadArgs};
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "rdt", version, about = "Reddit threads as citable text for research, from the Arctic Shift archive, with a local cache")]
struct Cli {
    /// If the archive is rate limiting, wait up to 90 seconds for it and try once more, instead of stopping with exit code 5.
    #[arg(long, global = true)]
    wait: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check that the cache and HTTPS work on this machine. Sends nothing.
    Doctor,
    /// Show whether the archive is reachable and current, and what the cache holds. One request.
    Sources,
    /// Print a digest of one thread: the post and the comments worth reading.
    Thread {
        /// A Reddit thread or comment URL, `t3_` id or bare id.
        reference: String,
        /// Token budget for the digest.
        #[arg(long, default_value_t = 5000)]
        budget: usize,
        /// Words to look for: comments containing them are kept whatever their score.
        #[arg(long)]
        focus: Option<String>,
        /// Show this comment in full.
        #[arg(long)]
        comment: Option<String>,
        /// Load the replies under this comment that the first fetch left out.
        #[arg(long)]
        expand: Option<String>,
        /// How many comments to ask the archive for.
        #[arg(long, default_value_t = 500)]
        limit: usize,
        /// Fetch again even if a recent copy is cached.
        #[arg(long)]
        refresh: bool,
        /// Use the cache only.
        #[arg(long)]
        offline: bool,
        /// Show every loaded comment in full.
        #[arg(long)]
        full: bool,
        #[arg(long)]
        json: bool,
    },
    /// Turn URLs into thread ids, drop duplicates, and say which are cached. Offline. `-` reads stdin.
    Resolve {
        refs: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Search chosen subreddits in the archive, plus the cache, and rank the results.
    Search {
        query: String,
        /// Subreddits to search, comma separated. The archive cannot search all of Reddit.
        #[arg(long, value_delimiter = ',')]
        sub: Vec<String>,
        /// Also search the subreddits these Reddit URLs point into (as found by a web search). Comma separated.
        #[arg(long, value_delimiter = ',')]
        from_urls: Vec<String>,
        /// Only posts on or after this date (YYYY-MM-DD).
        #[arg(long)]
        after: Option<String>,
        /// Only posts before this date (YYYY-MM-DD).
        #[arg(long)]
        before: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Also search comment text in the archive (slower; one more request per subreddit).
        #[arg(long)]
        comments: bool,
        /// How fast the topic goes stale: fast (about 6 months), normal (about 2 years), timeless.
        #[arg(long, default_value = "normal")]
        pace: String,
        #[arg(long)]
        offline: bool,
        /// Include posts marked over-18.
        #[arg(long)]
        nsfw: bool,
        /// Show how each result was scored.
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        json: bool,
    },
    /// Suggest which subreddits to search for a topic, from the Reddit URLs a web search found. `-` reads stdin.
    Subs {
        topic: String,
        /// Reddit URLs (any kind) that a web search returned for the topic.
        urls: Vec<String>,
        /// Do not ask the archive for sizes.
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        nsfw: bool,
        #[arg(long)]
        json: bool,
    },
    /// Search the cache only. Offline.
    Local {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value = "normal")]
        pace: String,
        #[arg(long)]
        nsfw: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        json: bool,
    },
    /// Expand cached ids into full citations with permalinks. Offline.
    Cite { ids: Vec<String> },
    /// Check that a quote appears word for word in a cached post or comment. Offline.
    VerifyQuote { id: String, quote: String },
    /// Open a read-only window onto the cache in your browser, served on this machine only.
    Ui {
        #[arg(long, default_value_t = ui::DEFAULT_PORT)]
        port: u16,
    },
    /// Inspect or trim the cache.
    Cache {
        #[command(subcommand)]
        cmd: CacheCmd,
    },
}

#[derive(Subcommand)]
enum CacheCmd {
    /// Counts and age of what is cached.
    Stats,
    /// Ask the archive about every cached item and blank any that were removed or deleted since.
    Check,
    /// Delete threads not fetched within this many days.
    Purge {
        #[arg(long)]
        older_than_days: u32,
    },
    /// Delete one thread from the cache.
    Forget { id: String },
}

fn db_path() -> PathBuf {
    if let Some(p) = std::env::var_os("RDT_DB") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("rdt").join("cache.sqlite")
}

fn main() {
    let cli = Cli::parse();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let nonce = format!("{:06x}", (nanos ^ std::process::id().rotate_left(13)) & 0xff_ffff);
    let json = matches!(
        &cli.cmd,
        Cmd::Thread { json: true, .. }
            | Cmd::Resolve { json: true, .. }
            | Cmd::Search { json: true, .. }
            | Cmd::Local { json: true, .. }
            | Cmd::Subs { json: true, .. }
    );

    let path = db_path();
    let result = (|| -> Result<String, Fail> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Fail::new(6, format!("cannot create {}: {e}", dir.display())))?;
        }
        let store = Store::open(&path).map_err(|e| Fail::new(6, format!("cannot open the cache at {}: {e}", path.display())))?;
        // The archive's allowance is small, and an agent may run this tool many times in a row: wait out the gap
        // since the previous run's last request before making any new one.
        if store.last_request_at().ok().flatten().is_some_and(|at| (0..2).contains(&(now - at))) {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        let arctic = Arctic::new(Https::default(), SystemClock::default());
        if cli.wait {
            arctic.set_max_wait(90);
        }
        let mut ctx = Ctx { store, arctic: &arctic, now, nonce: &nonce };
        let out = match cli.cmd {
            Cmd::Doctor => run::doctor(&ctx, &path),
            Cmd::Sources => run::sources(&ctx),
            Cmd::Thread { reference, budget, focus, comment, expand, limit, refresh, offline, full, json } => {
                run::thread(&mut ctx, &ThreadArgs { reference, budget, focus, comment, expand, limit, refresh, offline, full, json })
            }
            Cmd::Resolve { mut refs, json } => {
                if refs.iter().any(|r| r == "-") {
                    let mut stdin = String::new();
                    std::io::stdin().read_to_string(&mut stdin).map_err(|e| Fail::new(2, format!("cannot read stdin: {e}")))?;
                    refs.retain(|r| r != "-");
                    refs.extend(stdin.split_whitespace().map(str::to_owned));
                }
                run::resolve(&ctx, &refs, json)
            }
            Cmd::Subs { topic, mut urls, offline, nsfw, json } => {
                if urls.iter().any(|u| u == "-") {
                    let mut stdin = String::new();
                    std::io::stdin().read_to_string(&mut stdin).map_err(|e| Fail::new(2, format!("cannot read stdin: {e}")))?;
                    urls.retain(|u| u != "-");
                    // Piped text may carry titles and snippets: only tokens that are plainly Reddit addresses count.
                    let reddit = |t: &&str| t.to_ascii_lowercase().contains("reddit.com/") || t.starts_with("r/") || t.starts_with("/r/");
                    urls.extend(stdin.split_whitespace().filter(reddit).map(str::to_owned));
                }
                run::subs(&mut ctx, &SubsArgs { topic, urls, offline, nsfw, json })
            }
            Cmd::Search { query, mut sub, from_urls, after, before, limit, comments, pace, offline, nsfw, explain, json } => {
                for name in from_urls.iter().filter_map(|u| reddit_core::ids::subreddit_of(u)) {
                    if !sub.iter().any(|s| s.trim().trim_start_matches("r/").eq_ignore_ascii_case(&name)) {
                        sub.push(name);
                    }
                }
                run::search(&mut ctx, &SearchArgs { query, subs: sub, after, before, limit, comments, pace, offline, nsfw, explain, json })
            }
            Cmd::Local { query, limit, pace, nsfw, explain, json } => run::search(
                &mut ctx,
                &SearchArgs { query, subs: Vec::new(), after: None, before: None, limit, comments: false, pace, offline: true, nsfw, explain, json },
            ),
            Cmd::Ui { port } => ui::serve(&mut ctx, port),
            Cmd::Cite { ids } => run::cite(&ctx, &ids),
            Cmd::VerifyQuote { id, quote } => run::verify_quote(&ctx, &id, &quote),
            Cmd::Cache { cmd } => match cmd {
                CacheCmd::Stats => run::cache_stats(&ctx),
                CacheCmd::Check => run::cache_check(&mut ctx),
                CacheCmd::Purge { older_than_days } => run::cache_purge(&mut ctx, older_than_days),
                CacheCmd::Forget { id } => run::cache_forget(&mut ctx, &id),
            },
        };
        ctx.flush_log();
        out
    })();

    match result {
        Ok(text) => print!("{text}"),
        Err(f) => {
            if json {
                println!("{}", serde_json::json!({"ok": false, "code": f.code, "error": f.message, "retry_after_s": f.retry_after_s}));
            }
            eprintln!("rdt: {}", f.message);
            std::process::exit(f.code);
        }
    }
}
