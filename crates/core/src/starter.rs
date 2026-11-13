//! A built-in topic -> subreddit starter list.
//!
//! Web searches often return no reddit.com URLs at all, which left `rdt subs` with nothing to go on.
//! This table lets it propose subreddits from the topic words alone. Every name here was picked because it
//! is known to exist with that exact spelling; a wrong name costs a wasted archive lookup, so fewer is better.

use crate::text;

/// A subreddit the starter list proposes, and the topic word that brought it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Starter {
    pub name: &'static str,
    pub because: &'static str,
}

/// How a group ranks. A specific group (one brand, one game, one OS) knows more about the question than the
/// general PC subreddits, so it goes first; the general ones follow as a fallback.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A piece of PC hardware: when it matches, the general PC group is added too.
    Hardware,
    /// A specific game, OS, app or device family.
    Specific,
    /// Broad PC subreddits, ranked last.
    General,
}

struct Group {
    kind: Kind,
    /// Lowercase. A trigger with a space is matched as consecutive topic words ("steam deck").
    words: &'static [&'static str],
    subs: &'static [&'static str],
}

/// The general PC hardware group, also added whenever a hardware group matches.
const GENERAL_PC: Group = Group {
    kind: Kind::General,
    words: &[
        "pc",
        "build",
        "building",
        "computer",
        "desktop",
        "motherboard",
        "mobo",
        "bios",
        "uefi",
        "psu",
        "power supply",
        "cpu",
        "gpu",
        "bsod",
        "freeze",
        "freezes",
        "freezing",
        "crash",
        "crashes",
        "crashing",
        "overheating",
        "temps",
        "fans",
        "driver",
        "drivers",
        "prebuilt",
        "expo",
        "xmp",
        "b650",
        "x670",
        "b850",
        "x870",
        "z790",
        "b760",
        "coil whine",
    ],
    subs: &["buildapc", "techsupport", "PcBuildHelp", "buildapcforme"],
};

static GROUPS: &[Group] = &[
    Group {
        kind: Kind::Hardware,
        words: &[
            "amd",
            "radeon",
            "rx",
            "adrenalin",
            "ryzen",
            "fsr",
            "x3d",
            "am4",
            "am5",
            "9070",
            "9060",
            "7900",
            "7800",
            "7700",
            "7600",
            "6950",
            "6900",
            "6800",
            "6700",
            "6600",
            "7800x3d",
            "9800x3d",
            "9950x3d",
            "5800x3d",
            "5700x3d",
            "7950x3d",
        ],
        subs: &["radeon", "AMDHelp", "Amd"],
    },
    Group {
        kind: Kind::Hardware,
        words: &["nvidia", "geforce", "rtx", "gtx", "dlss", "5090", "5080", "5070", "5060", "4090", "4080", "4070", "4060", "3090", "3080", "3070", "3060"],
        subs: &["nvidia", "nvidiahelp"],
    },
    Group {
        kind: Kind::Hardware,
        words: &["intel", "i3", "i5", "i7", "i9", "14900k", "13900k", "14700k", "13700k", "raptor lake", "core ultra"],
        subs: &["intel"],
    },
    Group {
        kind: Kind::Hardware,
        words: &["monitor", "monitors", "display", "oled", "hz", "144hz", "240hz", "ghosting", "hdr", "freesync", "gsync", "g-sync"],
        subs: &["Monitors", "OLED_Gaming"],
    },
    Group {
        kind: Kind::Hardware,
        words: &["ram", "memory", "ddr4", "ddr5", "dimm", "overclock", "overclocking", "undervolt", "undervolting", "timings"],
        subs: &["overclocking"],
    },
    Group { kind: Kind::Hardware, words: &["ssd", "nvme", "hdd", "storage", "nas", "jbod", "raid", "990", "870", "sata"], subs: &["DataHoarder", "NewMaxx"] },
    Group { kind: Kind::Hardware, words: &["laptop", "laptops", "notebook"], subs: &["SuggestALaptop", "GamingLaptops"] },
    Group { kind: Kind::Hardware, words: &["router", "routers", "wifi", "wi-fi", "ethernet", "mesh", "lan", "networking", "modem"], subs: &["HomeNetworking"] },
    Group { kind: Kind::Specific, words: &["windows", "win11", "win10", "windows 11", "windows 10"], subs: &["Windows11", "Windows10", "WindowsHelp"] },
    Group { kind: Kind::Specific, words: &["linux", "ubuntu", "fedora", "debian", "kernel", "mesa"], subs: &["linuxquestions", "linux4noobs"] },
    Group { kind: Kind::Specific, words: &["cyberpunk", "2077", "phantom liberty"], subs: &["cyberpunkgame", "LowSodiumCyberpunk"] },
    Group { kind: Kind::Specific, words: &["steam deck", "steamdeck"], subs: &["SteamDeck"] },
    Group { kind: Kind::Specific, words: &["elden ring", "eldenring", "nightreign"], subs: &["Eldenring"] },
    Group { kind: Kind::Specific, words: &["wallpaper engine", "wallpaperengine"], subs: &["wallpaperengine"] },
    Group {
        kind: Kind::Specific,
        words: &[
            "emulation",
            "emulator",
            "emulators",
            "retroarch",
            "rpcs3",
            "pcsx2",
            "dolphin",
            "duckstation",
            "ppsspp",
            "cemu",
            "ryujinx",
            "eden",
            "rom",
            "roms",
        ],
        subs: &["emulation", "EmulationOnAndroid"],
    },
    Group { kind: Kind::Specific, words: &["handheld", "handhelds", "retroid", "anbernic", "ayn", "odin", "thor"], subs: &["SBCGaming"] },
    Group { kind: Kind::Specific, words: &["nintendo", "joycon", "joy-con", "switch 2", "nintendo switch"], subs: &["NintendoSwitch"] },
    Group { kind: Kind::General, words: &["pc gaming", "pcgaming", "stutter", "stuttering", "fps", "steam", "frametime"], subs: &["pcgaming", "pcmasterrace"] },
    GENERAL_PC,
];

/// Every form of the topic a trigger can match: whole terms, their words, the letter and digit runs inside a
/// word (so "rx9070xt" finds "rx" and "9070"), and a crude singular. Also the words in order, space-joined,
/// for triggers of more than one word.
fn forms(topic: &str) -> (Vec<String>, String) {
    let mut words = Vec::new();
    for term in text::terms(topic) {
        words.extend(term.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned));
    }
    let joined = format!(" {} ", words.join(" "));
    let mut set: Vec<String> = text::terms(topic);
    for w in &words {
        set.push(w.clone());
        // Split where letters meet digits; a model number written without spaces still names its family.
        let mut run = String::new();
        let mut last_digit = None;
        for c in w.chars() {
            let digit = c.is_ascii_digit();
            if last_digit.is_some_and(|d| d != digit) {
                set.push(std::mem::take(&mut run));
            }
            run.push(c);
            last_digit = Some(digit);
        }
        set.push(run);
        if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
            set.push(w[..w.len() - 1].to_owned());
        }
    }
    set.sort();
    set.dedup();
    (set, joined)
}

/// The first of a group's triggers the topic contains, if any.
fn matched(group: &Group, set: &[String], joined: &str) -> Option<&'static str> {
    group.words.iter().copied().find(|w| if w.contains(' ') { joined.contains(&format!(" {w} ")) } else { set.binary_search_by(|s| s.as_str().cmp(w)).is_ok() })
}

/// Subreddits worth searching for a topic, best first, at most `limit`, no duplicates (case-insensitive).
pub fn for_topic(topic: &str, limit: usize) -> Vec<Starter> {
    let (set, joined) = forms(topic);
    let mut hits: Vec<(Kind, &Group, &'static str)> = Vec::new();
    let mut hardware: Option<&'static str> = None;
    for g in GROUPS {
        if let Some(word) = matched(g, &set, &joined) {
            hits.push((g.kind, g, word));
            if g.kind == Kind::Hardware {
                hardware.get_or_insert(word);
            }
        }
    }
    // A question about one GPU or drive is still a PC question; the general subreddits are the fallback.
    if let Some(word) = hardware
        && !hits.iter().any(|(_, g, _)| std::ptr::eq(*g, &GROUPS[GROUPS.len() - 1]))
    {
        hits.push((Kind::General, &GROUPS[GROUPS.len() - 1], word));
    }
    // Stable sort keeps table order within a rank; general groups go last.
    hits.sort_by_key(|(kind, ..)| *kind == Kind::General);
    let mut out: Vec<Starter> = Vec::new();
    for (_, g, because) in hits {
        for &name in g.subs {
            if out.len() >= limit {
                return out;
            }
            if !out.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
                out.push(Starter { name, because });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(topic: &str, limit: usize) -> Vec<&'static str> {
        for_topic(topic, limit).into_iter().map(|s| s.name).collect()
    }

    #[test]
    fn a_gpu_and_a_game_rank_above_the_general_pc_subreddits() {
        let got = names("RX 9070 XT hard freeze cyberpunk", 8);
        assert!(got.len() <= 8, "{got:?}");
        let at = |n: &str| got.iter().position(|g| *g == n).unwrap_or_else(|| panic!("{n} missing from {got:?}"));
        let build = at("buildapc");
        for n in ["radeon", "AMDHelp", "Amd", "cyberpunkgame"] {
            assert!(at(n) < build, "{n} should come before buildapc: {got:?}");
        }
        let amd = for_topic("RX 9070 XT hard freeze cyberpunk", 8);
        assert_eq!((amd[0].name, amd[0].because), ("radeon", "rx"));
    }

    #[test]
    fn general_pc_is_the_fallback_for_any_hardware_group() {
        let got = for_topic("990 pro firmware", 20);
        assert_eq!(got[0].name, "DataHoarder");
        let build = got.iter().find(|s| s.name == "buildapc").expect("general group added");
        assert_eq!(build.because, "990", "the fallback says which hardware word brought it in");
        assert!(names("cyberpunk ending", 20).iter().all(|n| *n != "buildapc"), "a game alone is not a hardware question");
    }

    #[test]
    fn limit_is_respected_and_names_are_unique() {
        let topic = "rtx 4090 vs rx 7900 on windows 11 laptop with ddr5 and nvme, router wifi, pc gaming stutter";
        for limit in [0, 1, 3, 8, 50] {
            let got = names(topic, limit);
            assert!(got.len() <= limit, "{limit}: {got:?}");
            let mut lower: Vec<String> = got.iter().map(|n| n.to_lowercase()).collect();
            lower.sort();
            let before = lower.len();
            lower.dedup();
            assert_eq!(lower.len(), before, "duplicates in {got:?}");
        }
        assert_eq!(names(topic, 50).iter().filter(|n| n.eq_ignore_ascii_case("radeon")).count(), 1);
    }

    #[test]
    fn unrelated_topics_get_nothing() {
        assert!(for_topic("sourdough starter", 10).is_empty());
        assert!(for_topic("", 10).is_empty());
    }

    #[test]
    fn matching_ignores_case_and_finds_glued_model_numbers_and_phrases() {
        assert_eq!(names("RADEON Driver Timeout", 3), names("radeon driver timeout", 3));
        assert!(names("RX9070XT black screen", 8).contains(&"AMDHelp"));
        assert!(names("RTX4080 coil whine", 8).contains(&"nvidia"));
        assert!(names("Steam-Deck battery", 8).contains(&"SteamDeck"));
        assert_eq!(names("Elden Ring FPS lock", 8), ["Eldenring", "pcgaming", "pcmasterrace"]);
        assert!(names("\"wallpaper engine\" high cpu", 8).starts_with(&["wallpaperengine"]));
        assert!(names("7800X3D temps", 8).starts_with(&["radeon", "AMDHelp", "Amd"]));
    }
}
