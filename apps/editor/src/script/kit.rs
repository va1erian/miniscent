//! Basic entities: a door, a lift, a wave. Each is ordinary level content (brushes,
//! zones and markers with names) plus a few lines of script that wire them up.
//! The level format gains nothing, and the logic stays where the author can read
//! and change it. Most of the functions called are in `assets/scripts/lib/kit.st`.

use mscent_math::Vec3;
use mscent_world::{Entity, Op};

use crate::model::command::Command;
use crate::model::document::{BrushDef, EntityDef, GroupDef, LabelDef, Shape};
use crate::model::state::LIMIT;
use crate::model::{EditorState, Selection, Tool};

/// A kind of basic entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitKind {
    Door,
    LockedDoor,
    SwitchDoor,
    Lift,
    Trigger,
    Wave,
    Terminal,
    HealthPack,
    Exit,
}

/// A basic entity on offer.
#[derive(Clone, Copy, Debug)]
pub struct KitEntry {
    pub kind: KitKind,
    pub label: &'static str,
    /// New names are this and a number.
    stem: &'static str,
    /// What the status bar says when it is picked.
    pub help: &'static str,
}

const fn k(kind: KitKind, label: &'static str, stem: &'static str, help: &'static str) -> KitEntry {
    KitEntry {
        kind,
        label,
        stem,
        help,
    }
}

pub const KIT: [KitEntry; 9] = [
    k(
        KitKind::Door,
        "Door",
        "door",
        "opens as the player comes near",
    ),
    k(
        KitKind::LockedDoor,
        "Locked door",
        "locked",
        "with a keycard to find",
    ),
    k(
        KitKind::SwitchDoor,
        "Switch door",
        "vault",
        "opens once its switches are shot",
    ),
    k(KitKind::Lift, "Lift", "lift", "rides up and down by itself"),
    k(
        KitKind::Trigger,
        "Trigger",
        "trigger",
        "does something when walked into",
    ),
    k(
        KitKind::Wave,
        "Enemy wave",
        "wave",
        "enemies appear when walked into",
    ),
    k(
        KitKind::Terminal,
        "Terminal",
        "console",
        "does something when the use key is held",
    ),
    k(
        KitKind::HealthPack,
        "Health pack",
        "health",
        "repairs the player when touched",
    ),
    k(KitKind::Exit, "Exit", "exit", "wins the level when reached"),
];

impl KitKind {
    pub fn entry(self) -> &'static KitEntry {
        let found = KIT.iter().find(|e| e.kind == self);
        found.unwrap_or(&KIT[0])
    }
}

/// A box in world units.
type Bounds = ([f64; 3], [f64; 3]);

/// What placing an entity adds. A plan touches nothing; [`EditorState::place_kit`]
/// applies it as one undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The moving brush and its name.
    pub mover: Option<(Bounds, String)>,
    pub things: Vec<(Entity, String)>,
    pub script: String,
}

/// Size of a door or platform with no room to fit it to.
const LONE_DOOR: f64 = 4.0;

/// The smallest carved box around `p`, seen from above.
fn room_around(st: &EditorState, p: [f64; 3]) -> Option<Bounds> {
    let carved = st.doc.brushes().iter().filter(|b| b.op == Op::Subtract);
    let around = carved.map(|b| b.bounds()).filter(|(lo, hi)| {
        (0..2).all(|i| p[i] >= lo[i] && p[i] <= hi[i]) && (0..3).all(|i| lo[i] < hi[i])
    });
    // A room at the click's height first: rooms can be stacked, and the height
    // of a click in the top view is only the depth set for new things.
    let elsewhere = |r: &Bounds| p[2] < r.0[2] || p[2] > r.1[2];
    around.min_by(|a, b| {
        let size = |r: &Bounds| (0..3).map(|i| r.1[i] - r.0[i]).product::<f64>();
        elsewhere(a)
            .cmp(&elsewhere(b))
            .then(size(a).total_cmp(&size(b)))
    })
}

fn at(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

fn marker(p: [f64; 3], name: String) -> (Entity, String) {
    (
        Entity::Marker {
            pos: at(p),
            yaw: 0.0,
        },
        name,
    )
}

fn zone(p: [f64; 3], half: [f64; 3], name: String) -> (Entity, String) {
    (
        Entity::Zone {
            pos: at(p),
            half: at(half),
        },
        name,
    )
}

/// A door across the room: across its longer way, so it closes the room off, and
/// floor to ceiling. Its area reaches well to either side.
struct DoorFit {
    bounds: Bounds,
    area: Entity,
    /// How far to rise to clear the opening.
    travel: f64,
    /// The way across the door, X or Y, to set things to one side of it.
    across: usize,
    center: [f64; 3],
}

fn fit_door(p: [f64; 3], room: Option<Bounds>) -> DoorFit {
    let (lo, hi) = room.unwrap_or_else(|| {
        (
            [p[0] - LONE_DOOR, p[1] - LONE_DOOR, p[2] - LONE_DOOR],
            [p[0] + LONE_DOOR, p[1] + LONE_DOOR, p[2] + LONE_DOOR],
        )
    });
    let across = usize::from(hi[1] - lo[1] > hi[0] - lo[0]);
    let along = 1 - across;
    let mut bounds = (lo, hi);
    bounds.0[across] = p[across] - 0.5;
    bounds.1[across] = p[across] + 0.5;
    let height = hi[2] - lo[2];
    let mut center = [0.0; 3];
    center[across] = p[across];
    center[along] = (lo[along] + hi[along]) / 2.0;
    center[2] = (lo[2] + hi[2]) / 2.0;
    let mut half = [0.0; 3];
    half[across] = 4.0;
    half[along] = ((hi[along] - lo[along]) / 2.0 - 0.5).max(1.0);
    half[2] = (height / 2.0 - 0.5).max(1.0);
    let area = zone(center, half, String::new()).0;
    DoorFit {
        bounds,
        area,
        travel: height + 1.0,
        across,
        center,
    }
}

/// A point `by` to one side of the door and `side` along it.
fn beside(fit: &DoorFit, by: f64, side: f64) -> [f64; 3] {
    let mut out = fit.center;
    out[fit.across] += by;
    out[1 - fit.across] += side;
    out
}

/// A door and its area; the caller writes the script.
fn door_plan(fit: &DoorFit, name: &str) -> Plan {
    Plan {
        mover: Some((fit.bounds, name.to_string())),
        things: vec![(fit.area, format!("{name}_area"))],
        script: String::new(),
    }
}

fn lift_plan(p: [f64; 3], room: Option<Bounds>, name: &str) -> Plan {
    let floor = room.map_or(p[2] - 0.5, |(lo, _)| lo[2]);
    let rise = room.map_or(6.0, |(lo, hi)| (hi[2] - lo[2] - 5.0).max(1.0));
    let bounds = (
        [p[0] - 3.0, p[1] - 3.0, floor],
        [p[0] + 3.0, p[1] + 3.0, floor + 1.0],
    );
    Plan {
        mover: Some((bounds, name.to_string())),
        things: Vec::new(),
        script: format!(
            "\"Lift: rides up {rise} and back, every 5 seconds.\"\nlift: '{name}' rise: {rise} every: 5.\n"
        ),
    }
}

/// `p` moved to stay in the room, clear of its walls, where something can stand
/// on it and be reached. An axis the room has no space on stays as it is.
fn inside(p: [f64; 3], room: Option<Bounds>) -> [f64; 3] {
    let mut out = p;
    if let Some((lo, hi)) = room {
        for i in 0..3 {
            if hi[i] - lo[i] > 2.0 {
                out[i] = p[i].clamp(lo[i] + 1.0, hi[i] - 1.0);
            }
        }
    }
    out
}

/// The plan for `kind` at `p`, under `name`.
fn plan(kind: KitKind, p: [f64; 3], room: Option<Bounds>, name: &str) -> Plan {
    let spot = inside(p, room);
    let single = |thing: (Entity, String), script: String| Plan {
        mover: None,
        things: vec![thing],
        script,
    };
    match kind {
        KitKind::Door => {
            let fit = fit_door(p, room);
            let Plan { mover, things, .. } = door_plan(&fit, name);
            Plan {
                mover,
                things,
                script: format!(
                    "\"Door: opens as the player comes near. {name}_area is where.\"\nautoDoor: '{name}' travel: (x: 0 y: 0 z: {}) key: nil.\n",
                    fit.travel
                ),
            }
        }
        KitKind::LockedDoor => {
            let fit = fit_door(p, room);
            let Plan {
                mover, mut things, ..
            } = door_plan(&fit, name);
            let key = format!("{name}_key");
            things.push(marker(inside(beside(&fit, -3.0, 0.0), room), key.clone()));
            Plan {
                mover,
                things,
                script: format!(
                    "\"Locked door: the keycard on {key} opens it. Move the marker to hide it.\"\nkeycardAt: '{key}' gives: '{key}'.\nautoDoor: '{name}' travel: (x: 0 y: 0 z: {}) key: '{key}'.\n",
                    fit.travel
                ),
            }
        }
        KitKind::SwitchDoor => {
            let fit = fit_door(p, room);
            let Plan {
                mover, mut things, ..
            } = door_plan(&fit, name);
            // A switch door has no area: nothing happens on walking up to it.
            things.clear();
            for (i, side) in [-3.0, 3.0].into_iter().enumerate() {
                let spot = inside(beside(&fit, -3.0, side), room);
                things.push(marker(spot, format!("{name}_switch_{}", i + 1)));
            }
            Plan {
                mover,
                things,
                script: format!(
                    "\"Switch door: shoot every {name}_switch marker's switch to open it.\"\nswitchDoor: '{name}' travel: (x: 0 y: 0 z: {}).\n",
                    fit.travel
                ),
            }
        }
        KitKind::Lift => lift_plan(p, room, name),
        KitKind::Trigger => single(
            zone(spot, [3.0, 3.0, 3.0], name.to_string()),
            format!(
                "\"Trigger: runs once, when the player walks in.\"\n(zone: '{name}') onceEnter: [\n    say: 'Something stirs'.\n].\n"
            ),
        ),
        KitKind::Wave => Plan {
            mover: None,
            things: vec![
                zone(spot, [6.0, 6.0, 3.0], name.to_string()),
                marker(
                    inside([spot[0] - 3.0, spot[1], spot[2]], room),
                    format!("{name}_spawn_1"),
                ),
                marker(
                    inside([spot[0] + 3.0, spot[1], spot[2]], room),
                    format!("{name}_spawn_2"),
                ),
            ],
            script: format!(
                "\"Enemy wave: on the {name}_spawn markers, when the player walks in. Add markers for more.\"\nwave: '{name}' kind: 'drone'.\n"
            ),
        },
        KitKind::Terminal => single(
            marker(spot, name.to_string()),
            format!(
                "\"Terminal: hold the use key (G) for 2 seconds.\"\nterminal: '{name}' hold: 2 do: [\n    say: 'Access granted'.\n].\n"
            ),
        ),
        KitKind::HealthPack => single(
            marker(spot, name.to_string()),
            format!("\"Health pack: gone once picked up.\"\nhealthPack: '{name}' heals: 50.\n"),
        ),
        KitKind::Exit => single(
            zone(spot, [3.0, 3.0, 3.0], name.to_string()),
            format!("\"Exit: reaching this zone wins the level.\"\nexitZone: '{name}'.\n"),
        ),
    }
}

fn planned_names(plan: &Plan) -> Vec<&str> {
    let mover = plan.mover.iter().map(|(_, n)| n.as_str());
    mover
        .chain(plan.things.iter().map(|(_, n)| n.as_str()))
        .collect()
}

fn in_range(plan: &Plan) -> bool {
    let brush_ok = plan.mover.iter().all(|((lo, hi), _)| {
        lo.iter()
            .chain(hi)
            .all(|v| v.is_finite() && v.abs() <= LIMIT)
    });
    brush_ok && plan.things.iter().all(|(e, _)| e.is_valid())
}

impl EditorState {
    /// Picks a basic entity: the next click in a 2D view places it.
    pub fn arm_kit(&mut self, kind: KitKind) {
        self.set_tool(Tool::Kit(kind));
        let entry = kind.entry();
        self.message = format!(
            "{}: {}. Click in a 2D view to place it",
            entry.label, entry.help
        );
    }

    /// The first numbered name whose whole plan is free.
    fn free_plan(&self, kind: KitKind, p: [f64; 3], room: Option<Bounds>) -> Option<Plan> {
        (1..=9999).find_map(|n| {
            let plan = plan(kind, p, room, &format!("{}_{n}", kind.entry().stem));
            let free = planned_names(&plan)
                .iter()
                .all(|name| !self.doc.name_taken(name));
            free.then_some(plan)
        })
    }

    /// Places a basic entity at `p`, as one undo step, and adds its lines to the
    /// script, which undo there is the script's own. Whether it was placed.
    pub fn place_kit(&mut self, kind: KitKind, p: [f64; 3]) -> bool {
        self.tool = Tool::Select;
        let room = room_around(self, p);
        let Some(plan) = self.free_plan(kind, p, room).filter(in_range) else {
            self.message = "Cannot place that here: it would leave the map".into();
            return false;
        };
        let mut cmds = Vec::new();
        let mut first = None;
        if let Some((bounds, name)) = &plan.mover {
            let id = self.add_kit_brush(*bounds, name, &mut cmds);
            first = Some(Selection::Brush(id));
        }
        for (entity, name) in &plan.things {
            let id = self.add_kit_thing(*entity, name, &mut cmds);
            first = first.or(Some(Selection::Entity(id)));
        }
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.selection = first;
        self.draft.filled_for = None;
        let line = self.script.append_block(&plan.script);
        self.show_script_line = Some(line);
        self.message = format!(
            "Placed {}. Its script starts on line {}",
            planned_names(&plan).first().copied().unwrap_or(""),
            line + 1
        );
        true
    }

    fn add_kit_brush(&mut self, (min, max): Bounds, name: &str, cmds: &mut Vec<Command>) -> u32 {
        let id = self.doc.alloc_id();
        let brush = BrushDef {
            id,
            shape: Shape::Box { min, max },
            op: Op::Add,
            material: self.new_material,
            texinfos: Vec::new(),
        };
        let index = self.doc.brushes().len();
        cmds.push(Command::AddBrush { index, brush });
        let before = self.doc.groups().to_vec();
        let mut after = before.clone();
        after.push(GroupDef {
            name: name.to_string(),
            tags: Vec::new(),
            brushes: vec![id],
        });
        cmds.push(Command::SetGroups { before, after });
        id
    }

    fn add_kit_thing(&mut self, entity: Entity, name: &str, cmds: &mut Vec<Command>) -> u32 {
        let id = self.doc.alloc_id();
        let added = cmds
            .iter()
            .filter(|c| matches!(c, Command::AddEntity { .. }))
            .count();
        let index = self.doc.entities().len() + added;
        cmds.push(Command::AddEntity {
            index,
            entity: EntityDef { id, entity },
        });
        cmds.push(Command::SetLabel {
            entity: id,
            before: None,
            after: Some(LabelDef {
                name: Some(name.to_string()),
                tags: Vec::new(),
            }),
        });
        id
    }
}

#[cfg(test)]
#[path = "kit_tests.rs"]
mod tests;
