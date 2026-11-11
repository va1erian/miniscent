//! What happened in a run, one JSON object per line, for an agent to read.
//! The same run gives the same bytes, so two traces can simply be compared.

use std::io::Write;

use mscent_collide::STEP_DT;
use mscent_game::{Event, Game};

/// JSON string contents: quotes, backslashes and control characters escaped.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn thing(game: &Game, kind: &str, index: usize) -> String {
    game.thing_name(kind, index)
        .unwrap_or_else(|| format!("{kind}_{index}"))
}

/// The fields after `"t"` for an event, or `None` for what no one needs to read
/// through: every shot an enemy fires would bury everything else.
fn event_fields(game: &Game, event: &Event) -> Option<String> {
    let named = |ev: &str, field: &str, kind: &str, i: usize| {
        let name = escape(&thing(game, kind, i));
        format!("\"ev\":\"{ev}\",\"{field}\":\"{name}\"")
    };
    Some(match *event {
        Event::EnemyFired(_) | Event::PlayerFired | Event::Shot { .. } => return None,
        Event::EnemyHit(i) => named("hit", "who", "enemy", i),
        Event::EnemyKilled(i) => named("destroyed", "who", "enemy", i),
        Event::PlayerHurt(n) => format!("\"ev\":\"hurt\",\"damage\":{n}"),
        Event::PlayerDied => "\"ev\":\"died\"".to_string(),
        Event::GoalReached => "\"ev\":\"goal\"".to_string(),
        Event::Restarted => "\"ev\":\"restarted\"".to_string(),
        Event::ScriptWon => "\"ev\":\"won\"".to_string(),
        Event::ScriptLost => "\"ev\":\"lost\"".to_string(),
        Event::MoverSent(i) => {
            // An instant move has arrived by the time its first step is written.
            let pose = game
                .mover(i)
                .and_then(|m| m.target_name().or(m.pose_name()));
            let pose = pose.unwrap_or("");
            let pose = escape(pose);
            let head = named("move_to", "mover", "mover", i);
            format!("{head},\"pose\":\"{pose}\"")
        }
        Event::MoverArrived(i) => {
            let pose = game.mover(i).and_then(|m| m.pose_name()).unwrap_or("");
            let pose = escape(pose);
            let head = named("arrived", "mover", "mover", i);
            format!("{head},\"pose\":\"{pose}\"")
        }
        Event::MoverBlocked(i) => named("blocked", "mover", "mover", i),
        Event::MoverHit(i) => named("mover_hit", "mover", "mover", i),
        Event::ZoneEntered(i) => named("enter", "zone", "zone", i),
        Event::ZoneLeft(i) => named("exit", "zone", "zone", i),
        Event::EnemySpawned(i) => named("spawned", "who", "enemy", i),
        Event::PropSpawned(i) => named("spawned", "who", "prop", i),
        Event::PropHit(i) => named("hit", "who", "prop", i),
        Event::PropDestroyed(i) => named("destroyed", "who", "prop", i),
        Event::PropPicked(i) => named("pickup", "who", "prop", i),
        Event::PropUsed(i) => named("used", "who", "prop", i),
        Event::Disembarked => "\"ev\":\"disembarked\"".to_string(),
        Event::Boarded => "\"ev\":\"boarded\"".to_string(),
        Event::BoardRefused => "\"ev\":\"board_refused\"".to_string(),
    })
}

/// Writes a run's trace as it goes.
pub struct Trace<W: Write> {
    out: W,
    step: u64,
}

impl<W: Write> Trace<W> {
    pub fn new(out: W) -> Self {
        Self { out, step: 0 }
    }

    fn line(&mut self, fields: &str) -> std::io::Result<()> {
        let t = self.step as f64 * f64::from(STEP_DT);
        writeln!(self.out, "{{\"t\":{t:.3},{fields}}}")
    }

    /// Call after every step; it takes the game's events and messages.
    pub fn record_step(&mut self, game: &mut Game) -> std::io::Result<()> {
        self.step += 1;
        for event in game.take_events() {
            if let Some(fields) = event_fields(game, &event) {
                self.line(&fields)?;
            }
        }
        for message in game.take_messages() {
            self.line(&format!("\"say\":\"{}\"", escape(&message)))?;
        }
        Ok(())
    }

    pub fn error(&mut self, error: &str) -> std::io::Result<()> {
        self.line(&format!("\"err\":\"{}\"", escape(error)))
    }

    pub fn check(&mut self, passed: bool, because: &str) -> std::io::Result<()> {
        self.line(&format!(
            "\"check\":{passed},\"because\":\"{}\"",
            escape(because)
        ))
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escape_leaves_nothing_that_breaks_a_json_string() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("a\"b\\c\nd\u{1}é"), "a\\\"b\\\\c\\nd\\u0001é");
    }
}
