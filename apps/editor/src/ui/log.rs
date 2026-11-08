//! The log box: what stands in the way of the level now, kept at the top where
//! it cannot scroll away, then the newest lines of the log. The box cannot be
//! scrolled, so it shows only what fits.

use xui::widget::HasText;

use super::app::Editor;
use super::wrap::{LOG_COLS, LOG_ROWS, wrap};

/// Lines of the log kept, oldest dropped first.
const KEPT: usize = 300;

/// The box's text: `problems` first (at most all but two rows), a rule, then the
/// newest lines of `log` that fit.
fn compose(problems: &[String], log: &[String]) -> String {
    let mut rows: Vec<String> = problems
        .iter()
        .flat_map(|p| wrap(p, LOG_COLS))
        .take(LOG_ROWS.saturating_sub(2))
        .collect();
    if !rows.is_empty() {
        rows.push("-".repeat(LOG_COLS / 2));
    }
    let room = LOG_ROWS.saturating_sub(rows.len());
    rows.extend_from_slice(&log[log.len().saturating_sub(room)..]);
    rows.join("\n")
}

impl Editor {
    /// Adds lines to the log.
    pub(super) fn push_log(&mut self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        for line in lines {
            self.log.extend(wrap(line, LOG_COLS));
        }
        let excess = self.log.len().saturating_sub(KEPT);
        self.log.drain(..excess);
        self.show_log();
    }

    /// Keeps `problems` on show while they last.
    pub(super) fn show_problems(&mut self, problems: Vec<String>) {
        if problems != self.problems {
            self.problems = problems;
            self.show_log();
        }
    }

    fn show_log(&self) {
        let text = compose(&self.problems, &self.log);
        self.log_view.get().set_text(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_stay_on_top_however_much_is_logged() {
        let log: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        let problems = ["Cannot play: the level has no goal: place one".to_string()];
        let text = compose(&problems, &log);
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(rows.len(), LOG_ROWS);
        assert!(rows[0].starts_with("Cannot play"), "{text}");
        assert_eq!(rows[LOG_ROWS - 1], "line 49");
        assert_eq!(compose(&[], &log).lines().count(), LOG_ROWS);
    }
}
