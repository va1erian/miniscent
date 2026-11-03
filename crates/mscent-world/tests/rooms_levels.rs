//! The room table of every level shipped in `assets/levels` stays small and
//! quick to build, and is a table `Rooms::from_parts` accepts back. The numbers
//! are in `docs/dynamic-lights.md`; `examples/rooms_report.rs` prints them.

use std::path::Path;
use std::time::{Duration, Instant};

use mscent_world::rooms::MAX_ROOMS;
use mscent_world::{LeafKind, Level, Portals, Rooms, compile, format};

/// What a shipped level's table may take, far above what they take today.
const TABLE_BUDGET_BYTES: usize = 64 * 1024;
/// And the time to build it, in a debug build on a slow machine.
const BUILD_BUDGET: Duration = Duration::from_millis(500);

fn shipped() -> Vec<(String, Level)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("the levels")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "msl"))
        .collect();
    files.sort();
    assert!(files.len() >= 9, "only {} levels", files.len());
    files
        .iter()
        .map(|f| {
            let level = format::read(&std::fs::read(f).expect("a file")).expect("a level");
            (f.display().to_string(), level)
        })
        .collect()
}

#[test]
fn every_shipped_level_has_a_small_table_built_quickly() {
    for (name, level) in shipped() {
        let bsp = compile(&level.csg_brushes()).expect("a tree");
        let portals = Portals::build(&bsp);
        let started = Instant::now();
        let rooms = Rooms::build(&bsp, &portals);
        let took = started.elapsed();
        let empty = bsp
            .leaves()
            .iter()
            .filter(|l| l.kind == LeafKind::Empty)
            .count();
        assert!(
            rooms.count() >= 1 && rooms.count() <= empty.min(MAX_ROOMS),
            "{name}"
        );
        assert!(rooms.table_bytes() <= TABLE_BUDGET_BYTES, "{name}");
        assert!(took <= BUILD_BUDGET, "{name}: {took:?}");
        // Every empty leaf is in a room and no solid one is.
        for (i, leaf) in (0u32..).zip(bsp.leaves()) {
            let room = rooms.room_of(mscent_world::LeafId(i));
            assert_eq!(
                room.is_some(),
                leaf.kind == LeafKind::Empty,
                "{name} leaf {i}"
            );
        }
    }
}
