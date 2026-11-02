//! A mover and the zone named after it, `door_1_area`, are one thing to the person
//! placing a door: the door opens when the player is in that zone. The level
//! holds no link between them, by design, only the name. So here the zone follows
//! its mover when that is edited, and the check says when they have come apart.

use mscent_world::{Entity, Level};

use super::check::Problem;
use crate::model::EditorState;
use crate::model::command::Command;
use crate::model::document::{BrushDef, EntityDef, Id};
use crate::model::entity::{pos, with_pos};

/// How far a zone may be from its mover, in world units, and still reach it.
const REACH: f64 = 1.0;

/// The name of the zone that goes with a mover.
fn area_name(mover: &str) -> String {
    format!("{mover}_area")
}

type Bounds = ([f64; 3], [f64; 3]);

fn grow(into: &mut Option<Bounds>, (lo, hi): Bounds) {
    let (a, b) = into.get_or_insert((lo, hi));
    for i in 0..3 {
        a[i] = a[i].min(lo[i]);
        b[i] = b[i].max(hi[i]);
    }
}

fn center((lo, hi): Bounds) -> [f64; 3] {
    [0, 1, 2].map(|i| (lo[i] + hi[i]) / 2.0)
}

impl EditorState {
    /// The bounds of a group's brushes, with `changed` standing in for its own.
    fn group_bounds(&self, brushes: &[Id], changed: &BrushDef) -> Option<Bounds> {
        let mut bounds = None;
        for id in brushes {
            let brush = if *id == changed.id {
                Some(changed)
            } else {
                self.doc.brush(*id)
            };
            if let Some(brush) = brush {
                grow(&mut bounds, brush.bounds());
            }
        }
        bounds
    }

    /// The zone called `name`, if there is one.
    fn zone_named(&self, name: &str) -> Option<EntityDef> {
        let zones = self.doc.entities().iter();
        let mut zones = zones.filter(|e| matches!(e.entity, Entity::Zone { .. }));
        zones
            .find(|e| {
                let label = self.doc.label(e.id);
                label.and_then(|l| l.name.as_deref()) == Some(name)
            })
            .copied()
    }

    /// What moves the zone of the mover `after` belongs to, by as much as the
    /// mover's middle moved. `None` if it has no such zone or did not move.
    fn area_follows(&self, before: &BrushDef, after: &BrushDef) -> Option<Command> {
        let group = self.doc.group_of(before.id)?;
        let zone = self.zone_named(&area_name(&group.name))?;
        let was = center(self.group_bounds(&group.brushes, before)?);
        let now = center(self.group_bounds(&group.brushes, after)?);
        let at = pos(&zone.entity);
        let moved = [0, 1, 2].map(|i| at[i] + now[i] - was[i]);
        let entity = with_pos(&zone.entity, moved);
        (moved != at && entity.is_valid()).then_some(Command::EditEntity {
            before: zone,
            after: EntityDef { entity, ..zone },
        })
    }

    /// Runs a brush edit, and takes the mover's zone along in the same undo step.
    pub fn run_brush_edit(&mut self, before: BrushDef, after: BrushDef) {
        let follows = self.area_follows(&before, &after);
        let edit = Command::EditBrush { before, after };
        let cmd = match follows {
            Some(zone) => Command::Batch(vec![edit, zone]),
            None => edit,
        };
        self.history.run(&mut self.doc, cmd);
    }

    /// As [`EditorState::run_brush_edit`], for a drag that has already changed the
    /// brush in the document.
    pub fn record_brush_edit(&mut self, before: BrushDef, after: BrushDef) {
        match self.area_follows(&before, &after) {
            Some(zone) => {
                zone.apply(&mut self.doc);
                let edit = Command::EditBrush { before, after };
                self.history.record(Command::Batch(vec![edit, zone]));
            }
            None => self.history.record(Command::EditBrush { before, after }),
        }
    }
}

/// The bounds of a mover's brushes as saved.
fn mover_bounds(level: &Level, brushes: &[u32]) -> Option<Bounds> {
    let mut bounds = None;
    for index in brushes {
        if let Some(brush) = level.brushes.get(*index as usize) {
            let b = brush.bounds();
            let lo = [b.min.x, b.min.y, b.min.z].map(f64::from);
            let hi = [b.max.x, b.max.y, b.max.z].map(f64::from);
            grow(&mut bounds, (lo, hi));
        }
    }
    bounds
}

/// The bounds of the zone called `name`.
fn zone_bounds(level: &Level, name: &str) -> Option<Bounds> {
    let label = level
        .labels
        .iter()
        .find(|l| l.name.as_deref() == Some(name))?;
    match level.entities.get(label.entity as usize)? {
        Entity::Zone { pos, half } => {
            let (p, h) = (
                [pos.x, pos.y, pos.z].map(f64::from),
                [half.x, half.y, half.z].map(f64::from),
            );
            Some((
                [0, 1, 2].map(|i| p[i] - h[i]),
                [0, 1, 2].map(|i| p[i] + h[i]),
            ))
        }
        _ => None,
    }
}

/// Movers the script names whose zone does not reach them: walking up to the door
/// does nothing, and nothing else would say why.
pub fn stranded_areas(level: &Level, script: &str) -> Vec<Problem> {
    let mut found = Vec::new();
    for group in &level.mover_groups {
        let zone = area_name(&group.name);
        if !script.contains(&format!("'{}'", group.name)) {
            continue;
        }
        let (Some(mover), Some(area)) = (
            mover_bounds(level, &group.brushes),
            zone_bounds(level, &zone),
        ) else {
            continue;
        };
        let reaches =
            (0..3).all(|i| area.0[i] <= mover.1[i] + REACH && area.1[i] >= mover.0[i] - REACH);
        if !reaches {
            found.push(Problem {
                line: None,
                text: format!(
                    "The zone '{zone}' does not reach the mover '{}', so walking up to it does nothing. Move the zone onto it",
                    group.name
                ),
                warning: true,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, EntityKind, Pointer, Selection, View};
    use crate::script::kit::KitKind;
    use miniscent_game::agent::{TestOutput, run_test};
    use mscent_world::Op;

    /// One room, 32 by 12 by 8, the player at one end, a door in the middle.
    fn room_with_door() -> EditorState {
        let mut st = EditorState::new(Document::new());
        st.new_op = Op::Subtract;
        st.add_box([-16.0, -6.0, -4.0], [16.0, 6.0, 4.0]).unwrap();
        st.place_entity(EntityKind::PlayerStart, [-14.0, 0.0, 0.0]);
        assert!(st.place_kit(KitKind::Door, [0.0, 0.0, 1.0]));
        st.history.clear();
        st
    }

    fn zone_x(st: &EditorState) -> f64 {
        pos(&st.zone_named("door_1_area").expect("the zone").entity)[0]
    }

    fn door(st: &EditorState) -> BrushDef {
        let mut brushes = st.doc.brushes().iter();
        brushes
            .rfind(|b| b.op == Op::Add)
            .cloned()
            .expect("the door")
    }

    #[test]
    fn typing_new_bounds_or_moving_a_door_takes_its_zone_along_in_one_undo() {
        let mut st = room_with_door();
        st.selection = Some(Selection::Brush(door(&st).id));
        assert!(st.edit_selected_brush(|b| b.translate([8.0, 0.0, 0.0])));
        assert_eq!(zone_x(&st), 8.0);
        assert_eq!(st.history.depth(), 1);
        assert!(st.undo());
        assert_eq!(zone_x(&st), 0.0);
        assert_eq!(door(&st).bounds().0[0], -0.5);
        assert!(st.redo());
        assert_eq!(zone_x(&st), 8.0);
    }

    #[test]
    fn dragging_a_door_takes_its_zone_along_in_one_undo() {
        let mut st = room_with_door();
        st.selection = Some(Selection::Brush(door(&st).id));
        let at = |x: f64| Pointer {
            view: View::ALL[0],
            pos: [x, 3.0],
            px: 0.01,
        };
        st.pointer_down(at(0.0));
        st.pointer_move(at(6.0));
        st.pointer_up(at(6.0));
        assert_eq!(door(&st).bounds().0[0], 5.5, "the door moved");
        assert_eq!(zone_x(&st), 6.0);
        assert_eq!(st.history.depth(), 1);
        assert!(st.undo());
        assert_eq!((door(&st).bounds().0[0], zone_x(&st)), (-0.5, 0.0));
    }

    #[test]
    fn resizing_a_door_keeps_its_zone_on_its_middle() {
        let mut st = room_with_door();
        st.selection = Some(Selection::Brush(door(&st).id));
        assert!(st.edit_selected_brush(|b| {
            if let crate::model::Shape::Box { max, .. } = &mut b.shape {
                max[0] = 2.5;
            }
        }));
        assert_eq!(zone_x(&st), 1.0);
    }

    #[test]
    fn other_brushes_and_a_door_that_stays_put_move_no_zone() {
        let mut st = room_with_door();
        let room = st.doc.brushes()[0].id;
        st.selection = Some(Selection::Brush(room));
        assert!(st.edit_selected_brush(|b| b.translate([1.0, 0.0, 0.0])));
        assert_eq!(zone_x(&st), 0.0);
        st.selection = Some(Selection::Brush(door(&st).id));
        assert!(st.edit_selected_brush(|b| b.set_material(1)));
        assert_eq!(zone_x(&st), 0.0);
        assert!(matches!(st.history.depth(), 2));
    }

    #[test]
    fn a_door_with_its_zone_deleted_still_moves() {
        let mut st = room_with_door();
        let zone = st.zone_named("door_1_area").unwrap().id;
        st.selection = Some(Selection::Entity(zone));
        assert!(st.delete_selected());
        st.selection = Some(Selection::Brush(door(&st).id));
        assert!(st.edit_selected_brush(|b| b.translate([1.0, 0.0, 0.0])));
    }

    /// In the real game: a moved door opens for a player beside it where it is now.
    #[test]
    fn a_moved_door_opens_for_a_player_who_walks_up_to_it() {
        let mut st = room_with_door();
        st.selection = Some(Selection::Brush(door(&st).id));
        assert!(st.edit_selected_brush(|b| b.translate([10.0, 0.0, 0.0])));
        let dir = std::env::temp_dir().join(format!("mscent-area-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let level = dir.join("moved.msl");
        crate::files::save(&st.doc, &level).unwrap();
        std::fs::write(level.with_extension("st"), st.script.text()).unwrap();
        let test = dir.join("t.st");
        std::fs::write(
            &test,
            "door := mover: 'door_1'.\n\
             assert: door poseNow = 'closed' because: 'shut while the player is far off'.\n\
             flyTo: (x: 7 y: 0 z: 0).\n\
             wait: 1.5.\n\
             assert: door poseNow = 'open' because: 'open beside where it is now'.\n",
        )
        .unwrap();
        let out = TestOutput {
            trace: None,
            shots_dir: dir.clone(),
            assets: None,
        };
        let summary = run_test(&level, &test, &out).expect("passes");
        assert!(summary.contains(" 0 failed"), "{summary}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_zone_left_behind_is_reported_and_one_in_reach_is_not() {
        let mut st = room_with_door();
        let text = st.script.text().to_string();
        assert_eq!(stranded_areas(&st.doc.to_level().unwrap(), &text), []);
        // As an older level has it: the door moved and the zone did not.
        let mut moved = door(&st);
        moved.translate([12.0, 0.0, 0.0]);
        st.doc.replace_brush_for_test(&moved);
        let level = st.doc.to_level().unwrap();
        let found = stranded_areas(&level, &text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].warning && found[0].text.contains("door_1_area"));
        assert_eq!(
            stranded_areas(&level, "win."),
            [],
            "the script does not use it"
        );
    }
}
