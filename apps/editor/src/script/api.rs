//! Every message a script can send, with what the editor says about it: what
//! completion inserts and the one-line help beside it, and the guide's reference
//! page. A test compares this with the game's own table, so it cannot drift.

/// What a message is sent to, which puts it first in the completion list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recv {
    /// Nothing: a global function.
    Global,
    Mover,
    Zone,
    Group,
    Enemy,
    Marker,
    /// A prop from `spawn:at:tag:`.
    Prop,
    Player,
    /// What `defineEnemy:base:` or `defineProp:` returned.
    Kind,
    Array,
    Block,
    /// Any value: a number, a text or a true or false.
    Value,
}

impl Recv {
    /// A heading for the guide's reference page.
    pub fn title(self) -> &'static str {
        match self {
            Recv::Global => "Functions",
            Recv::Mover => "Sent to a mover",
            Recv::Zone => "Sent to a zone",
            Recv::Group => "Sent to a tagged group",
            Recv::Enemy => "Sent to an enemy",
            Recv::Marker => "Sent to a marker",
            Recv::Prop => "Sent to a spawned prop",
            Recv::Player => "Sent to the player",
            Recv::Kind => "Sent to a kind",
            Recv::Array => "Sent to an array",
            Recv::Block => "Sent to a block",
            Recv::Value => "Sent to any value",
        }
    }

    /// What a value made by this global function is.
    pub fn made_by(selector: &str) -> Option<Recv> {
        Some(match selector {
            "mover:" => Recv::Mover,
            "zone:" => Recv::Zone,
            "tagged:" => Recv::Group,
            "enemy:" => Recv::Enemy,
            "marker:" => Recv::Marker,
            "markers:" | "members" => Recv::Array,
            "spawn:" => Recv::Prop,
            "player" => Recv::Player,
            "defineEnemy:" | "defineProp:" => Recv::Kind,
            _ => return None,
        })
    }
}

/// One message.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub selector: &'static str,
    pub on: Recv,
    /// What completion puts in the text, with example arguments to overwrite.
    pub insert: &'static str,
    pub help: &'static str,
}

const fn e(selector: &'static str, on: Recv, insert: &'static str, help: &'static str) -> Entry {
    Entry {
        selector,
        on,
        insert,
        help,
    }
}

/// The game's functions, the language's own messages and the library in the
/// game and the language. Operators are not here: nothing completes them.
pub const API: &[Entry] = &[
    e(
        "mover:",
        Recv::Global,
        "mover: 'name'",
        "A named brush group that moves",
    ),
    e(
        "zone:",
        Recv::Global,
        "zone: 'name'",
        "A named box the player walks into",
    ),
    e("marker:", Recv::Global, "marker: 'name'", "A named point"),
    e(
        "markers:",
        Recv::Global,
        "markers: 'prefix'",
        "Markers by tag or by start of name",
    ),
    e(
        "enemy:",
        Recv::Global,
        "enemy: 'name'",
        "A named enemy placed in the level",
    ),
    e(
        "tagged:",
        Recv::Global,
        "tagged: 'tag'",
        "Every enemy and prop with a tag",
    ),
    e("player", Recv::Global, "player", "The player"),
    e(
        "spawn:at:tag:",
        Recv::Global,
        "spawn: 'drone' at: (marker: 'spot') tag: 'tag'",
        "Make one enemy or prop",
    ),
    e(
        "spawnAll:at:tag:",
        Recv::Global,
        "spawnAll: 'drone' at: (markers: 'prefix') tag: 'tag'",
        "One on each marker",
    ),
    e(
        "defineEnemy:base:",
        Recv::Global,
        "defineEnemy: 'name' base: 0",
        "A new enemy kind to tune",
    ),
    e(
        "defineProp:",
        Recv::Global,
        "defineProp: 'name'",
        "A new prop kind to tune",
    ),
    e(
        "say:",
        Recv::Global,
        "say: 'text'",
        "Show text on the screen",
    ),
    e("win", Recv::Global, "win", "End the level, won"),
    e("lose", Recv::Global, "lose", "End the level, lost"),
    e(
        "randomFrom:to:",
        Recv::Global,
        "randomFrom: 1 to: 6",
        "A repeatable random number",
    ),
    e(
        "x:y:z:",
        Recv::Global,
        "x: 0 y: 0 z: 4",
        "A position or direction",
    ),
    e(
        "wait:",
        Recv::Global,
        "wait: 1",
        "Pause this handler, in seconds",
    ),
    e("fork:", Recv::Global, "fork: [ ]", "Run a block alongside"),
    e(
        "after:do:",
        Recv::Global,
        "after: 2 do: [ ]",
        "Run a block once, later",
    ),
    e(
        "every:do:",
        Recv::Global,
        "every: 2 do: [ ]",
        "Run a block again and again",
    ),
    e(
        "waitUntil:",
        Recv::Global,
        "waitUntil: [ ]",
        "Pause until the block is true",
    ),
    e(
        "waitArrived:",
        Recv::Global,
        "waitArrived: door",
        "Pause until a mover stops",
    ),
    e(
        "waitDestroyed:",
        Recv::Global,
        "waitDestroyed: group",
        "Pause until a group is gone",
    ),
    e(
        "pose:move:",
        Recv::Mover,
        "pose: 'open' move: (x: 0 y: 0 z: 4)",
        "Define a pose by sliding",
    ),
    e(
        "pose:turn:degrees:about:",
        Recv::Mover,
        "pose: 'open' turn: (x: 1 y: 0 z: 0) degrees: 90 about: (marker: 'hinge')",
        "Define a pose by turning",
    ),
    e(
        "moveTo:in:",
        Recv::Mover,
        "moveTo: 'open' in: 1",
        "Go to a pose, in seconds",
    ),
    e(
        "poseNow",
        Recv::Mover,
        "poseNow",
        "The pose it is at, nil while moving",
    ),
    e("moving", Recv::Mover, "moving", "Whether it is on its way"),
    e(
        "whenBlocked:",
        Recv::Mover,
        "whenBlocked: 'reverse'",
        "'hold' or 'reverse'",
    ),
    e(
        "whenBlocked:damage:",
        Recv::Mover,
        "whenBlocked: 'crush' damage: 40",
        "Hold, and hurt what is in the way",
    ),
    e(
        "onArrived:",
        Recv::Mover,
        "onArrived: [ ]",
        "When it reaches a pose",
    ),
    e(
        "onEnter:",
        Recv::Zone,
        "onEnter: [ ]",
        "Each time the player walks in",
    ),
    e(
        "onceEnter:",
        Recv::Zone,
        "onceEnter: [ ]",
        "The first time the player walks in",
    ),
    e(
        "onExit:",
        Recv::Zone,
        "onExit: [ ]",
        "Each time the player walks out",
    ),
    e(
        "hasPlayer",
        Recv::Zone,
        "hasPlayer",
        "Whether the player is inside",
    ),
    e("alive", Recv::Group, "alive", "How many are left"),
    e("members", Recv::Group, "members", "The group as an array"),
    e(
        "onEachDestroyed:",
        Recv::Group,
        "onEachDestroyed: [ ]",
        "When any one of them dies",
    ),
    e(
        "onAllDestroyed:",
        Recv::Group,
        "onAllDestroyed: [ ]",
        "When the last one dies",
    ),
    e("enable", Recv::Group, "enable", "Wake them up"),
    e("disable", Recv::Group, "disable", "Put them to sleep"),
    e(
        "setFaction:",
        Recv::Group,
        "setFaction: 'friendly'",
        "'friendly' or 'hostile'",
    ),
    e(
        "onPickup:",
        Recv::Prop,
        "onPickup: [ ]",
        "When the player touches it",
    ),
    e(
        "onUse:hold:",
        Recv::Prop,
        "onUse: [ ] hold: 2",
        "When the use key is held on it",
    ),
    e("onHit:", Recv::Prop, "onHit: [ ]", "When it is shot"),
    e(
        "onDestroyed:",
        Recv::Prop,
        "onDestroyed: [ ]",
        "When it is destroyed",
    ),
    e("destroy", Recv::Prop, "destroy", "Remove it"),
    e(
        "give:",
        Recv::Player,
        "give: 'key'",
        "Give the player a key",
    ),
    e(
        "has:",
        Recv::Player,
        "has: 'key'",
        "Whether the player holds a key",
    ),
    e("hp", Recv::Player, "hp", "Health left"),
    e("heal:", Recv::Player, "heal: 25", "Give health back"),
    e("pos", Recv::Player, "pos", "Where it is"),
    e(
        "onFoot",
        Recv::Player,
        "onFoot",
        "Whether the pilot is out of the ship",
    ),
    e("hp:", Recv::Kind, "hp: 80", "Health of this kind"),
    e(
        "speed:",
        Recv::Kind,
        "speed: 4",
        "How fast this enemy moves",
    ),
    e("radius:", Recv::Kind, "radius: 1", "How big this kind is"),
    e(
        "damage:",
        Recv::Kind,
        "damage: 10",
        "What one of its attacks does",
    ),
    e(
        "cooldown:",
        Recv::Kind,
        "cooldown: 1",
        "Seconds between its attacks",
    ),
    e(
        "sightRange:",
        Recv::Kind,
        "sightRange: 30",
        "How far it sees",
    ),
    e(
        "attackRange:",
        Recv::Kind,
        "attackRange: 20",
        "How far it attacks",
    ),
    e(
        "mesh:",
        Recv::Kind,
        "mesh: 'crate'",
        "What this prop looks like",
    ),
    e(
        "do:",
        Recv::Array,
        "do: [:each | ]",
        "Run a block on each item",
    ),
    e("size", Recv::Array, "size", "How many items or characters"),
    e("at:", Recv::Array, "at: 1", "The item at a place, from 1"),
    e(
        "at:put:",
        Recv::Array,
        "at: 1 put: 0",
        "Change the item at a place",
    ),
    e("add:", Recv::Array, "add: 0", "Add an item at the end"),
    e("value", Recv::Block, "value", "Run the block"),
    e("value:", Recv::Block, "value: 1", "Run it with a value"),
    e(
        "value:value:",
        Recv::Block,
        "value: 1 value: 2",
        "Run it with two values",
    ),
    e(
        "whileTrue:",
        Recv::Block,
        "whileTrue: [ ]",
        "Loop while this block is true",
    ),
    e(
        "whileFalse:",
        Recv::Block,
        "whileFalse: [ ]",
        "Loop until this block is true",
    ),
    e(
        "timesRepeat:",
        Recv::Value,
        "timesRepeat: [ ]",
        "Run a block this many times",
    ),
    e(
        "ifTrue:",
        Recv::Value,
        "ifTrue: [ ]",
        "Run a block when true",
    ),
    e(
        "ifFalse:",
        Recv::Value,
        "ifFalse: [ ]",
        "Run a block when false",
    ),
    e(
        "ifTrue:ifFalse:",
        Recv::Value,
        "ifTrue: [ ] ifFalse: [ ]",
        "Choose between two blocks",
    ),
    e(
        "ifFalse:ifTrue:",
        Recv::Value,
        "ifFalse: [ ] ifTrue: [ ]",
        "Choose between two blocks",
    ),
    e(
        "and:",
        Recv::Value,
        "and: [ ]",
        "Also true, only tried when needed",
    ),
    e(
        "or:",
        Recv::Value,
        "or: [ ]",
        "Or true, only tried when needed",
    ),
    e("not", Recv::Value, "not", "The opposite"),
    e("isNil", Recv::Value, "isNil", "Whether it is nil"),
];

/// The prelude's and the game library's functions, which no table lists.
#[cfg(test)]
pub(super) const WRITTEN_IN_SCRIPT: &[&str] = &[
    "do:",
    "timesRepeat:",
    "waitUntil:",
    "every:do:",
    "after:do:",
    "waitArrived:",
    "waitDestroyed:",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_table_holds_exactly_what_the_game_and_the_language_provide() {
        let game = mscent_game::natives();
        let language = mscent_script::language_selectors();
        let mut wanted: BTreeSet<&str> = game.iter().map(|n| n.0).collect();
        wanted.extend(language.iter().copied());
        wanted.extend(WRITTEN_IN_SCRIPT);
        // Operators have no letters, so nothing completes them.
        wanted.retain(|s| s.starts_with(|c: char| c.is_ascii_alphabetic()));
        let have: BTreeSet<&str> = API.iter().map(|e| e.selector).collect();
        let missing: Vec<_> = wanted.difference(&have).collect();
        let extra: Vec<_> = have.difference(&wanted).collect();
        assert!(missing.is_empty(), "not in the table: {missing:?}");
        assert!(extra.is_empty(), "no longer exists: {extra:?}");
    }

    #[test]
    fn what_completion_inserts_sends_the_message_it_says_it_does() {
        for entry in API {
            let parts: Vec<&str> = entry.selector.split_inclusive(':').collect();
            let mut rest = entry.insert;
            for part in parts {
                let at = rest
                    .find(part)
                    .unwrap_or_else(|| panic!("{} inserts {}", entry.selector, entry.insert));
                rest = &rest[at + part.len()..];
            }
        }
    }
}
