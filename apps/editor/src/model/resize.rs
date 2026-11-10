//! Setting a brush's edges by number, which is the one way to resize a voxel
//! volume that needs no handle: it lands on the chunk grid, so the panel shows
//! where it really went.

use mscent_world::voxel::{CHUNK_EDGE, MAX_CHUNKS_PER_AXIS};

use super::state::EditorState;

impl EditorState {
    /// Sets one edge of the selected box or volume. A volume snaps it to the
    /// nearest chunk edge, so a value that rounds to where the edge already is
    /// changes nothing and says why. Whether the document changed.
    pub fn set_selected_edge(&mut self, axis: usize, high: bool, value: f64) -> bool {
        let Some(def) = self.selected_brush() else {
            return false;
        };
        let (mut lo, mut hi) = def.bounds();
        if axis >= 3 {
            return false;
        }
        if high {
            hi[axis] = value;
        } else {
            lo[axis] = value;
        }
        let volume = def.volume().map(|v| v.cell().units() * CHUNK_EDGE as f64);
        let Some(next) = def.with_bounds(lo, hi) else {
            if volume.is_some() {
                self.message =
                    format!("A volume has between 1 and {MAX_CHUNKS_PER_AXIS} chunks on an axis");
            }
            return false;
        };
        if let (true, Some(chunk)) = (next.bounds() == def.bounds(), volume) {
            self.message = format!("Volume edges snap to whole chunks of {chunk} units");
            return false;
        }
        self.edit_selected_brush(|b| *b = next)
    }
}
