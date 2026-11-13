//! Ordering candidate threads: text relevance first, bounded priors second, rank fusion as a nudge.
//!
//! The weights are reasoned starting values, not tuned on data. They are few and all visible in `Ranked`
//! so `--explain` can show them.

use crate::model::{Post, State, Thread};
use crate::text;
use std::collections::HashMap;

/// Occurrences of `needle` in `hay` that start at a word boundary. Both must already be lower-case.
/// Prefix matching stands in for stemming: "undervolt" finds "undervolting", "ram" does not find "program".
pub fn count(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut n = 0;
    let mut from = 0;
    while let Some(pos) = hay[from..].find(needle) {
        let start = from + pos;
        if hay[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric()) {
            n += 1;
        }
        from = start + needle.len();
    }
    n
}

fn saturate(tf: usize) -> f64 {
    tf as f64 / (tf as f64 + 1.2)
}

/// Title counts three times the body; quoted phrases and terms that name a specific thing (they contain a digit) count double;
/// the total is scaled by the share of terms matched, so a thread matching every term beats one that repeats a few.
pub fn text_score(terms: &[String], title: &str, body: &str) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let (title, body) = (title.to_lowercase(), body.to_lowercase());
    let mut sum = 0.0;
    let mut matched = 0;
    for t in terms {
        let (in_title, in_body) = (count(&title, t), count(&body, t));
        if in_title + in_body > 0 {
            matched += 1;
        }
        let weight = if text::is_identifier(t) || t.contains(' ') { 2.0 } else { 1.0 };
        sum += weight * (3.0 * saturate(in_title) + saturate(in_body));
    }
    sum * matched as f64 / terms.len() as f64
}

/// Between 0.6 and 1.5: popularity and recency may reorder close calls, never overturn a clear text match.
/// `half_life_days` of `None` means the topic is timeless.
pub fn prior(post: &Post, now: i64, half_life_days: Option<f64>, resolved: bool) -> f64 {
    let popularity = ((post.score.unwrap_or(0).max(0) as f64 + 1.0).log10() + (post.num_comments.unwrap_or(0).max(0) as f64 + 1.0).log10()) / 6.0;
    let age_days = (now - post.created).max(0) as f64 / 86_400.0;
    let recency = half_life_days.map_or(1.0, |h| 0.5 + 0.5 * 0.5f64.powf(age_days / h));
    let mut p = (0.8 + 0.4 * popularity.min(1.0)) * (0.7 + 0.3 * recency);
    if resolved {
        p *= 1.15;
    }
    if post.state != State::Live && post.num_comments.unwrap_or(0) < 3 {
        p *= 0.7;
    }
    p.clamp(0.6, 1.5)
}

/// Reciprocal Rank Fusion (Cormack et al. 2009, k = 60): needs only ranks, so lists with no comparable scores can be merged.
pub fn rrf(lists: &[Vec<String>]) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *out.entry(id.clone()).or_insert(0.0) += 1.0 / (60.0 + rank as f64 + 1.0);
        }
    }
    out
}

const RESOLVED: [&str; 12] = [
    "solved",
    "fixed it",
    "this fixed",
    "this worked",
    "that fixed",
    "that worked",
    "that did it",
    "problem was",
    "issue was",
    "culprit was",
    "turns out",
    "resolved",
];

pub fn has_resolution_marker(s: &str) -> bool {
    let lower = s.to_lowercase();
    RESOLVED.iter().any(|m| count(&lower, m) > 0)
}

/// Where the original poster (or the flair) says the problem was solved: `"post"`, `"flair"` or a comment id.
pub fn resolved_by(t: &Thread) -> Option<String> {
    if t.post.flair.as_deref().is_some_and(has_resolution_marker) {
        return Some("flair".into());
    }
    if has_resolution_marker(&t.post.selftext) {
        return Some("post".into());
    }
    t.comments.iter().find(|c| c.is_op && has_resolution_marker(&c.body)).map(|c| c.id.clone())
}

/// One thread found by one or more searches.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub post: Post,
    /// Texts of cached or found comments that matched, with their ids.
    pub comments: Vec<(String, String)>,
    pub cached_comments: bool,
}

#[derive(Debug, Clone)]
pub struct Ranked {
    pub post: Post,
    pub score: f64,
    pub text: f64,
    pub prior: f64,
    pub rrf: f64,
    /// Which lists found it and at what rank, e.g. `arctic:AMDHelp#2`.
    pub via: Vec<String>,
    pub matched: Vec<String>,
    pub snippet: String,
    /// Other threads folded into this one as duplicates, e.g. `r/x t3_abc`.
    pub also_in: Vec<String>,
    pub resolved: bool,
    pub cached_comments: bool,
}

/// Byte offset in `hay` (always a character boundary) where the lower-case `needle` first matches ignoring case.
fn find_ignoring_case(hay: &str, needle: &str) -> Option<usize> {
    hay.char_indices().map(|(i, _)| i).find(|i| {
        let mut rest = hay[*i..].chars().flat_map(char::to_lowercase);
        !needle.is_empty() && needle.chars().all(|n| rest.next() == Some(n))
    })
}

/// About 160 characters around the first query term found.
pub fn snippet(terms: &[String], s: &str) -> String {
    let flat = text::collapse_ws(s);
    let hit = terms.iter().filter_map(|t| find_ignoring_case(&flat, t)).min().unwrap_or(0);
    let start = flat[..hit].char_indices().rev().nth(50).map_or(0, |(i, _)| i);
    let window: String = flat[start..].chars().take(160).collect();
    format!("{}{}{}", if start > 0 { "…" } else { "" }, window.trim(), if flat[start..].chars().count() > 160 { "…" } else { "" })
}

fn norm_title(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `lists` are (label, ids in rank order). Duplicates fold into whichever copy has more comments.
pub fn rank(candidates: Vec<Candidate>, lists: &[(String, Vec<String>)], terms: &[String], now: i64, half_life_days: Option<f64>) -> Vec<Ranked> {
    let fused = rrf(&lists.iter().map(|(_, ids)| ids.clone()).collect::<Vec<_>>());
    let mut ranked: Vec<Ranked> = candidates
        .into_iter()
        .map(|c| {
            let post_text = text_score(terms, &c.post.title, &c.post.selftext);
            let mut comment_scores: Vec<f64> = c.comments.iter().map(|(_, body)| text_score(terms, "", body)).collect();
            comment_scores.sort_by(|a, b| b.total_cmp(a));
            let comment_text: f64 = comment_scores.iter().take(3).sum::<f64>() * 0.5;
            let resolved = has_resolution_marker(&c.post.selftext) || c.post.flair.as_deref().is_some_and(has_resolution_marker);
            let prior = prior(&c.post, now, half_life_days, resolved);
            let rrf = fused.get(&c.post.id).copied().unwrap_or(0.0);
            let mut matched = Vec::new();
            let (title, body) = (c.post.title.to_lowercase(), c.post.selftext.to_lowercase());
            if terms.iter().any(|t| count(&title, t) > 0) {
                matched.push("title".to_owned());
            }
            if terms.iter().any(|t| count(&body, t) > 0) {
                matched.push("post".to_owned());
            }
            let hits = comment_scores.iter().filter(|s| **s > 0.0).count();
            if hits > 0 {
                matched.push(format!("{hits} comment{}", if hits == 1 { "" } else { "s" }));
            }
            let best_comment = c.comments.iter().max_by(|a, b| text_score(terms, "", &a.1).total_cmp(&text_score(terms, "", &b.1)));
            let snippet = match best_comment {
                Some((id, body)) if post_text == 0.0 || c.post.selftext.is_empty() => format!("[t1_{id}] {}", snippet(terms, body)),
                _ => snippet(terms, &c.post.selftext),
            };
            let via = lists.iter().filter_map(|(label, ids)| ids.iter().position(|i| *i == c.post.id).map(|r| format!("{label}#{}", r + 1))).collect();
            let text = post_text + comment_text;
            Ranked {
                score: text * prior + 10.0 * rrf,
                text,
                prior,
                rrf,
                via,
                matched,
                snippet,
                also_in: Vec::new(),
                resolved,
                cached_comments: c.cached_comments,
                post: c.post,
            }
        })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| b.post.created.cmp(&a.post.created)));

    // Fold duplicates: a cross-post onto its parent, same outbound link, or same author and title.
    let mut keep: Vec<Ranked> = Vec::new();
    for r in ranked {
        let same = |k: &Ranked| {
            k.post.crosspost_parent.as_deref() == Some(r.post.id.as_str())
                || r.post.crosspost_parent.as_deref() == Some(k.post.id.as_str())
                || (r.post.crosspost_parent.is_some() && r.post.crosspost_parent == k.post.crosspost_parent)
                || (r.post.url.is_some() && r.post.url == k.post.url)
                || (r.post.author.is_some() && r.post.author == k.post.author && norm_title(&r.post.title) == norm_title(&k.post.title))
        };
        match keep.iter_mut().find(|k| same(k)) {
            Some(k) => {
                let label = |p: &Post| format!("r/{} t3_{} ({} comments)", p.subreddit, p.id, p.num_comments.unwrap_or(0));
                if r.post.num_comments.unwrap_or(0) > k.post.num_comments.unwrap_or(0) {
                    let mut also = std::mem::take(&mut k.also_in);
                    also.push(label(&k.post));
                    let score = k.score;
                    *k = r;
                    k.score = score;
                    k.also_in = also;
                } else {
                    k.also_in.push(label(&r.post));
                }
            }
            None => keep.push(r),
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{comment, post};

    fn terms(q: &str) -> Vec<String> {
        text::terms(q)
    }

    fn cand(p: Post) -> Candidate {
        Candidate { post: p, comments: Vec::new(), cached_comments: false }
    }

    #[test]
    fn word_boundary_prefix_matching() {
        assert_eq!(count("my program has ram and ramp", "ram"), 2);
        assert_eq!(count("undervolting helped", "undervolt"), 1);
        assert_eq!(count("the b650e-e board, b650e-e again", "b650e-e"), 2);
        assert_eq!(count("anything", ""), 0);
    }

    #[test]
    fn exact_identifier_in_a_small_thread_beats_a_popular_vague_one() {
        let mut exact = post("a", "B650E-E boot loop after enabling EXPO", "Stuck on memory training.");
        exact.score = Some(12);
        let mut vague = post("b", "Boot loop help", "My PC has a boot loop, expo maybe?");
        vague.score = Some(9000);
        vague.num_comments = Some(4000);
        let out = rank(vec![cand(vague), cand(exact)], &[], &terms("B650E-E EXPO boot loop"), 1_700_100_000, Some(730.0));
        assert_eq!(out[0].post.id, "a");
        assert!(out[0].text > out[1].text && out[1].prior > out[0].prior);
    }

    #[test]
    fn prior_is_bounded_and_recency_matters_only_when_asked() {
        let mut p = post("a", "t", "b");
        p.score = Some(1_000_000);
        p.num_comments = Some(1_000_000);
        let now = p.created;
        assert!(prior(&p, now, Some(180.0), true) <= 1.5);
        let old = now + 86_400 * 3650;
        assert!(prior(&p, old, Some(180.0), false) < prior(&p, now, Some(180.0), false));
        assert_eq!(prior(&p, old, None, false), prior(&p, now, None, false));
        p.score = None;
        p.num_comments = None;
        assert!(prior(&p, old, Some(180.0), false) >= 0.6);
    }

    #[test]
    fn fusion_rewards_agreement_between_lists() {
        let f = rrf(&[vec!["a".into(), "b".into()], vec!["b".into(), "c".into()]]);
        assert!(f["b"] > f["a"] && f["a"] > f["c"]);
    }

    #[test]
    fn crossposts_and_reposts_fold_into_the_bigger_copy() {
        let mut parent = post("a", "Fix for coil whine", "undervolt");
        parent.num_comments = Some(3);
        let mut cross = post("b", "Fix for coil whine", "undervolt");
        cross.crosspost_parent = Some("a".into());
        cross.subreddit = "other".into();
        cross.num_comments = Some(40);
        let mut repost = post("c", "fix for COIL whine!", "undervolt");
        repost.num_comments = Some(1);
        let out = rank(vec![cand(parent), cand(cross), cand(repost)], &[], &terms("coil whine"), 1_700_100_000, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].post.id, "b");
        assert_eq!(out[0].also_in.len(), 2);
    }

    #[test]
    fn comment_matches_surface_a_thread_whose_post_does_not_match() {
        let c = Candidate {
            post: post("a", "Weird noise", "help"),
            comments: vec![("k1".into(), "That is coil whine, undervolt it.".into())],
            cached_comments: true,
        };
        let out = rank(vec![c], &[("cache".into(), vec!["a".into()])], &terms("coil whine"), 1_700_100_000, None);
        assert!(out[0].score > 0.0);
        assert_eq!(out[0].matched, ["1 comment"]);
        assert!(out[0].snippet.starts_with("[t1_k1]"));
        assert_eq!(out[0].via, ["cache#1"]);
    }

    #[test]
    fn resolution_markers() {
        let mut t = Thread {
            post: post("p1", "t", "no luck"),
            comments: vec![comment("c1", None, 0, "Try a BIOS update", 3), comment("c2", Some("c1"), 1, "That did it, thanks", 1)],
            more: vec![],
        };
        assert_eq!(resolved_by(&t), None, "only the original poster's word counts");
        t.comments[1].is_op = true;
        assert_eq!(resolved_by(&t).as_deref(), Some("c2"));
        t.post.flair = Some("Solved".into());
        assert_eq!(resolved_by(&t).as_deref(), Some("flair"));
        assert!(!has_resolution_marker("unresolvedness is not a word but unsolved is"));
    }

    #[test]
    fn snippet_survives_characters_whose_lower_case_has_a_different_length() {
        let s = snippet(&terms("coil"), "\u{212A} coil whine \u{0130}\u{0130}");
        assert!(s.contains("coil whine"));
        assert_eq!(snippet(&terms("whine"), "\u{0130}\u{0130}\u{0130} COIL WHINE"), "\u{0130}\u{0130}\u{0130} COIL WHINE");
        assert_eq!(snippet(&terms("absent"), "\u{0130}"), "\u{0130}");
    }

    #[test]
    fn snippet_centres_on_the_match() {
        let long = format!("{} the coil whine stopped {}", "x ".repeat(100), "y ".repeat(200));
        let s = snippet(&terms("coil whine"), &long);
        assert!(s.contains("coil whine") && s.starts_with('…') && s.ends_with('…') && s.chars().count() <= 162);
    }
}
