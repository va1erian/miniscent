//! Reuse of baked lightmaps: a bake runs once per distinct set of inputs.
//!
//! [`BakeCache`] remembers the last bake in memory under a content key (see
//! [`mscent_world::lightmap::store::input_key`]) and can mirror it to a file
//! next to the level (`<level>.msl.lit`). The key covers face geometry,
//! lights, the bake settings and the baker's version, and nothing else: a new
//! texture mapping or camera does not invalidate it. A rebuild whose key did
//! not change gets the same `Arc<Lightmap>` back without any shadow ray; a
//! missing, stale, damaged or hostile file is ignored (with a reason in the
//! [`BakeReport`]) and the level is baked again. Files are written through a
//! temporary file and renamed, so they are never half written.
//!
//! A slow bake need not hold the cache: [`super::Compiled`] asks it for a
//! stored lightmap, bakes with no reference to it and hands the result back,
//! so a shared cache stays usable for the whole bake.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use mscent_world::Bsp;
use mscent_world::lightmap::store::{self, StoreError, input_key};
use mscent_world::lightmap::{BakeConfig, LightFace, Lightmap, LightmapError, PointLight, bake};

/// Where the last lightmap came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Baked,
    Memory,
    File,
}

/// What the last [`BakeCache`] lookup did, for logs and status bars.
#[derive(Clone, Debug, PartialEq)]
pub struct BakeReport {
    pub source: Source,
    /// Time of the bake or the file read; zero for a memory hit.
    pub ms: f64,
    /// The cache file consulted or written.
    pub path: Option<PathBuf>,
    /// This lookup wrote the cache file.
    pub written: bool,
    /// Why an existing file was ignored, or why a write failed.
    pub note: Option<String>,
}

impl BakeReport {
    /// One log line: baked vs cached, with timing and the file involved.
    pub fn describe(&self) -> String {
        let file = self
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let mut line = match self.source {
            Source::Memory => "lightmap reused from the last bake (no bake ran)".to_string(),
            Source::File => format!("lightmap loaded from cache ({:.0} ms): {file}", self.ms),
            Source::Baked if self.written => {
                format!("lightmap baked in {:.0} ms (written to {file})", self.ms)
            }
            Source::Baked => format!("lightmap baked in {:.0} ms (not cached on disk)", self.ms),
        };
        if let Some(n) = &self.note {
            line.push_str("; ");
            line.push_str(n);
        }
        line
    }
}

/// One-slot cache of the most recent bake.
#[derive(Default)]
pub struct BakeCache {
    slot: Option<(u64, Arc<Lightmap>)>,
    path: Option<PathBuf>,
    bakes: u32,
    last: Option<BakeReport>,
    rebake: bool,
    /// Why the last lookup refused the file, for the report of the bake it
    /// caused.
    miss_note: Option<String>,
}

/// A lightmap baked outside the cache, ready for [`BakeCache::store`].
pub struct BakedLight {
    pub(super) key: u64,
    pub(super) map: Lightmap,
    pub(super) ms: f64,
}

/// Why a cache file cannot be used, in words.
fn why(e: StoreError) -> String {
    match e {
        StoreError::BadMagic => "not a lightmap file".into(),
        StoreError::UnsupportedVersion(v) => format!("written by bake version {v}"),
        StoreError::Truncated => "truncated".into(),
        StoreError::TooLarge => "too large".into(),
        StoreError::KeyMismatch => "stale: the level, lights or bake settings changed".into(),
        StoreError::Corrupt => "damaged (checksum or range check failed)".into(),
    }
}

static WRITES: AtomicU32 = AtomicU32::new(0);

/// Writes `bytes` to `path` through a temporary file in the same directory,
/// so a crash or a second process never leaves a half-written cache.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut tmp = path.as_os_str().to_owned();
    // Process id plus a counter: two writers of one process (a build and a
    // Save) must not share a temporary file either.
    let n = WRITES.fetch_add(1, Ordering::Relaxed);
    tmp.push(format!(".{}.{n}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
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

    /// Points the cache at another file (or none). The memory slot stays.
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.path = path;
    }

    /// The next lookup bakes even when memory or the file would serve.
    pub fn force_rebake(&mut self) {
        self.rebake = true;
    }

    /// How many bakes actually ran.
    pub fn bakes(&self) -> u32 {
        self.bakes
    }

    /// Where the lightmap handed out last came from.
    pub fn last_source(&self) -> Option<Source> {
        self.last.as_ref().map(|r| r.source)
    }

    /// Details of the last lookup.
    pub fn last_report(&self) -> Option<&BakeReport> {
        self.last.as_ref()
    }

    /// Writes the held lightmap to `path` and makes that the cache file.
    /// `Ok(false)` when nothing is held yet.
    pub fn save_to(&mut self, path: PathBuf) -> Result<bool, String> {
        let wrote = match &self.slot {
            Some((key, map)) => {
                write_atomic(&path, &map.to_bytes(*key))?;
                true
            }
            None => false,
        };
        self.path = Some(path);
        Ok(wrote)
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
        self.miss_note = None;
        if !std::mem::take(&mut self.rebake)
            && let Some(map) = self.lookup(key, faces.len())
        {
            return Ok(map);
        }
        let t = Instant::now();
        let map = bake(bsp, faces, lights, cfg)?;
        let ms = ms_since(t);
        Ok(self.store(BakedLight { key, map, ms }))
    }

    /// The lightmap for `key` from memory or the file, without baking.
    pub(super) fn lookup(&mut self, key: u64, face_count: usize) -> Option<Arc<Lightmap>> {
        let mut report = self.new_report();
        if let Some((_, map)) = self.slot.as_ref().filter(|(k, _)| *k == key) {
            let map = Arc::clone(map);
            self.last = Some(report);
            return Some(map);
        }
        let t = Instant::now();
        match self.read_file(key, face_count) {
            Ok(Some(map)) => {
                let map = Arc::new(map);
                report.source = Source::File;
                report.ms = ms_since(t);
                self.last = Some(report);
                self.slot = Some((key, Arc::clone(&map)));
                Some(map)
            }
            Ok(None) => None,
            Err(e) => {
                self.miss_note = Some(format!("cache file ignored: {e}"));
                None
            }
        }
    }

    /// Takes in a finished bake: it becomes the held lightmap and is mirrored
    /// to the file.
    pub fn store(&mut self, baked: BakedLight) -> Arc<Lightmap> {
        let BakedLight { key, map, ms } = baked;
        let map = Arc::new(map);
        let mut report = self.new_report();
        report.source = Source::Baked;
        report.ms = ms;
        report.note = self.miss_note.take();
        self.bakes += 1;
        if let Some(path) = &self.path {
            // A cache that cannot be written is only a slower next start.
            match write_atomic(path, &map.to_bytes(key)) {
                Ok(()) => report.written = true,
                Err(e) => report.note = Some(format!("cache file not written: {e}")),
            }
        }
        self.last = Some(report);
        self.slot = Some((key, Arc::clone(&map)));
        map
    }

    fn new_report(&self) -> BakeReport {
        BakeReport {
            source: Source::Memory,
            ms: 0.0,
            path: self.path.clone(),
            written: false,
            note: None,
        }
    }

    /// `Ok(None)`: no file. `Err`: a file that must not be used, and why.
    fn read_file(&self, key: u64, face_count: usize) -> Result<Option<Lightmap>, String> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return Ok(None);
        };
        if meta.len() > store::MAX_BYTES as u64 {
            return Err(why(StoreError::TooLarge));
        }
        // The key comes first: a stale file costs one small read, which is
        // what lets the editor look it up on every edit.
        let mut head = [0; store::HEADER_BYTES];
        let opened = std::fs::File::open(path).map_err(|e| format!("unreadable: {e}"))?;
        if (&opened).read_exact(&mut head).is_ok() {
            store::check_header(&head, key).map_err(why)?;
        }
        drop(opened);
        let bytes = std::fs::read(path).map_err(|e| format!("unreadable: {e}"))?;
        let map = Lightmap::from_bytes(&bytes, key).map_err(why)?;
        if map.projections().len() != face_count {
            return Err("face count differs".into());
        }
        Ok(Some(map))
    }
}
