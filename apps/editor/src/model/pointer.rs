//! Pointer tools: create, pick, move and resize, driven in world units so the
//! GUI and the tests use the same code.

use mscent_math::Vec3d;

use super::command::Command;
use super::document::{BrushDef, EntityDef, Shape};
use super::entity::{pos, with_pos};
use super::state::{EditorState, Selection, Tool};
use super::view::View;

/// A pointer position in one view.
#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub view: View,
    /// World position along the view's horizontal and vertical axes.
    pub pos: [f64; 2],
    /// World units per pixel, for hit tolerances.
    pub px: f64,
}

/// A resize handle: -1, 0 or 1 along each view axis (never both 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub dx: i8,
    pub dy: i8,
}

/// The eight handles of a rectangle, with their positions.
pub fn handles(lo: [f64; 2], hi: [f64; 2]) -> Vec<(Handle, [f64; 2])> {
    let mid = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
    let mut out = Vec::with_capacity(8);
    for dy in [-1i8, 0, 1] {
        for dx in [-1i8, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }
            let x = match dx {
                -1 => lo[0],
                0 => mid[0],
                _ => hi[0],
            };
            let y = match dy {
                -1 => lo[1],
                0 => mid[1],
                _ => hi[1],
            };
            out.push((Handle { dx, dy }, [x, y]));
        }
    }
    out
}

/// The item being dragged, as it was before the drag.
#[derive(Clone, Debug)]
pub enum Original {
    Brush(BrushDef),
    Entity(EntityDef),
}

/// An in-progress drag.
#[derive(Clone, Debug)]
pub enum Drag {
    Create {
        view: View,
        start: [f64; 2],
        cur: [f64; 2],
    },
    Move {
        view: View,
        start: [f64; 2],
        before: Original,
    },
    Resize {
        view: View,
        handle: Handle,
        before: BrushDef,
    },
    /// The slice tool's line.
    Line {
        view: View,
        start: [f64; 2],
        cur: [f64; 2],
    },
    /// The vertex tool: the corners in `group` (indices into `points`, the
    /// brush's corners before the drag) follow the pointer.
    Vertex {
        view: View,
        group: Vec<usize>,
        before: BrushDef,
        points: Vec<Vec3d>,
    },
}

/// Pixels within which an entity or handle is hit.
const HIT_PIXELS: f64 = 7.0;

impl EditorState {
    /// The rectangle of the box being created, in view axes, if any.
    pub fn create_rect(&self) -> Option<(View, [f64; 2], [f64; 2])> {
        match self.drag {
            Some(Drag::Create { view, start, cur }) => Some((
                view,
                [start[0].min(cur[0]), start[1].min(cur[1])],
                [start[0].max(cur[0]), start[1].max(cur[1])],
            )),
            _ => None,
        }
    }

    /// What lies under the pointer: entities first, then the smallest brush.
    pub fn pick(&self, p: Pointer) -> Option<Selection> {
        let (a, b) = p.view.axes();
        let tol = HIT_PIXELS * p.px;
        let near = self
            .doc
            .entities()
            .iter()
            .filter_map(|e| {
                let q = pos(&e.entity);
                let d = (q[a] - p.pos[0]).abs().max((q[b] - p.pos[1]).abs());
                (d <= tol).then_some((d, e.id))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((_, id)) = near {
            return Some(Selection::Entity(id));
        }
        let mut best: Option<(f64, usize, u32)> = None;
        for (i, br) in self.doc.brushes().iter().enumerate() {
            let (lo, hi) = br.bounds();
            let inside =
                p.pos[0] >= lo[a] && p.pos[0] <= hi[a] && p.pos[1] >= lo[b] && p.pos[1] <= hi[b];
            if !inside || !br.covers(a, b, p.pos) {
                continue;
            }
            let area = (hi[a] - lo[a]) * (hi[b] - lo[b]);
            let better = match best {
                None => true,
                Some((ba, bi, _)) => area < ba || (area == ba && i > bi),
            };
            if better {
                best = Some((area, i, br.id));
            }
        }
        best.map(|(_, _, id)| Selection::Brush(id))
    }

    fn handle_at(&self, p: Pointer) -> Option<Handle> {
        let br = self.selected_brush().filter(|b| b.is_box())?;
        let (a, b) = p.view.axes();
        let (lo, hi) = br.bounds();
        let tol = HIT_PIXELS * p.px;
        handles([lo[a], lo[b]], [hi[a], hi[b]])
            .into_iter()
            .find(|(_, q)| (q[0] - p.pos[0]).abs() <= tol && (q[1] - p.pos[1]).abs() <= tol)
            .map(|(h, _)| h)
    }

    /// Button down. Returns whether anything visible changed.
    pub fn pointer_down(&mut self, p: Pointer) -> bool {
        // Working on in a 2D view keeps what was grown, which it may be about to move.
        self.commit_pending();
        self.drag = None;
        let snapped = [self.snap(p.pos[0]), self.snap(p.pos[1])];
        match self.tool {
            Tool::Box => {
                self.drag = Some(Drag::Create {
                    view: p.view,
                    start: snapped,
                    cur: snapped,
                });
                true
            }
            Tool::Place(k) => {
                let at = self.entity_position(p.view, p.pos);
                self.place_entity(k, at);
                true
            }
            Tool::Kit(k) => {
                let at = self.entity_position(p.view, p.pos);
                self.place_kit(k, at);
                true
            }
            Tool::Slice => self.slice_down(p, snapped),
            Tool::Vertex => self.vertex_down(p),
            Tool::Select | Tool::Face | Tool::Voxel => {
                if let Some(handle) = self.handle_at(p)
                    && let Some(before) = self.selected_brush().cloned()
                {
                    self.drag = Some(Drag::Resize {
                        view: p.view,
                        handle,
                        before,
                    });
                    return true;
                }
                self.faces.clear();
                self.selection = self.pick(p);
                let before = match self.selection {
                    Some(Selection::Brush(id)) => self.doc.brush(id).cloned().map(Original::Brush),
                    Some(Selection::Entity(id)) => {
                        self.doc.entity(id).copied().map(Original::Entity)
                    }
                    None => None,
                };
                if let Some(before) = before {
                    self.drag = Some(Drag::Move {
                        view: p.view,
                        start: snapped,
                        before,
                    });
                }
                true
            }
        }
    }

    /// Pointer moved with the button held. Returns whether the view changed.
    pub fn pointer_move(&mut self, p: Pointer) -> bool {
        let snapped = [self.snap(p.pos[0]), self.snap(p.pos[1])];
        let grid = self.grid;
        match self.drag.clone() {
            None => false,
            Some(Drag::Create { view, start, .. }) => {
                self.drag = Some(Drag::Create {
                    view,
                    start,
                    cur: snapped,
                });
                true
            }
            Some(Drag::Line { view, start, .. }) => {
                self.drag = Some(Drag::Line {
                    view,
                    start,
                    cur: snapped,
                });
                true
            }
            Some(Drag::Vertex {
                view,
                group,
                before,
                points,
            }) => {
                self.vertex_drag(view, (&group, &before, &points), snapped);
                true
            }
            Some(Drag::Move {
                view,
                start,
                before,
            }) => {
                let (a, b) = view.axes();
                let mut d = [0.0; 3];
                d[a] = snapped[0] - start[0];
                d[b] = snapped[1] - start[1];
                match before {
                    Original::Brush(orig) => {
                        let mut moved = orig;
                        moved.translate(d);
                        // Volumes may touch but not overlap: it stays where it fits.
                        if !self.volume_overlaps(&moved) {
                            self.doc.replace_brush(&moved);
                        }
                    }
                    Original::Entity(orig) => {
                        let q = orig.entity.pos();
                        let np = [q.x as f64 + d[0], q.y as f64 + d[1], q.z as f64 + d[2]];
                        let moved = EntityDef {
                            id: orig.id,
                            entity: with_pos(&orig.entity, np),
                        };
                        if moved.entity.is_valid() {
                            self.doc.replace_entity(&moved);
                        }
                    }
                }
                true
            }
            Some(Drag::Resize {
                view,
                handle,
                before,
            }) => {
                let (a, b) = view.axes();
                let mut resized = before;
                if let Shape::Box { min, max } = &mut resized.shape {
                    match handle.dx {
                        -1 => min[a] = snapped[0].min(max[a] - grid),
                        1 => max[a] = snapped[0].max(min[a] + grid),
                        _ => {}
                    }
                    match handle.dy {
                        -1 => min[b] = snapped[1].min(max[b] - grid),
                        1 => max[b] = snapped[1].max(min[b] + grid),
                        _ => {}
                    }
                }
                self.doc.replace_brush(&resized);
                true
            }
        }
    }

    /// Button up: commits the drag as one undo step.
    pub fn pointer_up(&mut self, p: Pointer) -> bool {
        self.pointer_move(p);
        match self.drag.take() {
            None => false,
            Some(Drag::Create { view, start, cur }) => {
                let (a, b) = view.axes();
                let mut min = [0.0; 3];
                let mut max = [0.0; 3];
                min[view.hidden()] = self.extent.0;
                max[view.hidden()] = self.extent.1;
                min[a] = start[0].min(cur[0]);
                max[a] = start[0].max(cur[0]);
                min[b] = start[1].min(cur[1]);
                max[b] = start[1].max(cur[1]);
                self.add_box(min, max).is_some()
            }
            Some(Drag::Move { before, .. }) => match before {
                Original::Brush(orig) => match self.doc.brush(orig.id).cloned() {
                    Some(after) if after != orig => {
                        self.record_brush_edit(orig, after);
                        self.message = "Moved brush".into();
                        true
                    }
                    _ => false,
                },
                Original::Entity(orig) => match self.doc.entity(orig.id).copied() {
                    Some(after) if after != orig => {
                        self.history.record(Command::EditEntity {
                            before: orig,
                            after,
                        });
                        self.message = "Moved entity".into();
                        true
                    }
                    _ => false,
                },
            },
            Some(Drag::Resize { before, .. }) => match self.doc.brush(before.id).cloned() {
                Some(after) if after != before => {
                    self.record_brush_edit(before, after);
                    self.message = "Resized brush".into();
                    true
                }
                _ => false,
            },
            Some(Drag::Line { view, start, cur }) => self.finish_line(super::tools::Cut {
                view,
                a: start,
                b: cur,
            }),
            Some(Drag::Vertex { before, .. }) => match self.doc.brush(before.id).cloned() {
                Some(after) if after != before => {
                    self.record_brush_edit(before, after);
                    self.message = "Moved corner".into();
                    true
                }
                _ => false,
            },
        }
    }

    fn volume_overlaps(&self, def: &BrushDef) -> bool {
        def.volume().is_some_and(|v| {
            (self.doc.volumes()).any(|(other, o)| other.id != def.id && o.overlaps(v))
        })
    }

    /// Abandons a drag, restoring the item as it was.
    pub fn cancel_drag(&mut self) {
        self.cancel_stroke();
        match self.drag.take() {
            Some(Drag::Move { before, .. }) => match before {
                Original::Brush(b) => self.doc.replace_brush(&b),
                Original::Entity(e) => self.doc.replace_entity(&e),
            },
            Some(Drag::Resize { before, .. }) | Some(Drag::Vertex { before, .. }) => {
                self.doc.replace_brush(&before)
            }
            _ => {}
        }
        self.cut = None;
    }
}
