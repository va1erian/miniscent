//! Reuse of baked lightmaps: a bake runs once per distinct set of inputs.
//!
//! [`BakeCache`] remembers the last bake in memory under a content key (see
//! [`mscent_world::lightmap::store::input_key`]) and can mirror it to a file
//! next to the level. A rebuild whose faces, lights and bake settings did not
//! change gets the same `Arc<Lightmap>` back without any shadow ray; a missing,
//! stale, damaged or hostile file is ignored and the level is baked again.

use std::path::PathBuf;
use std::sync::Arc;

use mscent_world::Bsp;
use mscent_world::lightmap::store::{self, input_key};
use mscent_world::lightmap::{BakeConfig, LightFace, Lightmap, LightmapError, PointLight, bake};

/// Where the last lightmap came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Baked,
    Memory,
    File,
}

/// One-slot cache of the most recent bake.
#[derive(Default)]
pub struct BakeCache {
    slot: Option<(u64, Arc<Lightmap>)>,
    path: Option<PathBuf>,
    bakes: u32,
    last: Option<Source>,
}

impl BakeCache {
    /// Memory only.
    pub fn new() -> Self {
        Self::default()
    }

    /// Memory plus a file (conventionally `<level>.msl.lit`), read on a
    /// memory miss and rewritten after every bake.
    pub fn with_file(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }

    /// How many bakes actually ran.
    pub fn bakes(&self) -> u32 {
        self.bakes
    }

    /// Where the lightmap handed out last came from.
    pub fn last_source(&self) -> Option<Source> {
        self.last
    }

    /// The lightmap for these inputs: from memory, from the file, or baked.
    pub(super) fn get_or_bake(
        &mut self,
        bsp: &Bsp,
        faces: &[LightFace],
        lights: &[PointLight],
        cfg: &BakeConfig,
    ) -> Result<Arc<Lightmap>, LightmapError> {
        let key = input_key(faces, lights, cfg);
        if let Some((_, map)) = self.slot.as_ref().filter(|(k, _)| *k == key) {
            self.last = Some(Source::Memory);
            return Ok(Arc::clone(map));
        }
        if let Some(map) = self.read_file(key, faces.len()) {
            let map = Arc::new(map);
            self.slot = Some((key, Arc::clone(&map)));
            self.last = Some(Source::File);
            return Ok(map);
        }
        let map = Arc::new(bake(bsp, faces, lights, cfg)?);
        self.bakes += 1;
        self.last = Some(Source::Baked);
        if let Some(path) = &self.path {
            // A cache that cannot be written is only a slower next start.
            let _ = std::fs::write(path, map.to_bytes(key));
        }
        self.slot = Some((key, Arc::clone(&map)));
        Ok(map)
    }

    fn read_file(&self, key: u64, face_count: usize) -> Option<Lightmap> {
        let path = self.path.as_ref()?;
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > store::MAX_BYTES as u64 {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let map = Lightmap::from_bytes(&bytes, key).ok()?;
        (map.projections().len() == face_count).then_some(map)
    }
}
