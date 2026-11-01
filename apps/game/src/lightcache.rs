//! Where baked lightmaps are kept. A level file `x.msl` has its lightmap in
//! `x.msl.lit` next to it; a level without a file (the built-in ones, an
//! unsaved editor document) uses the per-user cache directory. The file is
//! only ever an optimisation: its content key decides whether it is valid.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use mscent_render::BakeCache;

/// The per-user directory for cache files, from the environment:
/// `MINISCENT_CACHE_DIR`, else `LOCALAPPDATA` (Windows), else
/// `XDG_CACHE_HOME`, else `~/.cache`; always ending in `miniscent`.
pub fn cache_dir() -> Option<PathBuf> {
    cache_dir_from(|k| std::env::var_os(k))
}

fn cache_dir_from(get: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |k: &str| get(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = set("MINISCENT_CACHE_DIR") {
        return Some(dir);
    }
    let base = set("LOCALAPPDATA")
        .or_else(|| set("XDG_CACHE_HOME"))
        .or_else(|| set("HOME").map(|h| h.join(".cache")))?;
    Some(base.join("miniscent"))
}

/// The cache file for a level: `<level>.msl.lit` beside `level`, or
/// `<name>.msl.lit` in the cache directory when there is no level file.
pub fn path_for(level: Option<&Path>, name: &str) -> Option<PathBuf> {
    match level {
        Some(p) => {
            let mut s = p.as_os_str().to_owned();
            s.push(".lit");
            Some(PathBuf::from(s))
        }
        None => Some(cache_dir()?.join(format!("{name}.msl.lit"))),
    }
}

/// A bake cache for `level` (or the built-in level `name`). `no_cache` keeps
/// it in memory only; `rebake` makes the first lookup bake and overwrite the
/// file.
pub fn open(level: Option<&Path>, name: &str, no_cache: bool, rebake: bool) -> BakeCache {
    let mut cache = match path_for(level, name).filter(|_| !no_cache) {
        Some(path) => BakeCache::with_file(path),
        None => BakeCache::new(),
    };
    if rebake {
        cache.force_rebake();
    }
    cache
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_sits_next_to_the_level_or_in_the_cache_dir() {
        assert_eq!(
            path_for(Some(Path::new("a/b/x.msl")), "ignored"),
            Some(PathBuf::from("a/b/x.msl.lit"))
        );
        let env = |k: &str| (k == "MINISCENT_CACHE_DIR").then(|| OsString::from("c"));
        assert_eq!(cache_dir_from(env), Some(PathBuf::from("c")));
        let env = |k: &str| (k == "HOME").then(|| OsString::from("h"));
        assert_eq!(
            cache_dir_from(env),
            Some(PathBuf::from("h").join(".cache").join("miniscent"))
        );
        assert_eq!(cache_dir_from(|_| None), None);
        assert_eq!(cache_dir_from(|_| Some(OsString::new())), None);
    }
}
