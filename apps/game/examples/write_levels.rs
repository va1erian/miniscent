//! Writes the built-in levels to `assets/levels/*.msl`.
//!
//! `cargo run -p miniscent-game --example write_levels [out_dir]`

use std::path::PathBuf;

use miniscent_game::levels::corridor_level;
use mscent_world::{Level, format, showcase_level};

fn main() -> std::io::Result<()> {
    let dir: PathBuf = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("assets/levels"), PathBuf::from);
    std::fs::create_dir_all(&dir)?;
    let levels: [(&str, Level); 2] = [
        ("showcase.msl", showcase_level()),
        ("corridor.msl", corridor_level()),
    ];
    for (name, level) in levels {
        let bytes = format::write(&level);
        std::fs::write(dir.join(name), &bytes)?;
        println!("wrote {} ({} bytes)", dir.join(name).display(), bytes.len());
    }
    Ok(())
}
