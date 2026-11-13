//! Picking the comments worth an agent's attention out of a long thread, inside a token budget.
//!
//! Greedy by a visible priority: query matches beat votes, the original poster's follow-ups are kept,
//! every chosen comment brings its ancestors, and everything left out is counted.

use crate::model::{Comment, State, Thread};
use crate::rank::{count, has_resolution_marker};
use crate::text;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Options {
    /// Token budget for the whole digest.
    pub budget: usize,
    /// Lower-cased query terms; comments containing them are kept regardless of score.
    pub terms: Vec<String>,
    /// A comment to show in full whatever its score (from a comment permalink or `--comment`).
    pub focus: Option<String>,
    /// Show every loaded comment in full.
    pub full: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { budget: 5000, terms: Vec::new(), focus: None, full: false }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// Index into `Thread::comments`.
    pub index: usize,
    /// Body is cut to this many characters.
    pub cut_to: Option<usize>,
    pub tags: Vec<String>,
    /// Present only so a chosen reply can be understood.
    pub context_only: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Omitted {
    pub moderator_notices: usize,
    pub bots: usize,
    pub gone: usize,
    pub below_line: usize,
    pub not_loaded: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    pub post_cut_to: Option<usize>,
    /// In thread order.
    pub shown: Vec<Shown>,
    pub omitted: Omitted,
}

#[derive(PartialEq, Clone, Copy)]
enum Noise {
    None,
    Moderator,
    Bot,
    Gone,
}

fn noise(c: &Comment) -> Noise {
    if c.state != State::Live {
        return Noise::Gone;
    }
    let author = c.author.as_deref().unwrap_or("").to_lowercase();
    if author == "automoderator" || (c.stickied && c.distinguished.as_deref() == Some("moderator")) {
        return Noise::Moderator;
    }
    if author.ends_with("_bot") || author.ends_with("-bot") || c.body.to_lowercase().contains("i am a bot") {
        return Noise::Bot;
    }
    Noise::None
}

const BODY_CAP: usize = 1200;
const CONTEXT_CAP: usize = 200;
const HEADER_TOKENS: usize = 16;
const DISSENT: [&str; 10] =
    ["this is wrong", "not true", "don't do this", "do not do this", "careful", "incorrect", "outdated", "bad advice", "wrong", "dangerous"];

fn chars(c: &Comment) -> usize {
    c.body.chars().count()
}

/// A short, low-score remark that matches nothing asked for ("same here", "lol"). It may also be a real short
/// answer ("Update the BIOS."), so it is only ever put last, never dropped by rule.
fn filler(c: &Comment, o: &Options) -> bool {
    let lower = c.body.to_lowercase();
    chars(c) < 80 && !c.is_op && c.score.unwrap_or(0) < 5 && !o.terms.iter().any(|t| count(&lower, t) > 0)
}

fn priority(c: &Comment, o: &Options) -> (f64, Vec<String>) {
    let lower = c.body.to_lowercase();
    let len = chars(c);
    let mut p = 0.6 * (1.0 + c.score.unwrap_or(0).max(0) as f64).ln();
    p += if len < 80 { -0.5 } else { len.min(1500) as f64 / 1500.0 };
    let matched: Vec<&String> = o.terms.iter().filter(|t| count(&lower, t) > 0).collect();
    // A model number or a whole phrase is strong evidence; one common word ("memory") is weak.
    p += matched.iter().map(|t| if text::is_identifier(t) || t.contains(' ') { 4.0 } else { 1.5 }).sum::<f64>();
    if c.is_op {
        p += 1.5;
        if has_resolution_marker(&c.body) {
            p += 2.0;
        }
    }
    if c.depth == 0 {
        p += 1.0;
    }
    if lower.contains("http") || lower.contains("```") || c.body.chars().any(|ch| ch.is_ascii_digit()) {
        p += 0.3;
    }
    if o.focus.as_deref() == Some(c.id.as_str()) {
        p += 1000.0;
    }
    // In the label, a matched phrase stands for its own words.
    let phrases: Vec<&&String> = matched.iter().filter(|t| t.contains(' ')).collect();
    let named: Vec<String> =
        matched.iter().filter(|t| t.contains(' ') || !phrases.iter().any(|p| p.split(' ').any(|w| w == t.as_str()))).map(|t| format!("\"{t}\"")).collect();
    let tags = if named.is_empty() { Vec::new() } else { vec![format!("matches: {}", named.join(", "))] };
    (p, tags)
}

pub fn select(t: &Thread, o: &Options) -> Selection {
    let n = t.comments.len();
    let index: HashMap<&str, usize> = t.comments.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let parent = |i: usize| t.comments[i].parent_id.as_deref().and_then(|p| index.get(p).copied());
    // Walks are bounded by the comment count, so a malformed parent loop cannot hang the selection.
    let root = |mut i: usize| {
        for _ in 0..n {
            match parent(i) {
                Some(p) => i = p,
                None => break,
            }
        }
        i
    };
    let focused = |i: usize| o.focus.as_deref() == Some(t.comments[i].id.as_str());
    let kinds: Vec<Noise> = t.comments.iter().map(noise).collect();
    let budget = o.budget.max(300);

    let post_chars = t.post.selftext.chars().count();
    let post_cap = budget * 4 / 10 * 3;
    let post_cut_to = (!o.full && post_chars > post_cap).then_some(post_cap);
    let mut used = text::tokens(post_cut_to.unwrap_or(post_chars)) + 120;

    // None = not shown; Some(None) = shown in full; Some(Some(n)) = cut to n characters.
    let mut cut: Vec<Option<Option<usize>>> = vec![None; n];
    let mut merit = vec![false; n];
    let mut tags: Vec<Vec<String>> = vec![Vec::new(); n];

    if o.full {
        for i in 0..n {
            if kinds[i] != Noise::Gone {
                (cut[i], merit[i]) = (Some(None), true);
                tags[i] = priority(&t.comments[i], o).1;
            }
        }
    } else {
        // Filler sorts after everything else and is taken only while at least half the budget is still free,
        // so a small thread shows its short answers and a long one does not spend room on "same here".
        let mut order: Vec<(usize, f64, bool)> = (0..n)
            .filter(|i| kinds[*i] == Noise::None || (focused(*i) && kinds[*i] != Noise::Gone))
            .map(|i| (i, priority(&t.comments[i], o).0, filler(&t.comments[i], o) && !focused(i)))
            .collect();
        order.sort_by(|a, b| a.2.cmp(&b.2).then(b.1.total_cmp(&a.1)).then(a.0.cmp(&b.0)));
        let branch_cap = budget * 35 / 100;
        let mut branch_used: HashMap<usize, usize> = HashMap::new();
        let mut misses = 0;
        for (i, _, is_filler) in order {
            if is_filler && cut[i].is_none() && used > budget / 2 {
                continue;
            }
            let c = &t.comments[i];
            let focused = focused(i);
            let own_cut = (!focused && chars(c) > BODY_CAP).then_some(BODY_CAP);
            let own = HEADER_TOKENS + text::tokens(own_cut.unwrap_or(chars(c)));
            // A comment already shown as short context is charged only for what showing it properly adds.
            let mut cost = match cut[i] {
                None => own,
                Some(shown) => own.saturating_sub(HEADER_TOKENS + text::tokens(shown.unwrap_or(usize::MAX).min(chars(c)))),
            };
            let mut chain = Vec::new();
            let mut up = parent(i);
            for _ in 0..n {
                let Some(p) = up else { break };
                if cut[p].is_none() {
                    chain.push(p);
                    cost += HEADER_TOKENS + text::tokens(chars(&t.comments[p]).min(CONTEXT_CAP));
                }
                up = parent(p);
            }
            let branch = root(i);
            let branch_total = branch_used.get(&branch).copied().unwrap_or(0);
            let fits = used + cost <= budget && (branch_total == 0 || branch_total + cost <= branch_cap);
            if !fits && !focused {
                misses += 1;
                if misses > 25 {
                    break;
                }
                continue;
            }
            (cut[i], merit[i]) = (Some(own_cut), true);
            tags[i] = priority(c, o).1;
            for p in chain {
                let live = kinds[p] != Noise::Gone;
                cut[p] = Some((live && chars(&t.comments[p]) > CONTEXT_CAP).then_some(CONTEXT_CAP));
            }
            used += cost;
            *branch_used.entry(branch).or_insert(0) += cost;
        }

        // Corrections usually sit one level below a popular answer: keep the best disputing reply to each chosen comment.
        let mut replies: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            if let Some(p) = parent(i) {
                replies.entry(p).or_default().push(i);
            }
        }
        for p in (0..n).filter(|i| merit[*i]).collect::<Vec<_>>() {
            let best = replies.get(&p).into_iter().flatten().copied().filter(|r| cut[*r].is_none() && kinds[*r] == Noise::None).filter(|r| {
                let head: String = t.comments[*r].body.chars().take(200).collect::<String>().to_lowercase();
                DISSENT.iter().any(|d| count(&head, d) > 0)
            });
            if let Some(r) = best.max_by_key(|r| t.comments[*r].score.unwrap_or(0)) {
                let own_cut = (chars(&t.comments[r]) > BODY_CAP).then_some(BODY_CAP);
                let cost = HEADER_TOKENS + text::tokens(own_cut.unwrap_or(chars(&t.comments[r])));
                if used + cost <= budget + budget / 10 {
                    (cut[r], merit[r]) = (Some(own_cut), true);
                    tags[r] = priority(&t.comments[r], o).1;
                    used += cost;
                }
            }
        }
    }
    // A heuristic label, never a fact: a chosen reply that opens by disputing its parent.
    for i in 0..n {
        if merit[i] && parent(i).is_some() {
            let head = t.comments[i].body.chars().take(200).collect::<String>().to_lowercase();
            if DISSENT.iter().any(|d| count(&head, d) > 0) {
                tags[i].push("possible-correction".into());
            }
        }
    }

    let mut omitted = Omitted { not_loaded: t.more.iter().map(|m| m.count.max(m.children.len() as i64)).sum(), ..Omitted::default() };
    let mut shown = Vec::new();
    for i in 0..n {
        match (cut[i], kinds[i]) {
            (Some(c), _) => shown.push(Shown { index: i, cut_to: c, tags: std::mem::take(&mut tags[i]), context_only: !merit[i] }),
            (None, Noise::Moderator) => omitted.moderator_notices += 1,
            (None, Noise::Bot) => omitted.bots += 1,
            (None, Noise::Gone) => omitted.gone += 1,
            (None, Noise::None) => omitted.below_line += 1,
        }
    }
    Selection { post_cut_to, shown, omitted }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{comment, post};

    fn long(word: &str, n: usize) -> String {
        format!("{word} ").repeat(n)
    }

    fn thread() -> Thread {
        let mut c = vec![
            comment("top", None, 0, &long("Update the BIOS to the newest release before anything else.", 3), 50),
            comment("thanks", Some("top"), 1, "thanks", 2),
            comment("fix", Some("thanks"), 2, "Same board here: B650E-E needed 3040 and a CMOS clear.", 1),
            comment("wrong", Some("top"), 1, &long("Careful, the newest release bricked two boards for me.", 3), 8),
            comment("auto", None, 0, "Please read the rules.", 1),
            comment("gone", None, 0, "", 4),
            comment("under", Some("gone"), 1, &long("Reply to a removed comment that mentions b650e-e again.", 2), 3),
            comment("meh", None, 0, "lol", 0),
            comment("opfix", Some("top"), 1, "That did it, thanks. It was the BIOS.", 2),
        ];
        c[4].author = Some("AutoModerator".into());
        c[5].state = State::Removed;
        c[5].author = None;
        c[8].is_op = true;
        Thread {
            post: post("p1", "B650E-E boot loop", "Will not POST."),
            comments: c,
            more: vec![crate::model::MoreStub { parent_id: Some("top".into()), count: 7, children: vec![] }],
        }
    }

    fn ids<'a>(t: &'a Thread, s: &Selection) -> Vec<&'a str> {
        s.shown.iter().map(|x| t.comments[x.index].id.as_str()).collect()
    }

    #[test]
    fn keeps_matches_op_follow_ups_corrections_and_ancestors() {
        let t = thread();
        let s = select(&t, &Options { terms: vec!["b650e-e".into()], ..Options::default() });
        let shown = ids(&t, &s);
        for want in ["top", "fix", "thanks", "wrong", "opfix", "gone", "under"] {
            assert!(shown.contains(&want), "{want} missing from {shown:?}");
        }
        assert!(!shown.contains(&"auto"));
        let by = |id: &str| s.shown.iter().find(|x| t.comments[x.index].id == id).unwrap();
        assert!(by("fix").tags[0].contains("b650e-e"), "a one-point comment naming the board is kept");
        assert!(by("wrong").tags.contains(&"possible-correction".to_owned()));
        assert!(by("gone").context_only, "a removed parent is kept only as a placeholder");
        assert_eq!(s.omitted.moderator_notices, 1);
        assert_eq!(s.omitted.not_loaded, 7);
        assert_eq!(shown, t.comments.iter().map(|c| c.id.as_str()).filter(|i| shown.contains(i)).collect::<Vec<_>>(), "thread order is preserved");
    }

    #[test]
    fn every_comment_is_shown_or_counted() {
        let t = thread();
        for budget in [300, 600, 5000] {
            let s = select(&t, &Options { budget, ..Options::default() });
            let o = &s.omitted;
            assert_eq!(s.shown.len() + o.moderator_notices + o.bots + o.gone + o.below_line, t.comments.len(), "budget {budget}");
        }
    }

    #[test]
    fn budget_holds_and_long_bodies_are_cut() {
        let mut t = thread();
        for i in 0..60 {
            t.comments.push(comment(&format!("n{i}"), None, 0, &long("A long and detailed answer about memory training times.", 40), 20 - i as i64 / 4));
        }
        let o = Options { budget: 2000, ..Options::default() };
        let s = select(&t, &o);
        let spent: usize = s.shown.iter().map(|x| HEADER_TOKENS + text::tokens(x.cut_to.unwrap_or(t.comments[x.index].body.chars().count()))).sum();
        assert!(spent <= 2000 + 200, "spent {spent}");
        assert!(s.shown.iter().any(|x| x.cut_to == Some(BODY_CAP)));
        assert!(s.omitted.below_line > 40);
    }

    #[test]
    fn focus_comment_is_shown_in_full_even_when_low() {
        let mut t = thread();
        t.comments[7].body = long("buried detail", 300);
        let s = select(&t, &Options { budget: 300, focus: Some("meh".into()), ..Options::default() });
        let meh = s.shown.iter().find(|x| t.comments[x.index].id == "meh").unwrap();
        assert_eq!(meh.cut_to, None);
    }

    #[test]
    fn full_shows_all_live_comments_uncut() {
        let t = thread();
        let s = select(&t, &Options { full: true, ..Options::default() });
        assert_eq!(s.shown.len(), 8, "asking for everything includes moderator notices");
        assert!(s.shown.iter().all(|x| x.cut_to.is_none()));
        assert_eq!((s.omitted.gone, s.omitted.moderator_notices, s.omitted.below_line), (1, 0, 0));
    }

    #[test]
    fn upgrading_a_context_ancestor_is_charged_so_reply_chains_cannot_blow_the_budget() {
        let mut t = thread();
        t.comments.clear();
        for i in 0..40 {
            t.comments.push(comment(&format!("a{i}"), None, 0, &long("A long answer with plenty of detail in it.", 30), 5));
            let mut reply = comment(&format!("r{i}"), Some(&format!("a{i}")), 1, "That did it, it was the b650e-e BIOS.", 1);
            reply.is_op = true;
            t.comments.push(reply);
        }
        let s = select(&t, &Options { budget: 3000, terms: vec!["b650e-e".into()], ..Options::default() });
        let printed: usize =
            s.shown.iter().map(|x| HEADER_TOKENS + text::tokens(x.cut_to.unwrap_or(usize::MAX).min(t.comments[x.index].body.chars().count()))).sum();
        assert!(printed <= 3000 + 300, "printed {printed} tokens against a budget of 3000");
    }

    #[test]
    fn names_ending_in_bot_are_people_and_a_requested_notice_is_shown() {
        let mut t = thread();
        t.comments[0].author = Some("Talbot".into());
        let s = select(&t, &Options { focus: Some("auto".into()), ..Options::default() });
        let shown = ids(&t, &s);
        assert!(shown.contains(&"top") && shown.contains(&"auto"), "{shown:?}");
        t.comments[7].author = Some("helper_bot".into());
        assert_eq!(select(&t, &Options::default()).omitted.bots, 1);
    }

    #[test]
    fn a_parent_loop_cannot_hang_selection() {
        let mut t = thread();
        t.comments = vec![comment("a", Some("b"), 0, "one", 1), comment("b", Some("a"), 0, "two", 1), comment("c", Some("c"), 0, "three", 1)];
        let s = select(&t, &Options::default());
        assert_eq!(s.shown.len() + s.omitted.below_line, 3);
    }

    #[test]
    fn filler_goes_last_and_is_dropped_only_when_room_is_short() {
        let mut t = thread();
        for i in 0..12 {
            t.comments.push(comment(&format!("n{i}"), None, 0, &long("A long and detailed answer about memory training times.", 12), 9));
        }
        let tight = select(&t, &Options { terms: vec!["b650e-e".into()], budget: 2500, ..Options::default() });
        let shown = ids(&t, &tight);
        assert!(!shown.contains(&"meh"), "a low-score 'lol' is not shown when the budget is mostly spent: {shown:?}");
        assert!(shown.contains(&"thanks"), "a short remark stays when a chosen reply hangs under it");

        // A small thread of short answers, no focus words: everything is shown.
        let mut small = thread();
        small.comments = vec![comment("a", None, 0, "Update the BIOS.", 3), comment("b", None, 0, "Reseat the RAM.", 1)];
        let s = select(&small, &Options::default());
        assert_eq!(ids(&small, &s), ["a", "b"]);
        assert_eq!(s.omitted.below_line, 0);
    }

    #[test]
    fn a_whole_phrase_outweighs_one_common_word() {
        let mut t = thread();
        t.comments = vec![
            comment("word", None, 0, &long("Memory prices are up again this year, sadly.", 3), 40),
            comment("phrase", None, 0, &long("Enable memory context restore in the BIOS.", 3), 1),
        ];
        let o = Options { terms: vec!["memory context restore".into(), "memory".into(), "context".into(), "restore".into()], ..Options::default() };
        assert!(priority(&t.comments[1], &o).0 > priority(&t.comments[0], &o).0);
        assert_eq!(priority(&t.comments[1], &o).1, ["matches: \"memory context restore\""], "the phrase stands for its words in the label");
        assert_eq!(priority(&t.comments[0], &o).1, ["matches: \"memory\""]);
    }

    #[test]
    fn long_post_is_cut_to_its_share() {
        let mut t = thread();
        t.post.selftext = long("word", 5000);
        let s = select(&t, &Options { budget: 1000, ..Options::default() });
        assert_eq!(s.post_cut_to, Some(1200));
    }
}
