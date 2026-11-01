//! Undoable edits. Every command stores enough to be inverted exactly.

use super::document::{BrushDef, Document, EntityDef, Id};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    AddBrush {
        index: usize,
        brush: BrushDef,
    },
    RemoveBrush {
        index: usize,
        brush: BrushDef,
    },
    EditBrush {
        before: BrushDef,
        after: BrushDef,
    },
    /// Moves a brush in the CSG order.
    Reorder {
        id: Id,
        from: usize,
        to: usize,
    },
    AddEntity {
        index: usize,
        entity: EntityDef,
    },
    RemoveEntity {
        index: usize,
        entity: EntityDef,
    },
    EditEntity {
        before: EntityDef,
        after: EntityDef,
    },
    Batch(Vec<Command>),
}

impl Command {
    pub fn apply(&self, doc: &mut Document) {
        match self {
            Command::AddBrush { index, brush } => doc.insert_brush(*index, brush.clone()),
            Command::RemoveBrush { brush, .. } => {
                doc.remove_brush(brush.id);
            }
            Command::EditBrush { after, .. } => doc.replace_brush(after),
            Command::Reorder { id, to, .. } => doc.move_brush(*id, *to),
            Command::AddEntity { index, entity } => doc.insert_entity(*index, *entity),
            Command::RemoveEntity { entity, .. } => {
                doc.remove_entity(entity.id);
            }
            Command::EditEntity { after, .. } => doc.replace_entity(after),
            Command::Batch(list) => list.iter().for_each(|c| c.apply(doc)),
        }
    }

    /// Whether `newer` can be folded into this command: both edit the same
    /// brushes (one `EditBrush` each, or batches of them in the same order).
    pub fn merges_with(&self, newer: &Command) -> bool {
        match (self, newer) {
            (Command::EditBrush { before: a, .. }, Command::EditBrush { before: b, .. }) => {
                a.id == b.id
            }
            (Command::Batch(a), Command::Batch(b)) => {
                !a.is_empty()
                    && a.len() == b.len()
                    && a.iter().zip(b).all(|(x, y)| x.merges_with(y))
            }
            _ => false,
        }
    }

    /// Folds `newer` in after this command (see [`Command::merges_with`]):
    /// the result goes from this command's start to `newer`'s end.
    pub fn merge(&mut self, newer: &Command) {
        match (self, newer) {
            (Command::EditBrush { after, .. }, Command::EditBrush { after: n, .. }) => {
                *after = n.clone();
            }
            (Command::Batch(a), Command::Batch(b)) => {
                for (x, y) in a.iter_mut().zip(b) {
                    x.merge(y);
                }
            }
            _ => {}
        }
    }

    /// The command that undoes this one.
    pub fn inverse(&self) -> Command {
        match self {
            Command::AddBrush { index, brush } => Command::RemoveBrush {
                index: *index,
                brush: brush.clone(),
            },
            Command::RemoveBrush { index, brush } => Command::AddBrush {
                index: *index,
                brush: brush.clone(),
            },
            Command::EditBrush { before, after } => Command::EditBrush {
                before: after.clone(),
                after: before.clone(),
            },
            Command::Reorder { id, from, to } => Command::Reorder {
                id: *id,
                from: *to,
                to: *from,
            },
            Command::AddEntity { index, entity } => Command::RemoveEntity {
                index: *index,
                entity: *entity,
            },
            Command::RemoveEntity { index, entity } => Command::AddEntity {
                index: *index,
                entity: *entity,
            },
            Command::EditEntity { before, after } => Command::EditEntity {
                before: *after,
                after: *before,
            },
            Command::Batch(list) => {
                Command::Batch(list.iter().rev().map(Command::inverse).collect())
            }
        }
    }
}

/// The undo and redo stacks.
#[derive(Debug, Default)]
pub struct History {
    done: Vec<Command>,
    undone: Vec<Command>,
}

impl History {
    /// Applies `cmd` and records it, dropping the redo stack.
    pub fn run(&mut self, doc: &mut Document, cmd: Command) {
        cmd.apply(doc);
        self.record(cmd);
    }

    /// Applies `cmd`. When `merge` is set and the last recorded command can
    /// absorb it (see [`Command::merges_with`]) the two become one undo step,
    /// so a run of edits to one field undoes in one go.
    pub fn run_merging(&mut self, doc: &mut Document, cmd: Command, merge: bool) {
        cmd.apply(doc);
        if merge
            && self.undone.is_empty()
            && let Some(last) = self.done.last_mut()
            && last.merges_with(&cmd)
        {
            last.merge(&cmd);
            return;
        }
        self.record(cmd);
    }

    /// Number of undo steps recorded.
    pub fn depth(&self) -> usize {
        self.done.len()
    }

    /// Records a command whose effect is already in the document (a finished drag).
    pub fn record(&mut self, cmd: Command) {
        self.done.push(cmd);
        self.undone.clear();
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(cmd) = self.done.pop() else {
            return false;
        };
        cmd.inverse().apply(doc);
        self.undone.push(cmd);
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(cmd) = self.undone.pop() else {
            return false;
        };
        cmd.apply(doc);
        self.done.push(cmd);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }
}
