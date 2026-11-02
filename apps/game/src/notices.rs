//! What a script said, and what went wrong with it, on screen for a few seconds.

use mscent_render::{Framebuffer, draw_text};

/// Lines on screen at once; older ones make way.
const MAX_LINES: usize = 4;
/// Longest line drawn, so a long message cannot run off every screen.
const MAX_CHARS: usize = 60;
/// Frames a line stays, at about 60 a second.
const LIFETIME_FRAMES: u32 = 300;
const LINE_HEIGHT: i32 = 10;
const TOP: i32 = 24;
const MESSAGE: u32 = 0xfff0_f0d0;
const ERROR: u32 = 0xffff_5050;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    text: String,
    color: u32,
    frames: u32,
}

/// The lines on screen, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notices {
    lines: Vec<Line>,
}

impl Notices {
    fn push(&mut self, text: &str, color: u32) {
        if self.lines.len() >= MAX_LINES {
            self.lines.remove(0);
        }
        self.lines.push(Line {
            // The font draws capitals, as the rest of the HUD does.
            text: text
                .chars()
                .take(MAX_CHARS)
                .collect::<String>()
                .to_uppercase(),
            color,
            frames: LIFETIME_FRAMES,
        });
    }

    pub fn message(&mut self, text: &str) {
        self.push(text, MESSAGE);
    }

    pub fn error(&mut self, text: &str) {
        self.push(text, ERROR);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Draws this frame's lines and ages them.
    pub fn draw(&mut self, fb: &mut Framebuffer) {
        for (i, line) in self.lines.iter().enumerate() {
            draw_text(
                fb,
                8,
                TOP + i as i32 * LINE_HEIGHT,
                &line.text,
                line.color,
                1,
            );
        }
        self.lines.iter_mut().for_each(|l| l.frames -= 1);
        self.lines.retain(|l| l.frames > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(fb: &Framebuffer) -> usize {
        fb.pixels().iter().filter(|&&p| p & 0xff_ffff != 0).count()
    }

    #[test]
    fn a_line_shows_then_goes() {
        let mut n = Notices::default();
        let mut fb = Framebuffer::new(320, 180).unwrap();
        n.message("vault unlocked");
        n.draw(&mut fb);
        assert!(lit(&fb) > 0);
        for _ in 0..LIFETIME_FRAMES {
            n.draw(&mut fb);
        }
        assert!(n.lines.is_empty());
    }

    #[test]
    fn old_lines_make_way_and_long_ones_are_cut() {
        let mut n = Notices::default();
        for i in 0..10 {
            n.error(&format!("line {i}"));
        }
        assert_eq!(n.lines.len(), MAX_LINES);
        assert_eq!(n.lines[0].text, "LINE 6");
        n.message(&"x".repeat(1000));
        assert_eq!(n.lines[MAX_LINES - 1].text.len(), MAX_CHARS);
    }
}
