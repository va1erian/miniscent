//! A scripted input timeline for the headless backend.
//!
//! One command per line; `#` starts a comment. Times are seconds with an
//! optional `s` suffix.
//!
//! ```text
//! at 0.5s hold W            # key down until released
//! at 2s release W
//! at 1s hold D for 0.5s     # down, then up 0.5 s later
//! at 3s tap Space           # down and up 50 ms later
//! at 1s mouse 40 0          # one relative motion
//! at 1s mouse 120 -30 over 0.5s   # spread evenly over 0.5 s
//! at 4s fire                # left button, 100 ms
//! at 4s fire for 2s         # left button held for 2 s
//! at 9s quit
//! ```

use crate::key::{Event, Key, MouseButton};

/// Interval between the pieces of a `mouse ... over` motion.
const MOUSE_SLICE: f64 = 1.0 / 60.0;
/// How long `tap` and a bare `fire` keep the input down.
const TAP_SECONDS: f64 = 0.05;
const FIRE_SECONDS: f64 = 0.1;
/// Most pieces one `over` command may expand to.
const MAX_SLICES: usize = 10_000;

/// A parse failure with the 1-based line it happened on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}

impl core::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "script line {}: {}", self.line, self.message)
    }
}

/// What happens at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Input(Event),
    /// The script asks the run to end.
    Quit,
}

/// Actions sorted by time (seconds since the start of the run).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    actions: Vec<(f64, Action)>,
}

impl Script {
    /// A script that does nothing.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn actions(&self) -> &[(f64, Action)] {
        &self.actions
    }

    /// Parses script text.
    pub fn parse(text: &str) -> Result<Self, ScriptError> {
        let mut actions = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            parse_line(line, &mut actions).map_err(|message| ScriptError {
                line: i + 1,
                message,
            })?;
        }
        // Stable: commands at the same time keep their written order.
        actions.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Self { actions })
    }
}

fn seconds(word: Option<&&str>) -> Result<f64, String> {
    let w = word.ok_or("missing a time")?;
    let v: f64 = w
        .trim_end_matches('s')
        .parse()
        .map_err(|_| format!("bad time `{w}`"))?;
    if v.is_finite() && (0.0..=86_400.0).contains(&v) {
        Ok(v)
    } else {
        Err(format!("time `{w}` out of range"))
    }
}

fn number(word: Option<&&str>) -> Result<f32, String> {
    let w = word.ok_or("missing a number")?;
    let v: f32 = w.parse().map_err(|_| format!("bad number `{w}`"))?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("number `{w}` out of range"))
    }
}

fn key_of(word: Option<&&str>) -> Result<Key, String> {
    let w = word.ok_or("missing a key")?;
    Key::from_name(w).ok_or_else(|| format!("unknown key `{w}`"))
}

/// Parses an optional trailing `<word> <seconds>` pair.
fn optional_span(rest: &[&str], word: &str) -> Result<Option<f64>, String> {
    match rest {
        [] => Ok(None),
        [w, t] if *w == word => Ok(Some(seconds(Some(t))?)),
        _ => Err(format!("expected `{word} <seconds>`")),
    }
}

fn parse_line(line: &str, out: &mut Vec<(f64, Action)>) -> Result<(), String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let [at, t, cmd, rest @ ..] = words.as_slice() else {
        return Err("expected `at <time> <command>`".into());
    };
    if *at != "at" {
        return Err("a line starts with `at`".into());
    }
    let t = seconds(Some(t))?;
    let key = |code, pressed| Action::Input(Event::Key { code, pressed });
    let button = |pressed| {
        Action::Input(Event::MouseButton {
            button: MouseButton::Left,
            pressed,
        })
    };
    match *cmd {
        "hold" => {
            let k = key_of(rest.first())?;
            out.push((t, key(k, true)));
            if let Some(d) = optional_span(rest.get(1..).unwrap_or(&[]), "for")? {
                out.push((t + d, key(k, false)));
            }
        }
        "release" => out.push((t, key(key_of(rest.first())?, false))),
        "tap" => {
            let k = key_of(rest.first())?;
            out.push((t, key(k, true)));
            out.push((t + TAP_SECONDS, key(k, false)));
        }
        "fire" => {
            let d = optional_span(rest, "for")?.unwrap_or(FIRE_SECONDS);
            out.push((t, button(true)));
            out.push((t + d, button(false)));
        }
        "mouse" => {
            let (dx, dy) = (number(rest.first())?, number(rest.get(1))?);
            match optional_span(rest.get(2..).unwrap_or(&[]), "over")? {
                None => out.push((t, motion(dx, dy))),
                Some(d) => {
                    let n = ((d / MOUSE_SLICE).round() as usize).clamp(1, MAX_SLICES);
                    for i in 0..n {
                        let at = t + i as f64 * d / n as f64;
                        out.push((at, motion(dx / n as f32, dy / n as f32)));
                    }
                }
            }
        }
        "quit" if rest.is_empty() => out.push((t, Action::Quit)),
        other => return Err(format!("unknown command `{other}`")),
    }
    Ok(())
}

fn motion(dx: f32, dy: f32) -> Action {
    Action::Input(Event::MouseMotion { dx, dy })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_forms_in_time_order() {
        let s = Script::parse(
            "at 1s hold D for 0.5s\n# note\nat 0.5s hold W\nat 2 release W # end\nat 3s quit\n",
        )
        .unwrap();
        let times: Vec<f64> = s.actions().iter().map(|a| a.0).collect();
        assert_eq!(times, vec![0.5, 1.0, 1.5, 2.0, 3.0]);
        assert_eq!(
            s.actions()[0].1,
            Action::Input(Event::Key {
                code: Key::Char('W'),
                pressed: true
            })
        );
        assert_eq!(s.actions()[4].1, Action::Quit);
    }

    #[test]
    fn mouse_over_spreads_the_motion_and_sums_to_the_total() {
        let s = Script::parse("at 0s mouse 120 -30 over 0.5s").unwrap();
        assert!(s.actions().len() > 20);
        let (mut sx, mut sy) = (0.0, 0.0);
        for (_, a) in s.actions() {
            if let Action::Input(Event::MouseMotion { dx, dy }) = a {
                sx += dx;
                sy += dy;
            }
        }
        assert!((sx - 120.0).abs() < 0.01 && (sy + 30.0).abs() < 0.01);
    }

    #[test]
    fn errors_name_the_line() {
        for (text, line) in [
            ("at 1s hold\n", 1),
            ("\nat x hold W\n", 2),
            ("go\n", 1),
            ("at 1s hold Q9\n", 1),
            ("at 1s mouse 1\n", 1),
            ("at 1s mouse 1 2 over\n", 1),
            ("at 1e99 quit\n", 1),
            ("at 1s dance\n", 1),
        ] {
            assert_eq!(Script::parse(text).unwrap_err().line, line, "{text:?}");
        }
    }
}
