//! The brush manipulation tools, after Hammer: slice (clip), carve, vertex
//! move and face extrusion. The geometry is in `shape_ops.rs`; this file
//! turns it into undoable edits of the editor state.

use mscent_math::Vec3d;

use super::command::Command;
use super::document::{BrushDef, Id};
use super::pick::FaceRef;
use super::pointer::{Drag, Pointer};
use super::shape_ops as ops;
use super::state::{EditorState, Selection, Tool};
use super::view::View;

/// Pixels within which a corner handle is hit.
const HIT_PIXELS: f64 = 8.0;

/// A slice line waiting for Enter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub view: View,
    pub a: [f64; 2],
    pub b: [f64; 2],
}

/// Which side of the slice line survives. "Right" is the side to the right
/// of the direction the line was drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    Both,
    Right,
    Left,
}

impl Keep {
    pub fn name(self) -> &'static str {
        match self {
            Keep::Both => "both sides",
            Keep::Right => "the right side",
            Keep::Left => "the left side",
        }
    }

    fn next(self) -> Keep {
        match self {
            Keep::Both => Keep::Right,
            Keep::Right => Keep::Left,
            Keep::Left => Keep::Both,
        }
    }
}

impl EditorState {
    /// Switches tool, dropping a half-made slice.
    pub fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            // A held result is kept, as every other action but Esc keeps it.
            self.commit_pending();
            self.cancel_drag();
        }
        self.tool = tool;
    }

    /// The slice line being dragged or waiting to be applied.
    pub fn slice_line(&self) -> Option<Cut> {
        match self.drag {
            Some(Drag::Line { view, start, cur }) => Some(Cut {
                view,
                a: start,
                b: cur,
            }),
            _ => self.cut,
        }
    }

    pub(super) fn slice_down(&mut self, p: Pointer, snapped: [f64; 2]) -> bool {
        if self.selected_brush().is_none() {
            self.selection = self.pick(p).filter(|s| matches!(s, Selection::Brush(_)));
        }
        self.cut = None;
        self.drag = Some(Drag::Line {
            view: p.view,
            start: snapped,
            cur: snapped,
        });
        true
    }

    pub(super) fn finish_line(&mut self, cut: Cut) -> bool {
        if cut.a == cut.b {
            self.message = "The slice line has no length: drag it across the brush".into();
            return false;
        }
        self.cut = Some(cut);
        self.message = format!(
            "Slice line set: Enter keeps {}, K changes the side",
            self.keep.name()
        );
        true
    }

    /// Chooses what the next slice keeps (Both, then each side in turn).
    pub fn cycle_keep(&mut self) {
        self.keep = self.keep.next();
        self.message = format!("Slice keeps {}", self.keep.name());
    }

    /// Cuts the selected brush along the pending line.
    pub fn apply_slice(&mut self) -> bool {
        let (Some(cut), Some(def)) = (self.cut, self.selected_brush().cloned()) else {
            self.message = "Draw a slice line over a selected brush first".into();
            return false;
        };
        if def.is_volume() {
            self.message = "Voxel volumes cannot be sliced".into();
            return false;
        }
        let Some(plane) = ops::line_plane(cut.view.axes(), cut.a, cut.b, 1.0e-6) else {
            self.message = "The slice line has no length: drag it across the brush".into();
            return false;
        };
        // The kept piece (or the back one when both stay) is the original;
        // an id is only taken once the cut is known to work.
        let (back_id, front_id) = match self.keep {
            Keep::Right => (0, def.id),
            _ => (def.id, 0),
        };
        let (back, front) = ops::slice(back_id, front_id, &def, plane);
        let index = self.doc.brush_index(def.id).unwrap_or(0);
        let cmd = match (self.keep, back, front) {
            (Keep::Both, Some(back), Some(mut front)) => {
                front.id = self.doc.alloc_id();
                let joins = [(def.id, front.id)];
                let mut list = vec![
                    Command::EditBrush {
                        before: def,
                        after: back,
                    },
                    Command::AddBrush {
                        index: index + 1,
                        brush: front,
                    },
                ];
                list.extend(self.doc.pieces_join_movers(&joins));
                Command::Batch(list)
            }
            // The discarded side must exist too, or the line missed the brush.
            (Keep::Left, Some(after), Some(_)) | (Keep::Right, Some(_), Some(after)) => {
                Command::EditBrush { before: def, after }
            }
            _ => {
                self.message = "The line does not cut through the brush".into();
                return false;
            }
        };
        self.history.run(&mut self.doc, cmd);
        self.cut = None;
        self.faces.clear();
        self.message = format!("Sliced brush, kept {}", self.keep.name());
        true
    }

    /// Removes the selected brush's volume from every brush it overlaps and
    /// deletes it, leaving convex pieces (Hammer's Carve).
    pub fn carve_selected(&mut self) -> bool {
        let Some(cutter) = self.selected_brush().cloned() else {
            self.message = "Select a brush to carve with".into();
            return false;
        };
        if cutter.is_volume() {
            self.message = "A voxel volume cannot carve: use the Voxel tool's Carve".into();
            return false;
        }
        let mut cmds = Vec::new();
        let mut joins = Vec::new();
        let mut carved = 0;
        // Last to first, so an edit never moves the index of an earlier one.
        for index in (0..self.doc.brushes().len()).rev() {
            let target = self.doc.brushes()[index].clone();
            if target.id == cutter.id {
                cmds.push(Command::RemoveBrush {
                    index,
                    brush: target,
                });
                continue;
            }
            let Some(pieces) = ops::subtract(&target, &cutter) else {
                continue;
            };
            carved += 1;
            cmds.extend(self.carve_commands(index, target, pieces, &mut joins));
        }
        if carved == 0 {
            self.message = "The brush overlaps nothing to carve".into();
            return false;
        }
        cmds.extend(self.doc.pieces_join_movers(&joins));
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.selection = None;
        self.faces.clear();
        self.message = format!("Carved {}", super::count(carved, "brush", "brushes"));
        true
    }

    /// Replaces `target` (at `index`) by `pieces`; the first keeps its id, and
    /// `joins` gets each other piece with the brush it came from.
    fn carve_commands(
        &mut self,
        index: usize,
        target: BrushDef,
        pieces: Vec<BrushDef>,
        joins: &mut Vec<(Id, Id)>,
    ) -> Vec<Command> {
        let source = target.id;
        let mut pieces = pieces.into_iter();
        let Some(mut first) = pieces.next() else {
            return vec![Command::RemoveBrush {
                index,
                brush: target,
            }];
        };
        first.id = target.id;
        let mut cmds = vec![Command::EditBrush {
            before: target,
            after: first,
        }];
        for (k, mut brush) in pieces.enumerate() {
            brush.id = self.doc.alloc_id();
            joins.push((source, brush.id));
            cmds.push(Command::AddBrush {
                index: index + 1 + k,
                brush,
            });
        }
        cmds
    }

    /// Pushes the picked faces out by `steps` grid cells (negative pulls
    /// them in). Pushing out adds a prism brush on each face; pulling in
    /// moves the face itself.
    pub fn extrude_faces(&mut self, steps: f64) -> bool {
        let dist = steps * self.grid;
        if self.faces.is_empty() || dist == 0.0 || !dist.is_finite() {
            self.message = "Pick faces with the Face tool to extrude".into();
            return false;
        }
        let mut by_brush: Vec<(Id, Vec<usize>)> = Vec::new();
        for FaceRef { brush, plane } in self.faces.clone() {
            match by_brush.iter_mut().find(|(id, _)| *id == brush) {
                Some((_, planes)) => planes.push(plane),
                None => by_brush.push((brush, vec![plane])),
            }
        }
        let (mut cmds, mut fresh) = (Vec::new(), Vec::new());
        let mut scratch = self.doc.clone();
        for (id, planes) in by_brush {
            let Some(def) = self.doc.brush(id).cloned() else {
                continue;
            };
            if dist > 0.0 {
                for plane in planes {
                    let Some(mut brush) = ops::prism(0, &def, plane, dist) else {
                        continue;
                    };
                    brush.id = self.doc.alloc_id();
                    let index = scratch.brush_index(id).map_or(0, |i| i + 1);
                    fresh.push((plane, def.clone(), brush.id));
                    let cmd = Command::AddBrush { index, brush };
                    cmd.apply(&mut scratch);
                    cmds.push(cmd);
                }
            } else if let Some(after) = ops::move_planes(&def, &planes, dist) {
                cmds.push(Command::EditBrush { before: def, after });
            }
        }
        if cmds.is_empty() {
            self.message = "Those faces cannot move that far".into();
            return false;
        }
        let count = self.faces.len();
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.faces = if dist > 0.0 {
            self.pushed_faces(&fresh)
        } else {
            Vec::new()
        };
        self.face_key = None;
        let way = if dist > 0.0 { "out" } else { "in" };
        let faces = super::count(count, "face", "faces");
        self.message = format!("Pushed {faces} {way} by {}", dist.abs());
        true
    }

    /// The far faces of the prisms just made, so the next push continues
    /// from them. `fresh` lists each source plane, its brush and the prism.
    fn pushed_faces(&self, fresh: &[(usize, BrushDef, Id)]) -> Vec<FaceRef> {
        fresh
            .iter()
            .filter_map(|(plane, source, prism)| {
                let n = ops::parts(source)?.planes.get(*plane)?.normal;
                let built = self.doc.brush(*prism)?.to_brush(0).ok()?;
                let plane = built.plane_facing(n)?;
                Some(FaceRef {
                    brush: *prism,
                    plane,
                })
            })
            .collect()
    }

    /// The corners of the selected brush as view positions, for drawing and
    /// hit tests; only while the vertex tool is active.
    pub fn corner_handles(&self, view: View) -> Vec<[f64; 2]> {
        if self.tool != Tool::Vertex {
            return Vec::new();
        }
        let (a, b) = view.axes();
        self.selected_brush()
            .and_then(ops::vertices)
            .map(|vs| vs.iter().map(|v| [axis(v, a), axis(v, b)]).collect())
            .unwrap_or_default()
    }

    pub(super) fn vertex_down(&mut self, p: Pointer) -> bool {
        self.faces.clear();
        self.cut = None;
        if let Some(drag) = self.grab_corner(p) {
            self.drag = Some(drag);
            return true;
        }
        self.selection = self.pick(p).filter(|s| matches!(s, Selection::Brush(_)));
        if self
            .selected_brush()
            .is_some_and(|b| ops::vertices(b).is_none())
        {
            self.message = format!(
                "This brush has more than {} corners, or is a volume: its corners cannot be moved",
                ops::MAX_VERTS
            );
        }
        true
    }

    /// Starts moving the corner under the pointer, with every corner that
    /// shares its place in this view (so an edge seen end-on moves whole).
    fn grab_corner(&self, p: Pointer) -> Option<Drag> {
        let before = self.selected_brush()?.clone();
        let points = ops::vertices(&before)?;
        let (a, b) = p.view.axes();
        let dist = |v: &Vec3d| (axis(v, a) - p.pos[0]).hypot(axis(v, b) - p.pos[1]);
        let hit = points
            .iter()
            .enumerate()
            .min_by(|x, y| dist(x.1).total_cmp(&dist(y.1)))?;
        if dist(hit.1) > HIT_PIXELS * p.px {
            return None;
        }
        let anchor = *hit.1;
        let group = points
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                (axis(v, a) - axis(&anchor, a)).abs() < 1.0e-6
                    && (axis(v, b) - axis(&anchor, b)).abs() < 1.0e-6
            })
            .map(|(i, _)| i)
            .collect();
        Some(Drag::Vertex {
            view: p.view,
            group,
            before,
            points,
        })
    }

    pub(super) fn vertex_drag(
        &mut self,
        view: View,
        (group, before, points): (&[usize], &BrushDef, &[Vec3d]),
        to: [f64; 2],
    ) {
        let (a, b) = view.axes();
        let mut moved = points.to_vec();
        for &i in group {
            let mut c = [moved[i].x, moved[i].y, moved[i].z];
            c[a] = to[0];
            c[b] = to[1];
            moved[i] = Vec3d::new(c[0], c[1], c[2]);
        }
        match ops::reshape(before, &moved) {
            Some(def) => self.doc.replace_brush(&def),
            None => {
                // Back to the start, so an impossible spot is plain to see.
                self.doc.replace_brush(before);
                self.message = "A corner cannot go there".into();
            }
        }
    }
}

fn axis(v: &Vec3d, i: usize) -> f64 {
    [v.x, v.y, v.z][i]
}
