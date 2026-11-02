//! `materials.txt`: which materials exist and which id each one has.
//!
//! One line per id, in order, starting at 0: `id name gloss`, or `id retired`
//! for an id whose material was deleted. Levels store ids, so ids never move and
//! are never reused; new materials go at the end. Blank lines and lines starting
//! with `#` are skipped. Names become file names, and the file comes from disk,
//! so all of this is checked and every failure is an error.
//!
//! A directory with no manifest (a test fixture, or an `--assets` directory that
//! holds recipes or `brick.png`) gets ids from its file list, stock names first and
//! the rest sorted, and, as before there were manifests, the stock gloss for a known
//! name and [`DEFAULT_GLOSS`] for any other. So a copy of all ten stock materials
//! keeps their ids. Materials past them get theirs by name, which a new file can
//! move: a directory whose levels need them to stay needs a manifest.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use mscent_world::materials::NAMES as STOCK;

use crate::texture::TextureError;

/// The manifest's file name in a texture directory.
pub const FILE: &str = "materials.txt";
/// Gloss of what has no other: mildly glossy.
pub const DEFAULT_GLOSS: u8 = 40;
/// Most ids a manifest may hold, retired ones included.
pub const MAX_MATERIALS: usize = 1024;
/// Largest file accepted, far above any real one and read no further.
const MAX_BYTES: u64 = 64 * 1024;
const MAX_LINE: usize = 128;
const MAX_NAME: usize = 48;
/// What an id's name is once its material is gone; no material can be called it.
const RETIRED: &str = "retired";
/// Names Windows opens as devices whatever the extension.
const DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// One id of the manifest; the id is its position in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Material { name: String, gloss: u8 },
    Retired,
}

/// Why a manifest was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestError {
    TooLarge,
    NotText,
    /// It lists no ids at all.
    Empty,
    /// It lists more than [`MAX_MATERIALS`] ids.
    TooMany,
    /// A line is wrong; the number counts from 1.
    Line(usize, LineError),
}

/// What is wrong with a line of the manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineError {
    TooLong,
    /// Not `id name gloss` or `id retired`.
    BadFields,
    /// Not a plain decimal number that fits an id.
    BadId,
    /// An id that an earlier line already used: ids go up in order.
    DuplicateId,
    /// An id beyond the next one: a deleted material must stay as `retired`.
    Gap,
    /// A stock id marked retired: levels and the renderer need all of them.
    RetiredStock,
    /// Not usable as a file name; see [`name_is_valid`].
    BadName,
    DuplicateName,
    /// Not a plain decimal number from 0 to 255.
    BadGloss,
}

/// A name is 1 to 48 of `[a-z0-9-]`, starting and ending with a letter or digit,
/// and is no Windows device. With no `_`, `/`, `\`, `.` or `:` it cannot leave
/// the directory, and it cannot be read as a companion (`_n`, `_h`, `_s`) or
/// hidden (`_`) file.
pub fn name_is_valid(name: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let bytes = name.as_bytes();
    (1..=MAX_NAME).contains(&bytes.len())
        && bytes.iter().all(|&b| plain(b) || b == b'-')
        && bytes.first().copied().is_some_and(plain)
        && bytes.last().copied().is_some_and(plain)
        && name != RETIRED
        && !DEVICES.contains(&name)
}

/// A number written exactly as it prints, so `+5`, `007` and the like are out.
fn number<T: std::str::FromStr + ToString>(text: &str) -> Option<T> {
    let n: T = text.parse().ok()?;
    (n.to_string() == text).then_some(n)
}

/// The entry of one line that is not blank or a comment, which must be id `next`.
fn parse_line(text: &str, next: usize) -> Result<Entry, LineError> {
    let mut fields = text.split_ascii_whitespace();
    let (id, name, gloss, extra) = (fields.next(), fields.next(), fields.next(), fields.next());
    let (Some(id), Some(name), None) = (id, name, extra) else {
        return Err(LineError::BadFields);
    };
    let id = usize::from(number::<u16>(id).ok_or(LineError::BadId)?);
    if id < next {
        return Err(LineError::DuplicateId);
    }
    if id > next {
        return Err(LineError::Gap);
    }
    match gloss {
        None if name == RETIRED && id < STOCK.len() => Err(LineError::RetiredStock),
        None if name == RETIRED => Ok(Entry::Retired),
        None => Err(LineError::BadFields),
        Some(gloss) if name_is_valid(name) => Ok(Entry::Material {
            name: name.to_string(),
            gloss: number(gloss).ok_or(LineError::BadGloss)?,
        }),
        Some(_) => Err(LineError::BadName),
    }
}

/// The entries of a manifest's bytes, by id.
pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>, ManifestError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ManifestError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ManifestError::NotText)?;
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    for (i, line) in text.lines().enumerate() {
        let number = i + 1;
        if line.len() > MAX_LINE {
            return Err(ManifestError::Line(number, LineError::TooLong));
        }
        let line = line.trim_ascii();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if entries.len() == MAX_MATERIALS {
            return Err(ManifestError::TooMany);
        }
        let entry =
            parse_line(line, entries.len()).map_err(|why| ManifestError::Line(number, why))?;
        if let Entry::Material { name, .. } = &entry
            && !names.insert(name.clone())
        {
            return Err(ManifestError::Line(number, LineError::DuplicateName));
        }
        entries.push(entry);
    }
    if entries.is_empty() {
        return Err(ManifestError::Empty);
    }
    Ok(entries)
}

/// The text of a manifest with `name` at `gloss`: the line that names it changed,
/// or a new line at the end, whose id is the next one. Every other line, comments
/// included, stays as it is, so the file stays someone's to read. The result is
/// parsed before it is returned, so what comes back is a manifest, along with the
/// material's id.
pub fn with_material(text: &str, name: &str, gloss: u8) -> Result<(String, u16), ManifestError> {
    if !name_is_valid(name) {
        return Err(ManifestError::Line(0, LineError::BadName));
    }
    let is_row = |line: &str| {
        let line = line.trim_ascii();
        !line.is_empty() && !line.starts_with('#')
    };
    let mut out = String::with_capacity(text.len() + MAX_LINE);
    let mut rows = 0;
    let mut found = None;
    for line in text.lines() {
        if is_row(line) {
            if line.split_ascii_whitespace().nth(1) == Some(name) {
                found = Some(rows);
                out.push_str(&format!("{rows} {name} {gloss}\n"));
                rows += 1;
                continue;
            }
            rows += 1;
        }
        out.push_str(line);
        out.push('\n');
    }
    if found.is_none() {
        out.push_str(&format!("{rows} {name} {gloss}\n"));
    }
    parse(out.as_bytes())?;
    let id = u16::try_from(found.unwrap_or(rows)).map_err(|_| ManifestError::TooMany)?;
    Ok((out, id))
}

/// The text of a manifest listing `entries`, for a directory that had none.
pub fn text_of(entries: &[Entry]) -> String {
    let mut out = String::new();
    for (id, entry) in entries.iter().enumerate() {
        match entry {
            Entry::Material { name, gloss } => out.push_str(&format!("{id} {name} {gloss}\n")),
            Entry::Retired => out.push_str(&format!("{id} {RETIRED}\n")),
        }
    }
    out
}

/// The bytes of the file at `path`, never holding more of them than the size
/// limit allows. `None` when there is no such file.
fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, TextureError> {
    let io = |e: std::io::Error| TextureError::Io(format!("{}: {e}", path.display()));
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    Ok(Some(bytes))
}

/// Reads and checks the manifest at `path`. `None` when there is no such file.
pub fn read(path: &Path) -> Result<Option<Vec<Entry>>, TextureError> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(None);
    };
    parse(&bytes).map(Some).map_err(TextureError::Manifest)
}

/// The text of the manifest at `path`, checked. `None` when there is no such file.
pub fn read_text(path: &Path) -> Result<Option<String>, TextureError> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(None);
    };
    parse(&bytes).map_err(TextureError::Manifest)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| TextureError::Manifest(ManifestError::NotText))
}

/// The gloss of a name in a directory with no manifest, which must draw as it
/// did before manifests. Directories with one say it themselves.
fn listed_gloss(name: &str) -> u8 {
    match name {
        "metal" => 200,
        "lightpanel" => 120,
        "floor" => 90,
        "brick" => 25,
        "rock" => 10,
        "wax" => 90,
        "cap" => 70,
        "coral" => 30,
        "stem" => 20,
        "bark" => 8,
        _ => DEFAULT_GLOSS,
    }
}

/// The entries of a directory with no manifest: every base texture, the stock
/// names in their places and the others after them by name. PNGs starting with `_`
/// and the `_n`, `_h` and `_s` companions are not base textures, and neither is a
/// recipe whose name a manifest would refuse: it would take an id and move the
/// ones after it, for a file no manifest could list. More than [`MAX_MATERIALS`]
/// is an error, as it is in a manifest.
pub fn from_listing(dir: &Path) -> Result<Vec<Entry>, TextureError> {
    let io = |e: std::io::Error| TextureError::Io(format!("{}: {e}", dir.display()));
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let file = entry.map_err(io)?.file_name();
        let file = file.to_string_lossy();
        if let Some(stem) = file.strip_suffix(".png") {
            let companion = ["_n", "_h", "_s"].iter().any(|s| stem.ends_with(s));
            if !stem.starts_with('_') && !companion {
                names.push(stem.to_string());
            }
        } else if let Some(stem) = file.strip_suffix(crate::recipe_texture::EXTENSION)
            && name_is_valid(stem)
        {
            names.push(stem.to_string());
        }
    }
    let place = |name: &String| STOCK.iter().position(|n| n == name);
    names.sort_by_key(|name| (place(name).unwrap_or(usize::MAX), name.clone()));
    names.dedup();
    if names.len() > MAX_MATERIALS {
        return Err(TextureError::Manifest(ManifestError::TooMany));
    }
    Ok(names
        .into_iter()
        .map(|name| Entry::Material {
            gloss: listed_gloss(&name),
            name,
        })
        .collect())
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod manifest_tests;
