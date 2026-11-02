//! What is drawn over the finished frame: HUD, stats, hit markers and menus.

use mscent_game::{HudModel, Screen, draw_hud, draw_menu};
use mscent_render::{Framebuffer, draw_text, fill_rect};

use super::GameSession;

impl GameSession {
    pub(super) fn draw_overlays(&self, fb: &mut Framebuffer) {
        draw_hud(fb, &HudModel::from_game(&self.game));
        let (w, h) = (fb.width() as i32, fb.height() as i32);
        let stats = format!("{:.0} FPS {:.1} MS", self.stats.fps, self.stats.frame_ms);
        let size = mscent_render::text_size(&stats, 1);
        draw_text(fb, w - size.0 as i32 - 8, 8, &stats, 0xffff_ffff, 1);
        let (cx, cy) = (w / 2, h / 2);
        if self.hit > 0 {
            fill_rect(fb, cx - 9, cy - 9, 4, 4, 0xffff_3030);
            fill_rect(fb, cx + 6, cy - 9, 4, 4, 0xffff_3030);
            fill_rect(fb, cx - 9, cy + 6, 4, 4, 0xffff_3030);
            fill_rect(fb, cx + 6, cy + 6, 4, 4, 0xffff_3030);
        }
        if self.kill > 0 {
            draw_text(fb, cx - 12, cy + 16, "KILL", 0xffff_d040, 1);
        }
        if self.menu.screen() == Screen::Won || self.menu.screen() == Screen::Lost {
            let hint = "R TO RESTART";
            let tw = mscent_render::text_size(hint, 1).0 as i32;
            draw_text(fb, (w - tw) / 2, h - 24, hint, 0xffc0_c0c0, 1);
        }
        draw_menu(fb, &self.menu);
    }
}
