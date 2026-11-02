//! Names, tags and mover groups in the editor, and where the level's script
//! uses them. Every change is checked against the rules a saved level must meet
//! before it is applied, so the document stays one that can be saved.

use std::time::{Duration, Instant};

use super::command::Command;
use super::document::{GroupDef, LabelDef};
use super::state::{EditorState, Selection};
use mscent_world::Op;

/// How often the script beside the level is read again.
const SCRIPT_POLL: Duration = Duration::from_secs(1);

/// The panel's name and tag fields, while being typed in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelDraft {
    pub name: String,
    pub tags: String,
    /// What the fields were filled from, so a new selection fills them again.
    pub filled_for: Option<Selection>,
}

/// The script beside the level, as last read.
#[derive(Clone, Debug, Default)]
pub struct ScriptText {
    text: Option<String>,
    read_at: Option<Instant>,
}

fn split_tags(tags: &str) -> Vec<String> {
    let parts = tags.split([',', ' ']).filter(|t| !t.is_empty());
    parts.map(str::to_string).collect()
}

fn join_tags(tags: &[String]) -> String {
    tags.join(", ")
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
        let mut after = before.clone();
        after
            .iter_mut()
            .for_each(|g| g.brushes.retain(|b| *b != brush));
        if !name.is_empty() {
            if let Some(group) = after.iter_mut().find(|g| g.name == name) {
                group.brushes.push(brush);
                group.tags = split_tags(tags);
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

    /// Reads the script beside the level again, at most once a second.
    pub fn poll_script(&mut self) {
        let due = self
            .script
            .read_at
            .is_none_or(|t| t.elapsed() >= SCRIPT_POLL);
        if !due {
            return;
        }
        let path = self.doc.level_path().map(|p| p.with_extension("st"));
        self.script.text = path.and_then(|p| std::fs::read_to_string(p).ok());
        self.script.read_at = Some(Instant::now());
    }

    /// Which lines of the script name the selection, for the panel. Read only:
    /// the wiring lives in the script, and this shows where.
    pub fn script_uses(&self) -> String {
        let (name, _) = self.selection_label();
        if name.is_empty() {
            return String::new();
        }
        let Some(text) = &self.script.text else {
            return "No script beside this level".into();
        };
        let quoted = format!("'{name}'");
        let lines = text.lines().enumerate();
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
    use crate::model::{Document, EntityKind};

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
        st.poll_script();
        assert_eq!(st.script_uses(), "Script lines: 2, 3");
        let _ = std::fs::remove_dir_all(dir);
    }
}
