//! Names, tags and mover groups in the editor, and where the level's script
//! uses them. Every change is checked against the rules a saved level must meet
//! before it is applied, so the document stays one that can be saved.

use super::command::Command;
use super::document::{Document, GroupDef, Id, LabelDef};
use super::state::{EditorState, Selection};
use mscent_world::Op;

/// The panel's name and tag fields, while being typed in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelDraft {
    pub name: String,
    pub tags: String,
    /// What the fields were filled from, so a new selection fills them again.
    pub filled_for: Option<Selection>,
}

fn split_tags(tags: &str) -> Vec<String> {
    let parts = tags.split([',', ' ']).filter(|t| !t.is_empty());
    parts.map(str::to_string).collect()
}

fn join_tags(tags: &[String]) -> String {
    tags.join(", ")
}

/// A member's fields show the group's tags, so they replace them. A brush that
/// is only joining says nothing about the tags already there, so it adds.
fn set_group_tags(group: &mut GroupDef, tags: Vec<String>, member: bool) {
    if member {
        group.tags = tags;
        return;
    }
    for tag in tags {
        if !group.tags.contains(&tag) {
            group.tags.push(tag);
        }
    }
}

/// A label with neither name nor tag is no label.
fn label_of(name: &str, tags: &str) -> Option<LabelDef> {
    let name = name.trim();
    let tags = split_tags(tags);
    if name.is_empty() && tags.is_empty() {
        return None;
    }
    Some(LabelDef {
        name: (!name.is_empty()).then(|| name.to_string()),
        tags,
    })
}

impl Document {
    /// The command that puts each new piece in the mover group of the brush it
    /// was cut from, so cutting a door leaves no part of it behind as world. One
    /// command for all of them: each would otherwise undo the one before.
    pub(super) fn pieces_join_movers(&self, joins: &[(Id, Id)]) -> Option<Command> {
        let before = self.groups().to_vec();
        let mut after = before.clone();
        for (source, piece) in joins {
            let group = after.iter_mut().find(|g| g.brushes.contains(source));
            group.into_iter().for_each(|g| g.brushes.push(*piece));
        }
        (after != before).then_some(Command::SetGroups { before, after })
    }
}

impl EditorState {
    /// The name and tags the selection has now.
    fn selection_label(&self) -> (String, String) {
        match self.selection {
            Some(Selection::Entity(id)) => self.doc.label(id).map_or_else(Default::default, |l| {
                (l.name.clone().unwrap_or_default(), join_tags(&l.tags))
            }),
            Some(Selection::Brush(id)) => self
                .doc
                .group_of(id)
                .map_or_else(Default::default, |g| (g.name.clone(), join_tags(&g.tags))),
            None => Default::default(),
        }
    }

    /// Fills the draft from the selection if that changed, and says what the
    /// fields should show now.
    pub fn sync_draft(&mut self) -> (String, String) {
        if self.draft.filled_for != self.selection {
            let (name, tags) = self.selection_label();
            self.draft = LabelDraft {
                name,
                tags,
                filled_for: self.selection,
            };
        }
        (self.draft.name.clone(), self.draft.tags.clone())
    }

    /// Applies the draft to the selection: a label on an entity, a mover group
    /// on a brush. Whether anything changed; the message says why not.
    pub fn apply_draft(&mut self) -> bool {
        let (name, tags) = (self.draft.name.clone(), self.draft.tags.clone());
        match self.selection {
            Some(Selection::Entity(id)) => {
                let before = self.doc.label(id).cloned();
                let after = label_of(&name, &tags);
                let cmd = Command::SetLabel {
                    entity: id,
                    before,
                    after,
                };
                self.run_checked(cmd)
            }
            Some(Selection::Brush(id)) => self.set_mover(id, name.trim(), &tags),
            None => {
                self.message = "Nothing selected".into();
                false
            }
        }
    }

    /// Runs a command if the level would still be valid after it.
    fn run_checked(&mut self, cmd: Command) -> bool {
        let mut trial = self.doc.clone();
        cmd.apply(&mut trial);
        if trial == self.doc {
            return false;
        }
        let level = trial.to_level();
        let problem = level.map_err(|e| format!("{e:?}")).and_then(|l| {
            l.check_labels().map_err(|e| match e {
                mscent_world::LabelError::BadName => {
                    "Names and tags are lower-case letters, digits and _, at most 32".to_string()
                }
                mscent_world::LabelError::DuplicateName => "That name is already taken".to_string(),
                other => format!("{other:?}"),
            })
        });
        if let Err(why) = problem {
            self.message = why;
            return false;
        }
        self.history.run(&mut self.doc, cmd);
        self.message = "Names updated".into();
        true
    }

    /// Puts a brush in the mover group of that name, creating it, or takes it
    /// out of any group when the name is empty. A group's tags are the fields'.
    fn set_mover(&mut self, brush: u32, name: &str, tags: &str) -> bool {
        if !name.is_empty() && self.doc.brush(brush).is_some_and(|b| b.op != Op::Add) {
            self.message = "A mover must be an add brush: a moving hole has no meaning".into();
            return false;
        }
        let before: Vec<GroupDef> = self.doc.groups().to_vec();
        let member = before
            .iter()
            .any(|g| g.name == name && g.brushes.contains(&brush));
        let mut after = before.clone();
        after
            .iter_mut()
            .for_each(|g| g.brushes.retain(|b| *b != brush));
        if !name.is_empty() {
            if let Some(group) = after.iter_mut().find(|g| g.name == name) {
                group.brushes.push(brush);
                set_group_tags(group, split_tags(tags), member);
            } else {
                after.push(GroupDef {
                    name: name.to_string(),
                    tags: split_tags(tags),
                    brushes: vec![brush],
                });
            }
        }
        after.retain(|g| !g.brushes.is_empty());
        self.run_checked(Command::SetGroups { before, after })
    }

    /// Edit > Make mover: the selected brush moves, under a name nothing has.
    pub fn make_mover(&mut self) -> bool {
        let Some(Selection::Brush(id)) = self.selection else {
            self.message = "Select a brush first".into();
            return false;
        };
        if let Some(group) = self.doc.group_of(id) {
            self.message = format!("It is already the mover '{}'", group.name);
            return false;
        }
        let free = (1u32..).map(|n| format!("mover_{n}"));
        let name = free
            .into_iter()
            .find(|n| !self.doc.name_taken(n))
            .unwrap_or_default();
        let done = self.set_mover(id, &name, "");
        if done {
            self.draft.filled_for = None;
            self.message = format!("This brush is now the mover '{name}'");
        }
        done
    }

    /// Which lines of the script name the selection, for the panel. Read only:
    /// the wiring lives in the script, and this shows where.
    pub fn script_uses(&self) -> String {
        let (name, _) = self.selection_label();
        if name.is_empty() {
            return String::new();
        }
        if !self.script.has_file() && self.script.text().is_empty() {
            return "No script beside this level".into();
        }
        let quoted = format!("'{name}'");
        let lines = self.script.text().lines().enumerate();
        let found: Vec<String> = lines
            .filter(|(_, l)| l.contains(&quoted))
            .map(|(i, _)| (i + 1).to_string())
            .take(8)
            .collect();
        if found.is_empty() {
            "The script does not name this".into()
        } else {
            format!("Script lines: {}", found.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityKind;

    fn state() -> EditorState {
        let mut st = EditorState::new(Document::new());
        st.new_op = Op::Subtract;
        st.add_box([-8.0; 3], [8.0; 3]).unwrap();
        st.new_op = Op::Add;
        st
    }

    fn draft(st: &mut EditorState, name: &str, tags: &str) {
        st.sync_draft();
        st.draft.name = name.into();
        st.draft.tags = tags.into();
    }

    #[test]
    fn an_entity_is_named_tagged_saved_and_undone() {
        let mut st = state();
        let id = st.place_entity(EntityKind::Marker, [1.0, 1.0, 1.0]);
        st.selection = Some(Selection::Entity(id));
        draft(&mut st, "hinge", "doors, east");
        assert!(st.apply_draft());
        let level = st.doc.to_level().unwrap();
        assert_eq!(level.labels[0].name.as_deref(), Some("hinge"));
        assert_eq!(level.labels[0].tags, ["doors", "east"]);
        assert!(st.undo());
        assert!(st.doc.to_level().unwrap().labels.is_empty());
        assert!(st.redo());
        assert_eq!(
            st.doc.label(id).and_then(|l| l.name.as_deref()),
            Some("hinge")
        );
    }

    #[test]
    fn a_bad_or_taken_name_is_refused_and_changes_nothing() {
        let mut st = state();
        let a = st.place_entity(EntityKind::Marker, [1.0, 1.0, 1.0]);
        let b = st.place_entity(EntityKind::Marker, [2.0, 1.0, 1.0]);
        st.selection = Some(Selection::Entity(a));
        draft(&mut st, "taken", "");
        assert!(st.apply_draft());
        st.selection = Some(Selection::Entity(b));
        draft(&mut st, "taken", "");
        assert!(!st.apply_draft());
        assert!(st.message.contains("already taken"), "{}", st.message);
        draft(&mut st, "Bad Name", "");
        assert!(!st.apply_draft());
        assert!(st.message.contains("lower-case"), "{}", st.message);
        assert!(st.doc.label(b).is_none());
    }

    #[test]
    fn clearing_the_fields_removes_the_label() {
        let mut st = state();
        let id = st.place_entity(EntityKind::Marker, [1.0, 1.0, 1.0]);
        st.selection = Some(Selection::Entity(id));
        draft(&mut st, "x", "");
        assert!(st.apply_draft());
        draft(&mut st, "", "");
        assert!(st.apply_draft());
        assert!(st.doc.label(id).is_none());
        assert!(st.doc.to_level().unwrap().check_labels().is_ok());
    }

    #[test]
    fn make_mover_takes_the_brush_out_of_the_world_and_undo_puts_it_back() {
        let mut st = state();
        let id = st.add_box([1.0; 3], [2.0; 3]).unwrap();
        st.selection = Some(Selection::Brush(id));
        assert!(st.make_mover());
        assert!(st.message.contains("mover_1"), "{}", st.message);
        let level = st.doc.to_level().unwrap();
        assert_eq!(level.mover_groups.len(), 1);
        assert_eq!(level.static_brushes().len(), 1);
        assert!(!st.make_mover(), "once");
        assert!(st.undo());
        assert!(st.doc.to_level().unwrap().mover_groups.is_empty());
    }

    #[test]
    fn a_subtract_brush_cannot_be_a_mover() {
        let mut st = state();
        let room = st.doc.brushes()[0].id;
        st.selection = Some(Selection::Brush(room));
        assert!(!st.make_mover());
        assert!(st.message.contains("add brush"), "{}", st.message);
        assert!(st.doc.groups().is_empty());
    }

    #[test]
    fn brushes_with_the_same_name_move_together_and_renaming_moves_one_out() {
        let mut st = state();
        let (a, b) = (
            st.add_box([1.0; 3], [2.0; 3]).unwrap(),
            st.add_box([3.0; 3], [4.0; 3]).unwrap(),
        );
        st.selection = Some(Selection::Brush(a));
        draft(&mut st, "gate", "doors");
        assert!(st.apply_draft());
        st.selection = Some(Selection::Brush(b));
        draft(&mut st, "gate", "doors");
        assert!(st.apply_draft());
        assert_eq!(st.doc.groups().len(), 1);
        assert_eq!(st.doc.groups()[0].brushes, [a, b]);
        draft(&mut st, "", "");
        assert!(st.apply_draft());
        assert_eq!(st.doc.groups()[0].brushes, [a]);
    }

    #[test]
    fn joining_a_group_keeps_its_tags_and_a_member_can_replace_them() {
        let mut st = state();
        let (a, b) = (
            st.add_box([1.0; 3], [2.0; 3]).unwrap(),
            st.add_box([3.0; 3], [4.0; 3]).unwrap(),
        );
        st.selection = Some(Selection::Brush(a));
        draft(&mut st, "gate", "doors");
        assert!(st.apply_draft());
        st.selection = Some(Selection::Brush(b));
        draft(&mut st, "gate", "");
        assert!(st.apply_draft());
        assert_eq!(st.doc.groups()[0].tags, ["doors"]);
        draft(&mut st, "gate", "east");
        assert!(st.apply_draft());
        assert_eq!(st.doc.groups()[0].tags, ["east"]);
    }

    #[test]
    fn a_level_whose_names_clash_is_not_saved_over_the_file() {
        let mut st = state();
        let brush = st.add_box([1.0; 3], [2.0; 3]).unwrap();
        st.selection = Some(Selection::Brush(brush));
        draft(&mut st, "door", "");
        assert!(st.apply_draft());
        // As a subtract brush it is no mover, which frees the name.
        assert!(st.flip_selected());
        let marker = st.place_entity(EntityKind::Marker, [3.0, 3.0, 3.0]);
        st.selection = Some(Selection::Entity(marker));
        draft(&mut st, "door", "");
        assert!(st.apply_draft());
        st.selection = Some(Selection::Brush(brush));
        assert!(st.flip_selected());

        let path = std::env::temp_dir().join(format!("mscent-clash-{}.msl", std::process::id()));
        std::fs::write(&path, b"what was there").unwrap();
        let why = crate::files::save(&st.doc, &path).unwrap_err();
        assert!(why.contains("names"), "{why}");
        assert_eq!(std::fs::read(&path).unwrap(), b"what was there");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn undo_refills_the_fields() {
        let mut st = state();
        let id = st.add_box([1.0; 3], [2.0; 3]).unwrap();
        st.selection = Some(Selection::Brush(id));
        assert!(st.make_mover());
        assert_eq!(st.sync_draft().0, "mover_1");
        assert!(st.undo());
        assert_eq!(st.sync_draft().0, "");
    }

    #[test]
    fn cutting_a_mover_keeps_every_piece_in_it_and_undo_restores_it() {
        let mut st = state();
        let (a, b) = (
            st.add_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap(),
            st.add_box([3.0, 0.0, 0.0], [5.0, 2.0, 2.0]).unwrap(),
        );
        for id in [a, b] {
            st.selection = Some(Selection::Brush(id));
            draft(&mut st, "gate", "");
            assert!(st.apply_draft());
        }
        // The cutter crosses both, so both leave pieces, in one command.
        let cutter = st.add_box([1.0, 0.5, -1.0], [4.0, 1.5, 3.0]).unwrap();
        st.selection = Some(Selection::Brush(cutter));
        assert!(st.carve_selected());
        let group = &st.doc.groups()[0];
        let kept = st.doc.brushes().iter().filter(|b| b.op == Op::Add).count();
        assert!(group.brushes.len() > 2, "{group:?}");
        assert_eq!(group.brushes.len(), kept, "no piece left behind as world");
        assert!(st.undo());
        assert_eq!(st.doc.groups()[0].brushes, [a, b]);
    }

    #[test]
    fn a_new_selection_fills_the_fields_and_typing_is_kept_until_it_changes() {
        let mut st = state();
        let a = st.place_entity(EntityKind::Marker, [1.0, 1.0, 1.0]);
        st.selection = Some(Selection::Entity(a));
        draft(&mut st, "here", "");
        assert!(st.apply_draft());
        st.draft.name = "half typed".into();
        assert_eq!(st.sync_draft().0, "half typed");
        st.selection = None;
        assert_eq!(st.sync_draft().0, "");
        st.selection = Some(Selection::Entity(a));
        assert_eq!(st.sync_draft().0, "here");
    }

    #[test]
    fn script_uses_lists_the_lines_that_name_the_selection() {
        let dir = std::env::temp_dir().join(format!("mscent-label-uses-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("l.st"), "x := 1.\n(mover: 'door') y.\n'door'.\n").unwrap();
        let mut st = state();
        let id = st.add_box([1.0; 3], [2.0; 3]).unwrap();
        st.selection = Some(Selection::Brush(id));
        assert!(st.make_mover() && st.script_uses() == "No script beside this level");
        st.draft.filled_for = None;
        draft(&mut st, "door", "");
        assert!(st.apply_draft());
        st.doc.set_level_path(Some(dir.join("l.msl")));
        st.script.open(Some(&dir.join("l.st")));
        assert_eq!(st.script_uses(), "Script lines: 2, 3");
        let _ = std::fs::remove_dir_all(dir);
    }
}
