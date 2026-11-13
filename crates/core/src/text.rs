//! Small text and date helpers shared by ranking, selection and rendering.

/// Days-to-civil conversion (Howard Hinnant's algorithm), so no date crate is needed.
pub fn ymd(epoch: i64) -> (i64, u32, u32) {
    let z = epoch.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn date(epoch: i64) -> String {
    let (y, m, d) = ymd(epoch);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn datetime(epoch: i64) -> String {
    let s = epoch.rem_euclid(86_400);
    format!("{}T{:02}:{:02}Z", date(epoch), s / 3600, s % 3600 / 60)
}

/// `5d`, `8mo`, `2y`: how old something is, at a glance.
pub fn age(created: i64, now: i64) -> String {
    let days = (now - created).max(0) / 86_400;
    match days {
        0 => "today".into(),
        1..=59 => format!("{days}d"),
        60..=729 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

/// Accepts `YYYY-MM-DD` only; anything else is refused rather than guessed.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Midnight UTC of a `YYYY-MM-DD` date as epoch seconds.
pub fn epoch(date: &str) -> Option<i64> {
    if !is_date(date) {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (date[..4].parse().ok()?, date[5..7].parse().ok()?, date[8..].parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400)
}

/// Rough token estimate for English prose with markdown; errs high.
pub fn tokens(chars: usize) -> usize {
    chars.div_ceil(3)
}

/// Removes characters a reader cannot see but a model would still read: zero-width and bidi controls,
/// and control codes other than newline and tab. Unicode line and paragraph separators become plain newlines,
/// so nothing can start a visual line without the caller seeing a line break. Returns the cleaned text and how many were removed.
pub fn strip_invisible(s: &str) -> (String, usize) {
    let mut removed = 0;
    let out = s
        .chars()
        .filter_map(|c| {
            if matches!(c, '\u{2028}' | '\u{2029}') {
                return Some('\n');
            }
            let c = &c;
            let hidden = matches!(*c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}')
                || (c.is_control() && *c != '\n' && *c != '\t');
            removed += usize::from(hidden);
            (!hidden).then_some(*c)
        })
        .collect();
    (out, removed)
}

/// Cuts at a character boundary at or below `max` characters, preferring the end of a sentence or line.
pub fn cut(s: &str, max: usize) -> &str {
    if s.chars().count() <= max {
        return s;
    }
    let end = s.char_indices().nth(max).map_or(s.len(), |(i, _)| i);
    let head = &s[..end];
    let floor = head.len() * 6 / 10;
    match head.rfind(['\n', '.', '!', '?']) {
        Some(i) if i >= floor => &head[..=i],
        _ => head,
    }
}

/// Query terms: quoted phrases stay whole, everything else splits on whitespace. Lower-cased.
pub fn terms(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = query;
    while let Some(open) = rest.find('"') {
        out.extend(rest[..open].split_whitespace().map(str::to_lowercase));
        let after = &rest[open + 1..];
        let close = after.find('"').unwrap_or(after.len());
        let phrase = after[..close].trim().to_lowercase();
        if !phrase.is_empty() {
            out.push(phrase);
        }
        rest = after.get(close + 1..).unwrap_or("");
    }
    out.extend(rest.split_whitespace().map(str::to_lowercase));
    out.retain(|t| !matches!(t.as_str(), "or" | "and" | "not") && t.chars().any(char::is_alphanumeric));
    out.dedup();
    out
}

/// A term that names a specific thing (model number, error code, version): it contains a digit.
pub fn is_identifier(term: &str) -> bool {
    term.len() >= 3 && term.chars().any(|c| c.is_ascii_digit())
}

pub fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(datetime(1_794_307_045), "2026-11-10T10:37Z");
        assert_eq!(date(-86_400), "1969-12-31");
        assert_eq!((age(0, 86_400 * 3), age(0, 86_400 * 240), age(0, 86_400 * 800), age(5, 0)), ("3d".into(), "8mo".into(), "2y".into(), "today".into()));
        assert!(is_date("2026-01-31") && !is_date("2026-1-31") && !is_date("yesterday"));
        assert_eq!((epoch("1970-01-01"), epoch("2000-02-29"), epoch("2026-13-01"), epoch("soon")), (Some(0), Some(951_782_400), None, None));
        assert_eq!(date(epoch("2026-11-10").unwrap()), "2026-11-10");
    }

    #[test]
    fn invisible_characters_are_counted_and_removed() {
        assert_eq!(strip_invisible("a\u{200B}b\u{202E}c\r\n\td\u{0007}"), ("abc\n\td".into(), 4));
        assert_eq!(strip_invisible("a\u{2028}b\u{2029}c\u{0085}d\u{000B}e"), ("a\nb\ncde".into(), 2), "separators become newlines");
    }

    #[test]
    fn cut_prefers_sentence_end_and_respects_chars() {
        assert_eq!(cut("One two. Three four five", 12), "One two.");
        assert_eq!(cut("ééééé", 3), "ééé");
        assert_eq!(cut("short", 50), "short");
    }

    #[test]
    fn query_terms() {
        assert_eq!(terms(r#"B650E-E "boot loop" OR expo"#), ["b650e-e", "boot loop", "expo"]);
        assert_eq!(terms(r#""unclosed phrase"#), ["unclosed phrase"]);
        assert!(is_identifier("b650e-e") && is_identifier("0x80070057") && !is_identifier("expo"));
    }
}
