//! Turns whatever a person or a web search hands us (URLs in many shapes, fullnames, bare ids) into thread ids.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// A thread, optionally focused on one comment.
    Thread { post: String, comment: Option<String> },
    /// `/r/sub/s/CODE` share link: only Reddit can say where it points, and we do not ask Reddit.
    ShareLink(String),
    /// A Reddit address that is not a thread (subreddit, wiki, user, media), or not Reddit at all.
    NotThread,
}

fn is_id(s: &str) -> bool {
    (1..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

pub fn parse_ref(input: &str) -> Ref {
    let raw = input.trim();
    let lower = raw.to_ascii_lowercase();
    if let Some(id) = lower.strip_prefix("t3_") {
        return if is_id(id) { Ref::Thread { post: id.to_owned(), comment: None } } else { Ref::NotThread };
    }
    if !lower.contains('/') && !lower.contains('.') {
        return if lower.len() >= 5 && is_id(&lower) { Ref::Thread { post: lower, comment: None } } else { Ref::NotThread };
    }
    // Find the Reddit host anywhere in the string, so AMP and redirect wrappers work too.
    let Some((host_end, short)) = find_host(&lower).or_else(|| mirror_path(&lower).map(|i| (i, false))) else { return Ref::NotThread };
    let path = lower[host_end..].split(['?', '#']).next().unwrap_or("");
    let seg: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).map(|s| s.trim_end_matches(".json")).collect();
    if short {
        return match seg.first() {
            Some(id) if is_id(id) => Ref::Thread { post: (*id).to_owned(), comment: None },
            _ => Ref::NotThread,
        };
    }
    if let Some(i) = seg.iter().position(|s| *s == "comments") {
        let Some(post) = seg.get(i + 1).filter(|s| is_id(s)) else { return Ref::NotThread };
        // /comments/{id}/{slug}/{comment} or the newer /comments/{id}/comment/{comment}
        let comment = seg.get(i + 3).filter(|s| is_id(s)).map(|s| (*s).to_owned());
        return Ref::Thread { post: (*post).to_owned(), comment };
    }
    match seg.as_slice() {
        ["gallery" | "poll", id, ..] if is_id(id) => Ref::Thread { post: (*id).to_owned(), comment: None },
        ["r", _, "s", code, ..] => Ref::ShareLink((*code).to_owned()),
        _ => Ref::NotThread,
    }
}

/// Returns the index just past the host and whether it is the `redd.it` short host.
fn find_host(lower: &str) -> Option<(usize, bool)> {
    for (host, short) in [("reddit.com", false), ("redd.it", true)] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(host) {
            let (start, end) = (from + pos, from + pos + host.len());
            let before_ok = start == 0 || matches!(lower.as_bytes()[start - 1], b'.' | b'/');
            let after_ok = end == lower.len() || lower.as_bytes()[end] == b'/';
            // i.redd.it, v.redd.it and preview.redd.it are media hosts, not threads.
            let media = short && start > 0 && lower.as_bytes()[start - 1] == b'.';
            if before_ok && after_ok && !media {
                return Some((end, short));
            }
            from = end;
        }
    }
    None
}

/// Web searches often return Reddit threads through mirror sites on other hosts. A URL on any host whose path is
/// exactly Reddit's thread shape, `/r/<name>/comments/<id>...`, is accepted for its id alone: the host is never
/// contacted. Returns where the path starts.
fn mirror_path(lower: &str) -> Option<usize> {
    let after_scheme = lower.find("://")? + 3;
    let start = after_scheme + lower[after_scheme..].find('/')?;
    let seg: Vec<&str> = lower[start..].split(['?', '#']).next().unwrap_or("").split('/').filter(|s| !s.is_empty()).take(4).collect();
    matches!(seg.as_slice(), ["r", name, "comments", id] if is_subreddit_name(name) && id.len() >= 5 && is_id(id)).then_some(start)
}

/// Letters, digits and underscores, 2 to 21 of them. Anything else is not a subreddit name, wherever it came from.
pub fn is_subreddit_name(s: &str) -> bool {
    (2..=21).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The subreddit a Reddit URL points into, with the capitalisation the URL used. `r/name` and a bare name work too.
pub fn subreddit_of(input: &str) -> Option<String> {
    let raw = input.trim();
    let valid = is_subreddit_name;
    let lower = raw.to_ascii_lowercase();
    let path = match find_host(&lower) {
        Some((end, false)) => &raw[end..],
        Some((_, true)) => return None,
        None if !raw.contains('.') && !raw.contains(':') => raw,
        None => &raw[mirror_path(&lower)?..],
    };
    let seg: Vec<&str> = path.split(['?', '#']).next().unwrap_or("").split('/').filter(|s| !s.is_empty()).collect();
    match seg.as_slice() {
        [r, name, ..] if r.eq_ignore_ascii_case("r") && valid(name) => Some((*name).to_owned()),
        [name] if find_host(&lower).is_none() && valid(name) => Some((*name).to_owned()),
        _ => None,
    }
}

pub fn thread_url(post: &str) -> String {
    format!("https://www.reddit.com/comments/{post}")
}

pub fn comment_url(post: &str, comment: &str) -> String {
    format!("https://www.reddit.com/comments/{post}/_/{comment}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(post: &str, comment: Option<&str>) -> Ref {
        Ref::Thread { post: post.into(), comment: comment.map(Into::into) }
    }

    #[test]
    fn subreddit_from_urls_and_names() {
        let cases: &[(&str, Option<&str>)] = &[
            ("https://www.reddit.com/r/AMDHelp/comments/1xsjnud/about_x/", Some("AMDHelp")),
            ("https://old.reddit.com/r/buildapc/?sort=new", Some("buildapc")),
            ("https://www.google.com/amp/s/amp.reddit.com/r/ASUS/comments/1abc2de/slug/", Some("ASUS")),
            ("r/AMDHelp", Some("AMDHelp")),
            ("/r/pc_gaming/", Some("pc_gaming")),
            ("thinkpad", Some("thinkpad")),
            ("https://www.reddit.com/user/someone/comments/1abc2de/title/", None),
            ("https://www.reddit.com/comments/1abc2de", None),
            ("https://redd.it/1abc2de", None),
            ("https://example.com/r/AMDHelp/", None),
            ("https://lr.us.psf.lt/r/MSI_Gaming/comments/16okjq1/slow_boot_times/k3lr6xa", Some("MSI_Gaming")),
            ("https://www.reddit.com/r/a/", None),
            ("https://www.reddit.com/r/bad-name/", None),
            ("two words", None),
            ("", None),
        ];
        for (input, want) in cases {
            assert_eq!(subreddit_of(input).as_deref(), *want, "{input}");
        }
        assert!(is_subreddit_name("pc_gaming") && !is_subreddit_name("") && !is_subreddit_name("a\nb") && !is_subreddit_name("x</reddit_subs>"));
    }

    #[test]
    fn url_shapes() {
        let cases: &[(&str, Ref)] = &[
            ("https://www.reddit.com/r/ASUS/comments/1abc2de/b650ee_expo/", t("1abc2de", None)),
            ("https://old.reddit.com/r/ASUS/comments/1abc2de/b650ee_expo/?utm_source=share#x", t("1abc2de", None)),
            ("http://np.reddit.com/r/ASUS/comments/1ABC2DE", t("1abc2de", None)),
            ("reddit.com/comments/1abc2de", t("1abc2de", None)),
            ("https://www.reddit.com/r/ASUS/comments/1abc2de/slug/k3x9z1a/?context=3", t("1abc2de", Some("k3x9z1a"))),
            ("https://www.reddit.com/r/AMDHelp/comments/1xsjnud/about_x/19sfqopne46x0/", t("1xsjnud", Some("19sfqopne46x0"))),
            ("https://www.reddit.com/r/ASUS/comments/1abc2de/comment/k3x9z1a", t("1abc2de", Some("k3x9z1a"))),
            ("https://www.reddit.com/comments/1abc2de/_/k3x9z1a", t("1abc2de", Some("k3x9z1a"))),
            ("https://redd.it/1abc2de", t("1abc2de", None)),
            ("https://www.reddit.com/gallery/1abc2de", t("1abc2de", None)),
            ("https://www.reddit.com/user/someone/comments/1abc2de/title/", t("1abc2de", None)),
            ("https://www.google.com/amp/s/amp.reddit.com/r/ASUS/comments/1abc2de/slug/", t("1abc2de", None)),
            ("https://www.reddit.com/r/ASUS/comments/1abc2de.json", t("1abc2de", None)),
            ("t3_1abc2de", t("1abc2de", None)),
            ("1abc2de", t("1abc2de", None)),
            ("https://www.reddit.com/r/ASUS/s/AbCdEf123", Ref::ShareLink("abcdef123".into())),
            ("https://www.reddit.com/r/ASUS/", Ref::NotThread),
            ("https://www.reddit.com/r/ASUS/wiki/index", Ref::NotThread),
            ("https://i.redd.it/abcdefg.png", Ref::NotThread),
            // A look-alike host is not treated as Reddit: anything but the exact thread shape is refused there.
            ("https://notreddit.com/comments/1abc2de/", Ref::NotThread),
            ("https://notreddit.com/r/ASUS/", Ref::NotThread),
            ("https://notreddit.com/gallery/1abc2de", Ref::NotThread),
            ("https://reddit.com.evil.example/r/ASUS/s/AbCdEf", Ref::NotThread),
            ("https://example.com/?next=/r/ASUS/comments/1abc2de/x", Ref::NotThread),
            ("x://", Ref::NotThread),
            ("https://example.com/r/ASUS/comments/1abc2de/\u{00e9}", t("1abc2de", None)),
            ("https://example.com/comments/1abc2de", Ref::NotThread),
            // Mirror sites, as web searches return them: accepted only in Reddit's exact thread shape.
            ("https://lr.us.psf.lt/r/MSI_Gaming/comments/16okjq1/slow_boot_times/k3lr6xa", t("16okjq1", Some("k3lr6xa"))),
            ("https://lr.vern.cc/r/ASUS/comments/167a3m4/comment/k02pnz0/?context=3", t("167a3m4", Some("k02pnz0"))),
            ("https://reddit.birdcat.cafe/r/ASRock/comments/1iapgt6/b650i_lightning", t("1iapgt6", None)),
            ("https://example.com/blog/r/ASRock/comments/1iapgt6/x", Ref::NotThread),
            ("https://example.com/r/ASRock/", Ref::NotThread),
            ("https://example.com/r/ASRock/comments/", Ref::NotThread),
            ("hello world", Ref::NotThread),
            ("", Ref::NotThread),
        ];
        for (input, want) in cases {
            assert_eq!(&parse_ref(input), want, "{input}");
        }
    }
}
