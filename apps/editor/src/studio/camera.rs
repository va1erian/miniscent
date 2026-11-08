//! The fly camera of the 3D pane, as the rest of the editor reads it.

use super::Studio;
use super::worker::Worker;
use crate::model::{Document, FlyCam};

impl Studio {
    /// Puts the fly camera back where a new document starts.
    pub fn reset_camera(&mut self, doc: &Document) {
        self.set_camera(FlyCam::for_document(doc));
    }

    /// The fly camera as it is right now.
    pub fn fly_camera(&self) -> FlyCam {
        self.worker
            .as_ref()
            .and_then(Worker::current_camera)
            .unwrap_or_else(|| self.camera.clone())
    }

    /// The camera the 3D pane draws from, when it can be read without
    /// waiting: while the render thread holds it, `None`, not an older pose.
    pub fn shown_camera(&self) -> Option<FlyCam> {
        match &self.worker {
            Some(w) => w.current_camera(),
            None => Some(self.camera.clone()),
        }
    }
}
