//! Posts, comments and collapsed-branch stubs, parsed leniently from Reddit-shaped JSON.
//!
//! Arctic Shift stores Reddit's own objects, so one parser reads both. Fields are pulled out of a
//! `serde_json::Value` by hand: a missing, null or oddly typed field never fails a whole thread.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Live,
    /// Deleted by its author.
    Deleted,
    /// Removed by moderators or Reddit.
    Removed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Live => "live",
            State::Deleted => "deleted",
            State::Removed => "removed",
        }
    }
    pub fn parse(s: &str) -> State {
        match s {
            "deleted" => State::Deleted,
            "removed" => State::Removed,
            _ => State::Live,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    /// Base-36 id without the `t3_` prefix.
    pub id: String,
    pub subreddit: String,
    pub title: String,
    pub selftext: String,
    pub author: Option<String>,
    pub score: Option<i64>,
    pub upvote_ratio: Option<f64>,
    pub num_comments: Option<i64>,
    pub created: i64,
    pub edited: Option<i64>,
    pub permalink: String,
    pub url: Option<String>,
    pub flair: Option<String>,
    pub crosspost_parent: Option<String>,
    pub over_18: bool,
    pub locked: bool,
    pub state: State,
    /// When the source last read the score (archives freeze scores).
    pub scores_as_of: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// Id without the `t1_` prefix.
    pub id: String,
    pub post_id: String,
    /// `None` for a top-level comment.
    pub parent_id: Option<String>,
    pub depth: u32,
    pub author: Option<String>,
    pub body: String,
    pub score: Option<i64>,
    pub created: i64,
    pub edited: Option<i64>,
    pub is_op: bool,
    pub distinguished: Option<String>,
    pub stickied: bool,
    pub state: State,
}

/// A branch the source did not load.
#[derive(Debug, Clone, PartialEq)]
pub struct MoreStub {
    /// Comment id the unloaded replies hang under; `None` means directly under the post.
    pub parent_id: Option<String>,
    pub count: i64,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    pub post: Post,
    /// Pre-order: every comment appears after its parent, in the order the source returned them.
    pub comments: Vec<Comment>,
    pub more: Vec<MoreStub>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Integers arrive as ints, floats (`1700000000.0`) or occasionally numeric strings.
fn int(v: &Value, key: &str) -> Option<i64> {
    match v.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `edited` is `false` when never edited and a timestamp when it was.
fn edited(v: &Value) -> Option<i64> {
    int(v, "edited").filter(|t| *t > 0)
}

fn strip<'a>(fullname: &'a str, prefix: &str) -> &'a str {
    fullname.strip_prefix(prefix).unwrap_or(fullname)
}

fn author(v: &Value) -> Option<String> {
    text(v, "author").filter(|a| a != "[deleted]" && !a.is_empty())
}

const REMOVED_BODIES: [&str; 4] = ["[removed]", "[ Removed by Reddit ]", "[ Removed by moderator ]", "[removed by reddit]"];

fn state(v: &Value, body: &str) -> State {
    let body = body.trim();
    match v.get("removed_by_category").and_then(Value::as_str) {
        Some("deleted") => return State::Deleted,
        Some(_) => return State::Removed,
        None => {}
    }
    if body == "[deleted]" {
        State::Deleted
    } else if REMOVED_BODIES.iter().any(|r| body.eq_ignore_ascii_case(r)) {
        State::Removed
    } else {
        State::Live
    }
}

/// Accepts a bare data object or a `{"kind": "t3", "data": {...}}` envelope.
fn unwrap(v: &Value) -> (&str, &Value) {
    match (v.get("kind").and_then(Value::as_str), v.get("data")) {
        (Some(kind), Some(data)) if data.is_object() => (kind, data),
        _ => ("", v),
    }
}

/// Text of anything deleted or removed is never kept, whatever the source still holds.
pub fn parse_post(v: &Value) -> Option<Post> {
    let (_, d) = unwrap(v);
    let id = text(d, "id")?;
    let selftext = text(d, "selftext").unwrap_or_default();
    let state = state(d, &selftext);
    let live = state == State::Live;
    Some(Post {
        subreddit: text(d, "subreddit").unwrap_or_default(),
        title: if live { text(d, "title").unwrap_or_default() } else { format!("[{}]", state.as_str()) },
        selftext: if live { selftext } else { String::new() },
        author: if live { author(d) } else { None },
        score: int(d, "score"),
        upvote_ratio: d.get("upvote_ratio").and_then(Value::as_f64),
        num_comments: int(d, "num_comments"),
        created: int(d, "created_utc").unwrap_or(0),
        edited: edited(d),
        permalink: text(d, "permalink").unwrap_or_else(|| format!("/comments/{id}/")),
        url: text(d, "url").filter(|u| live && !u.contains("/comments/")),
        flair: text(d, "link_flair_text").map(|f| f.trim().to_owned()).filter(|f| live && !f.is_empty()),
        crosspost_parent: text(d, "crosspost_parent").map(|p| strip(&p, "t3_").to_owned()),
        over_18: flag(d, "over_18"),
        locked: flag(d, "locked"),
        state,
        scores_as_of: d.get("_meta").and_then(|m| int(m, "retrieved_2nd_on")).or_else(|| int(d, "retrieved_on")),
        id,
    })
}

fn parse_comment(d: &Value, post_id: &str, depth: u32) -> Option<Comment> {
    let id = text(d, "id")?;
    let body = text(d, "body").unwrap_or_default();
    let state = state(d, &body);
    let live = state == State::Live;
    let parent = text(d, "parent_id").unwrap_or_default();
    Some(Comment {
        id,
        post_id: post_id.to_owned(),
        parent_id: parent.strip_prefix("t1_").map(str::to_owned),
        depth,
        author: if live { author(d) } else { None },
        body: if live { body } else { String::new() },
        score: int(d, "score"),
        created: int(d, "created_utc").unwrap_or(0),
        edited: edited(d),
        is_op: flag(d, "is_submitter"),
        distinguished: text(d, "distinguished"),
        stickied: flag(d, "stickied"),
        state,
    })
}

/// A subreddit as the archive lists it. Counts are updated infrequently there, so treat them as rough.
#[derive(Debug, Clone, PartialEq)]
pub struct Subreddit {
    pub name: String,
    pub subscribers: Option<i64>,
    pub over_18: bool,
    pub description: String,
    /// Posts the archive holds for it: the best available hint of how slow a text search will be.
    pub posts: Option<i64>,
}

pub fn parse_subreddit(v: &Value) -> Option<Subreddit> {
    let (_, d) = unwrap(v);
    Some(Subreddit {
        name: text(d, "display_name").filter(|n| !n.is_empty())?,
        subscribers: int(d, "subscribers"),
        over_18: flag(d, "over18") || flag(d, "over_18"),
        description: text(d, "public_description").or_else(|| text(d, "title")).unwrap_or_default(),
        posts: d.get("_meta").and_then(|m| int(m, "num_posts")),
    })
}

/// A comment outside any tree (a search hit or an id lookup); its thread comes from `link_id`.
pub fn parse_flat_comment(v: &Value) -> Option<Comment> {
    let (_, d) = unwrap(v);
    let post = text(d, "link_id")?;
    parse_comment(d, strip(&post, "t3_"), 0)
}

/// Flattens a nested comment tree (`replies` is `""` or a Listing) into pre-order, without recursion,
/// so a very deep thread cannot overflow the stack. Unknown kinds are skipped.
pub fn parse_tree(nodes: &[Value], post_id: &str) -> (Vec<Comment>, Vec<MoreStub>) {
    let (mut comments, mut more) = (Vec::new(), Vec::new());
    let mut stack: Vec<(&Value, u32)> = nodes.iter().rev().map(|n| (n, 0)).collect();
    while let Some((node, depth)) = stack.pop() {
        let (kind, d) = unwrap(node);
        if kind == "more" {
            let parent = text(d, "parent_id").unwrap_or_default();
            let children =
                d.get("children").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default();
            more.push(MoreStub { parent_id: parent.strip_prefix("t1_").map(str::to_owned), count: int(d, "count").unwrap_or(0), children });
            continue;
        }
        if !(kind == "t1" || kind.is_empty() && d.get("body").is_some()) {
            continue;
        }
        let Some(c) = parse_comment(d, post_id, depth) else { continue };
        comments.push(c);
        if let Some(children) = d.get("replies").and_then(|r| r.get("data")).and_then(|l| l.get("children")).and_then(Value::as_array) {
            stack.extend(children.iter().rev().map(|n| (n, depth + 1)));
        }
    }
    (comments, more)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recorded_post_parses() {
        let v: Value = serde_json::from_str(include_str!("../../../fixtures/arctic/posts_ids.json")).unwrap();
        let p = parse_post(&v["data"][0]).unwrap();
        assert_eq!((p.id.as_str(), p.subreddit.as_str(), p.state), ("1xsjnud", "AMDHelp", State::Live));
        assert_eq!(p.flair.as_deref(), Some("Help (GPU)"));
        assert_eq!(p.url, None, "a self post's url is its own permalink, not an outbound link");
        assert!(p.scores_as_of.unwrap() > p.created);
    }

    #[test]
    fn recorded_tree_parses_in_preorder() {
        let v: Value = serde_json::from_str(include_str!("../../../fixtures/arctic/tree.json")).unwrap();
        let (c, more) = parse_tree(v["data"].as_array().unwrap(), "1xsjnud");
        assert_eq!(c.len(), 4);
        assert!(more.is_empty());
        let reply = c.iter().find(|x| x.depth == 1).unwrap();
        let parent_pos = c.iter().position(|x| Some(&x.id) == reply.parent_id.as_ref()).unwrap();
        assert!(parent_pos < c.iter().position(|x| x.id == reply.id).unwrap());
        assert!(c.iter().filter(|x| x.depth == 0).all(|x| x.parent_id.is_none()));
    }

    #[test]
    fn recorded_more_stub_parses() {
        let v: Value = serde_json::from_str(include_str!("../../../fixtures/arctic/tree_more.json")).unwrap();
        let (c, more) = parse_tree(v["data"].as_array().unwrap(), "1xsjnud");
        assert_eq!(c.len(), 2);
        assert_eq!(more, vec![MoreStub { parent_id: None, count: 2, children: vec!["19sgled0apypv".into()] }]);
    }

    #[test]
    fn quirks_never_fail_a_tree() {
        let nodes = json!([
            {"kind": "t1", "data": {"id": "c1", "author": "a", "body": "top", "score": null, "parent_id": "t3_p", "created_utc": 1700000001.0, "edited": 1700000500.0,
              "replies": {"kind": "Listing", "data": {"children": [
                {"kind": "t1", "data": {"id": "c2", "author": "[deleted]", "body": "[deleted]", "parent_id": "t1_c1", "created_utc": "1700000002", "edited": false, "replies": ""}},
                {"kind": "more", "data": {"id": "_", "count": 0, "parent_id": "t1_c1", "children": []}}]}}}},
            {"kind": "t5", "data": {"display_name": "s"}},
            {"kind": "t1", "data": {"body": "no id, skipped"}},
            {"kind": "t1", "data": {"id": "c3", "author": "m", "body": "[ Removed by Reddit ]", "parent_id": "t3_p"}},
            {"kind": "t1", "data": {"id": "c4", "author": "x", "body": "kept text", "removed_by_category": "moderator", "parent_id": "t3_p"}}
        ]);
        let (c, more) = parse_tree(nodes.as_array().unwrap(), "p");
        assert_eq!(c.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["c1", "c2", "c3", "c4"]);
        assert_eq!((c[0].score, c[0].edited, c[0].created), (None, Some(1700000500), 1700000001));
        assert_eq!((c[1].state, c[1].author.as_deref(), c[1].body.as_str(), c[1].created, c[1].depth), (State::Deleted, None, "", 1700000002, 1));
        assert_eq!(c[2].state, State::Removed);
        assert_eq!(
            (c[3].state, c[3].body.as_str(), c[3].author.as_deref()),
            (State::Removed, "", None),
            "removed text is never kept even if the source has it"
        );
        assert_eq!(more.len(), 1);
        assert_eq!(more[0].parent_id.as_deref(), Some("c1"));
    }

    #[test]
    fn recorded_subreddit_parses() {
        let v: Value = serde_json::from_str(include_str!("../../../fixtures/arctic/subs_prefix.json")).unwrap();
        let s = parse_subreddit(&v["data"][0]).unwrap();
        assert_eq!((s.name.as_str(), s.subscribers, s.over_18, s.posts), ("AMDHelp", Some(139_317), false, Some(182_867)));
        assert!(s.description.starts_with("Troubleshooting help"));
        assert_eq!(parse_subreddit(&json!({"subscribers": 5})), None);
    }

    #[test]
    fn deleted_post_keeps_no_text() {
        let p = parse_post(&json!({"id": "p", "title": "t", "selftext": "secret", "author": "a", "removed_by_category": "deleted"})).unwrap();
        assert_eq!((p.state, p.selftext.as_str(), p.author), (State::Deleted, "", None));
        let p = parse_post(
            &json!({"id": "p", "title": "My real name is in this title", "selftext": "[removed]", "url": "https://example.com/x", "link_flair_text": "f"}),
        )
        .unwrap();
        assert_eq!((p.state, p.title.as_str(), p.url, p.flair), (State::Removed, "[removed]", None, None), "title, link and flair go with the text");
    }
}
