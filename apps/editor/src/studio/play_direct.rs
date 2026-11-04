//! Play shown straight in the Play pane's own window, as the game shows itself:
//! the GPU draws the world and composites the HUD layer over it in the window,
//! and nothing is read back or passes through the UI thread
//! (`docs/hardware-renderer.md`). The game's own [`GpuWorld`] does it, on the
//! editor's device. When it cannot, the pane gets frames again.

use miniscent_game::GameSession;
use miniscent_game::app::{Shown, World, shown_sizes};
use miniscent_game::cli::BASE_RES;
use miniscent_game::gpu_world::GpuWorld;
use mscent_render::Framebuffer;

use super::gpu_view;
use super::worker::{Request, Surface};

/// The window Play shows in, while it can.
#[derive(Default)]
pub struct Direct {
    window: Option<Surface>,
    world: Option<GpuWorld>,
    /// The window was given up since the UI last asked.
    lost: bool,
}

impl Direct {
    /// Shows Play in `window` from the next frame, or (`None`) no longer.
    pub fn set_surface(&mut self, window: Option<Surface>) {
        self.window = window;
        self.lost = false;
        self.remake();
    }

    /// Draws the level as it is now, for one whose light changed: the GPU's
    /// copy of it is made again, in the same window.
    pub fn level_changed(&mut self) {
        if self.world.is_some() {
            self.remake();
        }
    }

    /// A world on the editor's device in the window, the old one's surface
    /// gone first, as a window takes one swap chain at a time.
    fn remake(&mut self) {
        self.world = None;
        let Some(window) = self.window.clone() else {
            return;
        };
        if !gpu_view::wanted() {
            self.give_up("the software renderer is chosen");
            return;
        }
        match gpu_view::device() {
            Ok(gpu) => self.world = Some(GpuWorld::on(gpu, false).in_window(window)),
            Err(why) => self.give_up(&why),
        }
    }

    /// Leaves Play to the pane's frames, and says why in the log.
    fn give_up(&mut self, why: &str) {
        self.world = None;
        self.window = None;
        self.lost = true;
        gpu_view::report(format!("Play: in the pane, because {why}"));
    }

    /// Whether the window was given up since the last call.
    pub fn take_lost(&mut self) -> bool {
        std::mem::take(&mut self.lost)
    }

    /// Whether Play shows in the window.
    pub fn shows(&self) -> bool {
        self.world.as_ref().is_some_and(World::shows)
    }

    /// Draws Play's frame for `req` and shows it in the window. `Ok` when it
    /// is shown; otherwise the window is given up, and the frame software drew
    /// instead (if any) is for the pane.
    pub fn show<'s>(
        &mut self,
        session: &'s mut GameSession,
        req: &Request,
    ) -> Result<(), Option<&'s Framebuffer>> {
        let Some(world) = self.world.as_mut() else {
            return Err(None);
        };
        // The world at the size the budget allows, filling the window.
        let sizes = shown_sizes(req.pane, BASE_RES, |_| (req.w, req.h));
        let shown = session.render_layered(sizes, &mut |assets, frame, layer, white| {
            let over = Shown {
                layer,
                white,
                window: req.pane,
                world: sizes.0,
            };
            world.show(assets, frame, &over)
        });
        match shown {
            Some(None) => {
                gpu_view::set_active();
                Ok(())
            }
            drawn => {
                let why = world
                    .failure()
                    .unwrap_or("the GPU drew no frame")
                    .to_string();
                self.give_up(&why);
                Err(drawn.flatten())
            }
        }
    }
}
