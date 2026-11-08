//! Enemy kinds as text: a `kind` line with the stats, the blueprint's lines each
//! behind a `|`, and `end`.
//!
//! ```text
//! kind hornet hp=40 radius=0.75 speed=9 sight=40 attack=8 damage=4 cooldown=0.4 lose=3
//! | miniscent-blueprint 1
//! | name hornet
//! end
//! ```
//!
//! Every stat must be given, in any order. The lines behind `|` are the
//! blueprint exactly, comments included, so they are not read as level text.

use super::TextError;
use super::words::{Res, Words, clip};
use crate::kinds::{Kind, KindStats};
use crate::model::{self, ModelError};

/// The words a kind's stats are written with, in the order they are written.
const STATS: [&str; 8] = [
    "hp", "radius", "speed", "sight", "attack", "damage", "cooldown", "lose",
];

/// Whether `line` opens a kind block.
pub(super) fn opens(line: &str) -> bool {
    line.split_whitespace().next() == Some("kind")
}

/// `k` as a block of text, ending in a newline.
pub(super) fn kind_text(k: &Kind) -> String {
    let s = &k.stats;
    let mut out = format!(
        "kind {} hp={} radius={} speed={} sight={} attack={} damage={} cooldown={} lose={}\n",
        k.name, s.hp, s.radius, s.speed, s.sight, s.attack_range, s.damage, s.cooldown, s.lose
    );
    for line in model::write(&k.blueprint).lines() {
        out += if line.is_empty() { "|" } else { "| " };
        out += line;
        out.push('\n');
    }
    out += "end\n";
    out
}

/// Reads the block that `header`, on line `at`, opens, taking its lines from
/// `lines`, which give 0-based line indices.
pub(super) fn read<'a>(
    at: usize,
    header: &str,
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
) -> Result<Kind, TextError> {
    let on = |line: usize| move |msg: String| TextError { line, msg };
    let (name, stats) = read_header(header).map_err(on(at))?;
    let (text, origins) = read_body(at, lines)?;
    let blueprint = model::read(text.as_bytes()).map_err(|e| match e {
        ModelError::Parse { line, why } => TextError {
            line: line
                .checked_sub(1)
                .and_then(|i| origins.get(i))
                .copied()
                .unwrap_or(at),
            msg: format!("in kind '{}': {why}", clip(&name)),
        },
        other => on(at)(format!("in kind '{}': {other}", clip(&name))),
    })?;
    Ok(Kind {
        name,
        stats,
        blueprint,
    })
}

/// The blueprint's text, and the file line each of its lines came from.
fn read_body<'a>(
    at: usize,
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
) -> Result<(String, Vec<usize>), TextError> {
    let (mut text, mut origins) = (String::new(), Vec::new());
    for (i, line) in lines {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('|') {
            text += rest.strip_prefix(' ').unwrap_or(rest);
            text.push('\n');
            origins.push(i + 1);
            if text.len() > model::MAX_FILE {
                return Err(TextError {
                    line: i + 1,
                    msg: "the blueprint is too large".to_string(),
                });
            }
            continue;
        }
        match trimmed.split('#').next().unwrap_or("").trim() {
            "end" => return Ok((text, origins)),
            "" => {}
            other => {
                return Err(TextError {
                    line: i + 1,
                    msg: format!("expected '|' or end in a kind, not '{}'", clip(other)),
                });
            }
        }
    }
    Err(TextError {
        line: at,
        msg: "this kind has no end".to_string(),
    })
}

fn read_header(header: &str) -> Res<(String, KindStats)> {
    let code = header.split('#').next().unwrap_or("");
    let mut w = Words {
        rest: code.split_whitespace(),
    };
    w.word("kind")?;
    let name = w.word("the kind's name")?.to_string();
    let mut values: [Option<&str>; 8] = [None; 8];
    for word in w.rest {
        let (key, value) = word
            .split_once('=')
            .ok_or_else(|| format!("expected stat=value, not '{}'", clip(word)))?;
        let slot = STATS
            .iter()
            .position(|&s| s == key)
            .ok_or_else(|| format!("no stat '{}'", clip(key)))?;
        if values[slot].replace(value).is_some() {
            return Err(format!("{} is given twice", STATS[slot]));
        }
    }
    let got = |i: usize| values[i].ok_or_else(|| format!("{} is missing", STATS[i]));
    let int = |i: usize| -> Res<u32> {
        let v = got(i)?;
        v.parse()
            .map_err(|_| format!("{} must be a whole number, not '{}'", STATS[i], clip(v)))
    };
    let num = |i: usize| -> Res<f32> {
        let v = got(i)?;
        match v.parse::<f32>() {
            Ok(x) if x.is_finite() => Ok(x),
            _ => Err(format!(
                "{} must be a finite number, not '{}'",
                STATS[i],
                clip(v)
            )),
        }
    };
    let stats = KindStats {
        hp: int(0)?,
        radius: num(1)?,
        speed: num(2)?,
        sight: num(3)?,
        attack_range: num(4)?,
        damage: int(5)?,
        cooldown: num(6)?,
        lose: num(7)?,
    };
    Ok((name, stats))
}
