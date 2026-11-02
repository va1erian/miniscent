//! What to offer at the caret. Inside a string that names something it offers
//! the level's own names, in code the messages, with those for the receiver
//! first. Offsets are in chars, as in the code editor.

use super::api::{API, Recv};
use super::scan::{At, Define, assigned_names, at_caret, defines, quoted_after};

/// What a suggestion is, for the marker beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Function,
    Method,
    Variable,
    /// A name from the level, or a word a message wants.
    Name,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub label: String,
    pub insert: String,
    pub detail: String,
    pub kind: Kind,
}

/// The level's names, sorted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelNames {
    pub movers: Vec<String>,
    pub zones: Vec<String>,
    pub markers: Vec<String>,
    pub enemies: Vec<String>,
    pub tags: Vec<String>,
}

/// What a string is, by the message that takes it.
#[derive(Clone, Copy)]
enum Wants {
    Movers,
    Zones,
    Markers,
    MarkersOrTags,
    Enemies,
    Tags,
    Kinds,
    Poses,
    Words(&'static [&'static str]),
}

fn wants(keyword: &str) -> Option<Wants> {
    Some(match keyword {
        "mover:" | "autoDoor:" | "switchDoor:" | "lift:" => Wants::Movers,
        "zone:" | "wave:" | "exitZone:" => Wants::Zones,
        "marker:" | "keycardAt:" | "terminal:" | "healthPack:" => Wants::Markers,
        "markers:" => Wants::MarkersOrTags,
        "enemy:" => Wants::Enemies,
        "tagged:" => Wants::Tags,
        "spawn:" | "spawnAll:" => Wants::Kinds,
        "moveTo:" => Wants::Poses,
        "whenBlocked:" => Wants::Words(&["hold", "reverse", "crush"]),
        "setFaction:" => Wants::Words(&["friendly", "hostile"]),
        _ => return None,
    })
}

/// Kinds a script can always spawn, in the game's own table.
const BUILT_IN_KINDS: [&str; 6] = ["drone", "hunter", "switch", "keycard", "terminal", "crate"];

fn name_items(names: impl IntoIterator<Item = String>, detail: &str) -> Vec<Suggestion> {
    names
        .into_iter()
        .map(|label| Suggestion {
            insert: label.clone(),
            label,
            detail: detail.to_string(),
            kind: Kind::Name,
        })
        .collect()
}

fn strings_for(want: Wants, names: &LevelNames, text: &str) -> Vec<Suggestion> {
    match want {
        Wants::Movers => name_items(names.movers.clone(), "mover"),
        Wants::Zones => name_items(names.zones.clone(), "zone"),
        Wants::Markers => name_items(names.markers.clone(), "marker"),
        Wants::MarkersOrTags => {
            let mut items = name_items(names.markers.clone(), "marker");
            items.extend(name_items(names.tags.clone(), "tag"));
            items
        }
        Wants::Enemies => name_items(names.enemies.clone(), "enemy"),
        Wants::Tags => {
            let mut tags = names.tags.clone();
            tags.extend(quoted_after(text, "tag:"));
            tags.sort();
            tags.dedup();
            name_items(tags, "tag")
        }
        Wants::Kinds => {
            let mut kinds: Vec<String> = BUILT_IN_KINDS.map(String::from).to_vec();
            kinds.extend(quoted_after(text, "defineEnemy:"));
            kinds.extend(quoted_after(text, "defineProp:"));
            name_items(kinds, "kind")
        }
        Wants::Poses => {
            let mut poses = quoted_after(text, "pose:");
            poses.push("closed".to_string());
            poses.sort();
            poses.dedup();
            name_items(poses, "pose")
        }
        Wants::Words(words) => name_items(words.iter().map(|w| (*w).to_string()), ""),
    }
}

fn message_item(selector: &str, insert: &str, help: &str, kind: Kind) -> Suggestion {
    Suggestion {
        label: selector.to_string(),
        insert: insert.to_string(),
        detail: help.to_string(),
        kind,
    }
}

fn define_item(d: &Define) -> Suggestion {
    let kind = if d.receiver {
        Kind::Method
    } else {
        Kind::Function
    };
    message_item(&d.selector, &d.insert, &d.note, kind)
}

/// Every message, with those for `receiver` first; `None` is no known receiver.
/// A library function is used more than a language message, so it comes before
/// the rest.
fn code_items(receiver: Option<Recv>, library: &[Define], text: &str) -> Vec<Suggestion> {
    let of = |wanted: Recv| {
        let matching = API.iter().filter(move |e| e.on == wanted);
        matching.map(|e| {
            let kind = if e.on == Recv::Global {
                Kind::Function
            } else {
                Kind::Method
            };
            message_item(e.selector, e.insert, e.help, kind)
        })
    };
    let mut items: Vec<Suggestion> = Vec::new();
    match receiver {
        Some(r) => {
            items.extend(of(r));
            items.extend(defines(text).iter().filter(|d| d.receiver).map(define_item));
        }
        None => {
            let vars = assigned_names(text).into_iter();
            items.extend(vars.map(|v| message_item(&v, &v, "variable", Kind::Variable)));
            items.extend(defines(text).iter().map(define_item));
            items.extend(library.iter().map(define_item));
            items.extend(of(Recv::Global));
        }
    }
    let seen: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
    let rest = API.iter().filter(|e| !seen.iter().any(|s| s == e.selector));
    items.extend(rest.map(|e| message_item(e.selector, e.insert, e.help, Kind::Method)));
    items
}

/// The code editor opens its popup after a period, which elsewhere leads to a
/// member. Here it ends a statement, and Enter would then accept a suggestion.
fn just_ended_statement(text: &str, caret: usize) -> bool {
    caret > 0 && text.chars().nth(caret - 1) == Some('.')
}

/// Where the word being completed starts, and what is on offer. `None` in a
/// comment, after a period, and in a string nothing is asked for.
pub fn suggest(
    names: &LevelNames,
    library: &[Define],
    text: &str,
    caret: usize,
) -> Option<(usize, Vec<Suggestion>)> {
    match at_caret(text, caret) {
        At::Comment => None,
        At::String { start, keyword } => {
            let items = strings_for(wants(&keyword?)?, names, text);
            (!items.is_empty()).then_some((start, items))
        }
        At::Code { .. } if just_ended_statement(text, caret) => None,
        At::Code { start, receiver } => Some((start, code_items(receiver, library, text))),
    }
}

/// Whether the character just typed opened a string the level has names for, so
/// the popup should open with nothing typed yet.
pub fn opens_names(text: &str, caret: usize) -> bool {
    let just_opened = caret > 0 && text.chars().nth(caret - 1) == Some('\'');
    just_opened
        && matches!(
            at_caret(text, caret),
            At::String { keyword: Some(k), .. } if wants(&k).is_some()
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> LevelNames {
        LevelNames {
            movers: vec!["door".into(), "lift".into()],
            zones: vec!["hall".into()],
            markers: vec!["spot".into()],
            enemies: vec!["boss".into()],
            tags: vec!["guards".into()],
        }
    }

    /// `|` marks the caret.
    fn run(text: &str) -> Option<(usize, Vec<Suggestion>)> {
        let caret = text.chars().position(|c| c == '|').expect("a caret");
        let clean = text.replace('|', "");
        suggest(&names(), &[], &clean, caret)
    }

    fn labels(text: &str) -> Vec<String> {
        let (_, items) = run(text).expect("suggestions");
        items.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn a_string_offers_the_level_names_that_message_wants() {
        assert_eq!(labels("x := mover: '|"), ["door", "lift"]);
        assert_eq!(labels("(zone: 'ha|')"), ["hall"]);
        assert_eq!(labels("marker: '|"), ["spot"]);
        assert_eq!(labels("markers: '|"), ["spot", "guards"]);
        assert_eq!(labels("enemy: '|"), ["boss"]);
        assert_eq!(labels("autoDoor: '|"), ["door", "lift"]);
        assert_eq!(labels("wave: '|"), ["hall"]);
    }

    #[test]
    fn the_word_replaced_starts_after_the_quote() {
        let (start, _) = run("zone: 'ha|").unwrap();
        assert_eq!(start, "zone: '".len());
    }

    #[test]
    fn a_string_nothing_asks_for_offers_nothing() {
        assert!(run("say: 'hel|").is_none());
        assert!(run("x := 'hel|").is_none());
    }

    #[test]
    fn a_comment_offers_nothing_even_when_it_looks_like_code() {
        assert!(run("\"mover: 'do|\"").is_none());
        assert!(run("\"pla|").is_none());
    }

    #[test]
    fn the_period_that_ends_a_statement_offers_nothing() {
        assert!(run("say: 'a'.|").is_none());
        assert!(run("x := 1.|").is_none());
        assert!(
            run("x := 1. |").is_some(),
            "Ctrl+Space after it still works"
        );
    }

    #[test]
    fn a_closed_comment_or_string_is_left_behind() {
        assert_eq!(labels("\"it's fine\" mover: '|"), ["door", "lift"]);
        assert!(labels("say: 'a'. pl|").contains(&"player".to_string()));
    }

    #[test]
    fn poses_tags_and_kinds_come_from_the_script_too() {
        let poses = labels("door pose: 'open' move: v. door pose: 'top' move: v. door moveTo: '|");
        assert_eq!(poses, ["closed", "open", "top"]);
        let tags = labels("spawn: 'drone' at: s tag: 'wave1'. tagged: '|");
        assert_eq!(tags, ["guards", "wave1"]);
        let kinds = labels("defineEnemy: 'sentry' base: 0. spawn: '|");
        assert!(kinds.contains(&"sentry".to_string()) && kinds.contains(&"crate".to_string()));
    }

    #[test]
    fn a_word_on_its_own_offers_variables_then_functions() {
        let got = labels("door := mover: 'door'.\nmo|");
        assert_eq!(got[0], "door");
        assert!(got.contains(&"mover:".to_string()));
        assert!(got.contains(&"say:".to_string()));
    }

    #[test]
    fn what_the_receiver_understands_comes_first() {
        let after_var = labels("door := mover: 'door'.\ndoor mo|");
        assert_eq!(after_var[0], "pose:move:");
        assert!(after_var.iter().position(|l| l == "moveTo:in:").unwrap() < 12);
        let after_paren = labels("(zone: 'hall') on|");
        assert_eq!(after_paren[0], "onEnter:");
        assert_eq!(labels("player he|")[0], "give:");
        assert_eq!(labels("(tagged: 'guards') al|")[0], "alive");
    }

    #[test]
    fn a_block_receiver_is_not_mistaken_for_a_function_call() {
        assert_eq!(labels("[x > 1] wh|")[0], "value");
    }

    #[test]
    fn the_scripts_and_the_librarys_functions_are_offered_with_their_arguments() {
        let library = crate::script::scan::defines("define wave: name kind: kind [ ]");
        let text = "define shout: what twice: n [ say: what ]\nsh";
        let (_, items) = suggest(&names(), &library, text, text.chars().count()).unwrap();
        let own = items.iter().find(|i| i.label == "shout:twice:").unwrap();
        assert_eq!(own.insert, "shout: what twice: n");
        let kit = items.iter().find(|i| i.label == "wave:kind:").unwrap();
        assert_eq!(kit.insert, "wave: name kind: kind");
    }

    #[test]
    fn opening_a_string_opens_the_popup_only_where_names_are_wanted() {
        let at = |t: &str| opens_names(t, t.chars().count());
        assert!(at("mover: '"));
        assert!(!at("say: '"));
        assert!(!at("mover: 'door'"));
        assert!(!at("mover: "));
        assert!(!at("\"mover: '"));
    }

    #[test]
    fn hostile_text_gets_an_answer_and_no_panic() {
        let cases = [
            "", "'", "\"", "(((((", ")))", "[:", "é'é'", "define", "define [", "x :=",
        ];
        for text in cases {
            for caret in 0..=text.chars().count() + 2 {
                let _ = suggest(&names(), &[], text, caret);
                let _ = opens_names(text, caret);
            }
        }
    }
}
