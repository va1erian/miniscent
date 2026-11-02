//! What the editor does on every tick of the UI's timer.

use super::{Editor, Mode};

impl Editor {
    /// One UI tick: forwards the active pane's input to the render thread
    /// and shows the newest frame it finished. Nothing here waits.
    pub(super) fn advance(&mut self) {
        // Esc or focus loss ended the pointer lock: the cursor is back, so
        // Play ends rather than running with a free cursor.
        if self.studio.mode == Mode::Play && self.play.get().take_lock_lost() {
            self.end_play(Some("Back in the editor"));
        }
        let pane = if self.studio.mode == Mode::Play {
            self.play.get()
        } else {
            self.pane3d()
        };
        let events = pane.take_events();
        let clicks = pane.take_clicks();
        let drags = pane.take_drags();
        let wheel = pane.take_wheel();
        let size = pane.size();
        let state = self.state.clone();
        let sculpted = state.borrow_mut().poll_stroke();
        self.studio.set_stroking(state.borrow().stroking());
        let tick = self.studio.tick(&events, size, &state.borrow().doc);
        self.push_log(&tick.log);
        if let Some(s) = &tick.status {
            self.state.borrow_mut().message = s.clone();
            self.set_status(s);
        }
        if let Some(text) = &tick.blank {
            self.pane3d().show_text(text);
        }
        self.show_preview(self.studio.preview(&state.borrow().doc));
        if tick.play_now {
            self.begin_play();
        }
        if tick.left_play {
            self.end_play(None);
        }
        if let Some(f) = tick.frame {
            let target = match f.mode {
                Mode::Play => self.play.get(),
                Mode::Edit => self.pane3d(),
            };
            target.set_rgba(f.w, f.h, f.rgba);
        }
        if self.studio.mode == Mode::Edit {
            self.pane_wheel(wheel);
            let busy = sculpted || !clicks.is_empty() || !drags.is_empty() || wheel != 0;
            for click in clicks {
                self.pick_click(click);
            }
            for drag in drags {
                self.pane_drag(drag);
            }
            if busy {
                self.refresh();
            }
        }
    }
}
