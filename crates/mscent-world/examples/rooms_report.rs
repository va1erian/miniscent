//! What the room table costs a level (M17.2): empty leaves, rooms, the table's
//! size and the time to build it, for every level in `assets/levels` and for
//! generated levels of 3, 5 and 8 rooms a side. Run from the repository root:
//!
//!   cargo run --release -p mscent-world --example rooms_report

use std::time::Instant;

use mscent_world::{
    GenParams, LeafKind, Level, Portals, Rooms, compile, extract_faces, format, generate_level,
};

struct Row {
    name: String,
    empty: usize,
    portals: usize,
    rooms: usize,
    bytes: usize,
    split_faces: usize,
    csg_ms: f64,
    rooms_ms: f64,
}

fn measure(name: String, level: &Level) -> Row {
    let brushes = level.csg_brushes();
    let t = Instant::now();
    let bsp = compile(&brushes).expect("a tree");
    let csg_ms = t.elapsed().as_secs_f64() * 1e3;
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    let t = Instant::now();
    let rooms = Rooms::build(&bsp, &portals);
    let rooms_ms = t.elapsed().as_secs_f64() * 1e3;
    let in_rooms = |f: &mscent_world::Face| {
        let mut r: Vec<u32> = f
            .front_leaves
            .iter()
            .filter_map(|&l| rooms.room_of(l))
            .collect();
        r.sort_unstable();
        r.dedup();
        r.len()
    };
    Row {
        name,
        empty: bsp
            .leaves()
            .iter()
            .filter(|l| l.kind == LeafKind::Empty)
            .count(),
        portals: portals.portals().len(),
        rooms: rooms.count(),
        bytes: rooms.table_bytes(),
        split_faces: faces.iter().filter(|f| in_rooms(f) > 1).count(),
        csg_ms,
        rooms_ms,
    }
}

fn main() {
    let mut levels: Vec<(String, Level)> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir("assets/levels")
        .expect("run from the repository root")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "msl"))
        .collect();
    files.sort();
    for f in files {
        let level = format::read(&std::fs::read(&f).expect("a file")).expect("a level");
        levels.push((
            f.file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned(),
            level,
        ));
    }
    for n in [3, 5, 8] {
        let params = GenParams {
            cols: n,
            rows: n,
            ..GenParams::default()
        };
        levels.push((format!("generated {n}x{n}"), generate_level(7, &params)));
    }
    println!(
        "| level | empty leaves | portals | rooms | table bytes | faces in >1 room | csg ms | rooms ms |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for (name, level) in levels {
        let r = measure(name, &level);
        println!(
            "| {} | {} | {} | {} | {} | {} | {:.1} | {:.2} |",
            r.name, r.empty, r.portals, r.rooms, r.bytes, r.split_faces, r.csg_ms, r.rooms_ms
        );
    }
}
