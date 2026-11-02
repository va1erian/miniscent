//! The level's script as being edited: the text, what the file beside the level
//! held when last read, and what the code editor still has to be told. Scripts
//! ship with levels, so what is read is checked like any other file.

use std::path::Path;
use std::time::{Duration, Instant};

use miniscent_game::scripts::MAX_SOURCE;

/// How often the file is read again, for an edit made in another program.
const POLL: Duration = Duration::from_secs(1);

/// What a script file holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Disk {
    #[default]
    Missing,
    Text(String),
    /// There is a file, but over the limit or not text. It is never overwritten
    /// by nothing at all, which is what the buffer would hold.
    Unreadable,
}

/// A file past the limit would be refused anyway, and this runs on the UI thread.
/// The text comes as read: the buffer decides about its line ends.
fn read_raw(path: &Path) -> Disk {
    let Ok(meta) = std::fs::metadata(path) else {
        return Disk::Missing;
    };
    if meta.len() > MAX_SOURCE as u64 {
        return Disk::Unreadable;
    }
    std::fs::read_to_string(path).map_or(Disk::Unreadable, Disk::Text)
}

/// A change the widget has not seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetEdit {
    /// Other text entirely: a level was opened, or the file changed. Undo starts
    /// over.
    Set(String),
    /// Text added at the end, as one step of the widget's own undo.
    Append(String),
}

#[derive(Clone, Debug, Default)]
pub struct ScriptBuffer {
    text: String,
    disk: Disk,
    read_at: Option<Instant>,
    for_widget: Vec<WidgetEdit>,
    /// The file changed while the buffer has edits of its own.
    conflict: bool,
    /// The file ends its lines in CRLF, as it does again when written. In memory
    /// they are always LF, which is what the code editor works in.
    crlf: bool,
}

impl ScriptBuffer {
    /// What the file holds, with LF line ends, noting which it had.
    fn read(&mut self, path: Option<&Path>) -> Disk {
        match path.map_or(Disk::Missing, read_raw) {
            Disk::Text(raw) => {
                self.crlf = raw.contains("\r\n");
                Disk::Text(raw.replace("\r\n", "\n"))
            }
            other => other,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether there are edits no file holds.
    pub fn dirty(&self) -> bool {
        match &self.disk {
            Disk::Text(on_disk) => *on_disk != self.text,
            Disk::Missing | Disk::Unreadable => !self.text.is_empty(),
        }
    }

    /// Whether there is a file beside the level, even an empty or unusable one.
    pub fn has_file(&self) -> bool {
        self.disk != Disk::Missing
    }

    pub fn conflict(&self) -> bool {
        self.conflict
    }

    /// Starts over on the script beside a newly opened level.
    pub fn open(&mut self, path: Option<&Path>) {
        self.disk = self.read(path);
        self.text = match &self.disk {
            Disk::Text(text) => text.clone(),
            Disk::Missing | Disk::Unreadable => String::new(),
        };
        self.read_at = Some(Instant::now());
        self.conflict = false;
        self.for_widget = vec![WidgetEdit::Set(self.text.clone())];
    }

    /// What the user typed.
    pub fn edited(&mut self, text: String) {
        self.text = text;
    }

    /// Adds a block of script at the end, with a blank line before it. Returns
    /// the zero-based line it starts on.
    pub fn append_block(&mut self, block: &str) -> usize {
        let mut added = String::new();
        if !self.text.is_empty() {
            if !self.text.ends_with('\n') {
                added.push('\n');
            }
            added.push('\n');
        }
        added.push_str(block);
        self.text.push_str(&added);
        self.for_widget.push(WidgetEdit::Append(added));
        self.text.lines().count() - block.lines().count()
    }

    /// Everything the widget has yet to apply, oldest first.
    pub fn take_widget_edits(&mut self) -> Vec<WidgetEdit> {
        std::mem::take(&mut self.for_widget)
    }

    /// Reads the file again if it is time. A change with no edits in the way is
    /// taken in; with edits, they stay and [`ScriptBuffer::conflict`] says so.
    pub fn poll(&mut self, path: Option<&Path>) {
        let due = self.read_at.is_none_or(|t| t.elapsed() >= POLL);
        if !due {
            return;
        }
        self.read_at = Some(Instant::now());
        let now = self.read(path);
        if now == self.disk {
            return;
        }
        if self.dirty() {
            self.conflict = true;
            self.disk = now;
        } else {
            self.take_in(now);
        }
    }

    fn take_in(&mut self, disk: Disk) {
        self.disk = disk;
        self.text = match &self.disk {
            Disk::Text(text) => text.clone(),
            Disk::Missing | Disk::Unreadable => String::new(),
        };
        self.conflict = false;
        self.for_widget.push(WidgetEdit::Set(self.text.clone()));
    }

    /// Throws the edits away for what the file holds now.
    pub fn reload(&mut self, path: Option<&Path>) {
        let now = self.read(path);
        self.take_in(now);
    }

    /// Writes the text beside the level, through a temporary file so a failed
    /// write leaves the old one. An empty script with no file makes no file: a
    /// level with one, even empty, needs no goal. `Ok(true)` means it wrote.
    pub fn save(&mut self, path: &Path) -> Result<bool, String> {
        if self.text.len() > MAX_SOURCE {
            return Err(format!(
                "the script is {} bytes; the limit is {MAX_SOURCE}",
                self.text.len()
            ));
        }
        let now = self.read(Some(path));
        let same = now == Disk::Text(self.text.clone());
        let nothing = self.text.is_empty() && matches!(now, Disk::Missing | Disk::Unreadable);
        if same || nothing {
            self.disk = now;
            self.conflict = false;
            return Ok(false);
        }
        let tmp = path.with_extension("st.tmp");
        let bytes = if self.crlf {
            self.text.replace('\n', "\r\n")
        } else {
            self.text.clone()
        };
        std::fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("cannot write {}: {e}", path.display()));
        }
        self.disk = Disk::Text(self.text.clone());
        self.conflict = false;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mscent-buffer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Makes the next poll read the file.
    fn poll_now(b: &mut ScriptBuffer, path: &Path) {
        b.read_at = None;
        b.poll(Some(path));
    }

    #[test]
    fn opening_reads_the_file_and_tells_the_widget_even_when_empty() {
        let path = temp("open.st");
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        assert_eq!(b.take_widget_edits(), [WidgetEdit::Set(String::new())]);
        assert!(!b.dirty() && !b.has_file());
        std::fs::write(&path, "say: 'a'.").unwrap();
        b.open(Some(&path));
        assert_eq!(b.text(), "say: 'a'.");
        assert!(!b.dirty() && b.has_file());
    }

    #[test]
    fn saving_writes_once_and_an_untouched_empty_script_makes_no_file() {
        let path = temp("save.st");
        let mut b = ScriptBuffer::default();
        assert_eq!(b.save(&path), Ok(false));
        assert!(
            !path.exists(),
            "no script, no file: the level still needs a goal"
        );
        b.edited("win.".into());
        assert!(b.dirty());
        assert_eq!(b.save(&path), Ok(true));
        assert!(!b.dirty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "win.");
        assert_eq!(b.save(&path), Ok(false), "nothing new");
        assert!(!path.with_extension("st.tmp").exists());
    }

    #[test]
    fn clearing_a_script_that_has_a_file_empties_the_file() {
        let path = temp("clear.st");
        std::fs::write(&path, "win.").unwrap();
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        b.edited(String::new());
        assert!(b.dirty());
        assert_eq!(b.save(&path), Ok(true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn an_unusable_file_is_not_overwritten_by_nothing() {
        let path = temp("big.st");
        std::fs::write(&path, vec![b' '; MAX_SOURCE + 1]).unwrap();
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        assert_eq!(b.text(), "", "past the limit is not read");
        assert_eq!(b.save(&path), Ok(false));
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            MAX_SOURCE as u64 + 1
        );
        std::fs::write(&path, [0xffu8, 0xfe, 0x00]).unwrap();
        b.open(Some(&path));
        assert_eq!(b.save(&path), Ok(false), "not text is not read either");
        assert_eq!(std::fs::read(&path).unwrap(), [0xff, 0xfe, 0x00]);
    }

    #[test]
    fn a_script_over_the_limit_is_refused_and_the_file_kept() {
        let path = temp("over.st");
        std::fs::write(&path, "win.").unwrap();
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        b.edited(" ".repeat(MAX_SOURCE + 1));
        assert!(b.save(&path).unwrap_err().contains("limit"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "win.");
        assert!(b.dirty());
    }

    #[test]
    fn a_failed_write_says_so_and_keeps_the_edits() {
        let path = temp("no-such-dir").join("level.st");
        let mut b = ScriptBuffer::default();
        b.edited("win.".into());
        assert!(b.save(&path).is_err());
        assert!(b.dirty() && b.text() == "win.");
    }

    #[test]
    fn a_change_outside_is_taken_in_when_there_are_no_edits() {
        let path = temp("outside.st");
        std::fs::write(&path, "win.").unwrap();
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        b.take_widget_edits();
        std::fs::write(&path, "lose.").unwrap();
        poll_now(&mut b, &path);
        assert_eq!(b.text(), "lose.");
        assert_eq!(b.take_widget_edits(), [WidgetEdit::Set("lose.".into())]);
        assert!(!b.dirty() && !b.conflict());
    }

    #[test]
    fn a_change_outside_does_not_overwrite_edits_and_either_side_can_win() {
        let path = temp("conflict.st");
        std::fs::write(&path, "win.").unwrap();
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        b.take_widget_edits();
        b.edited("mine.".into());
        std::fs::write(&path, "theirs.").unwrap();
        poll_now(&mut b, &path);
        assert_eq!(b.text(), "mine.");
        assert!(b.conflict() && b.dirty());
        assert!(b.take_widget_edits().is_empty());
        b.reload(Some(&path));
        assert_eq!(b.text(), "theirs.");
        assert!(!b.conflict() && !b.dirty());

        b.edited("mine.".into());
        std::fs::write(&path, "again.").unwrap();
        poll_now(&mut b, &path);
        assert!(b.conflict());
        assert_eq!(b.save(&path), Ok(true));
        assert!(!b.conflict());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine.");
    }

    #[test]
    fn a_file_with_crlf_line_ends_is_edited_in_lf_and_keeps_them_on_disk() {
        let path = temp("crlf.st");
        std::fs::write(&path, "say: 'a'.\r\nwin.\r\n").unwrap();
        let mut b = ScriptBuffer::default();
        b.open(Some(&path));
        assert_eq!(b.text(), "say: 'a'.\nwin.\n");
        assert_eq!(
            b.take_widget_edits(),
            [WidgetEdit::Set("say: 'a'.\nwin.\n".into())]
        );
        assert!(!b.dirty(), "the same text is no edit");
        assert_eq!(b.save(&path), Ok(false), "and nothing to write");
        poll_now(&mut b, &path);
        assert!(b.take_widget_edits().is_empty() && !b.conflict());
        b.edited("lose.\nwin.\n".into());
        assert_eq!(b.save(&path), Ok(true));
        assert_eq!(std::fs::read(&path).unwrap(), b"lose.\r\nwin.\r\n");
        poll_now(&mut b, &path);
        assert!(
            b.take_widget_edits().is_empty() && !b.conflict(),
            "our own write"
        );
    }

    #[test]
    fn saving_our_own_file_is_not_seen_as_a_change_outside() {
        let path = temp("own.st");
        let mut b = ScriptBuffer::default();
        b.edited("win.".into());
        b.save(&path).unwrap();
        b.take_widget_edits();
        poll_now(&mut b, &path);
        assert!(b.take_widget_edits().is_empty() && !b.conflict());
    }

    #[test]
    fn an_appended_block_says_its_line_and_keeps_a_blank_line_above() {
        let mut b = ScriptBuffer::default();
        assert_eq!(b.append_block("win.\n"), 0);
        assert_eq!(b.append_block("lose.\n"), 2);
        assert_eq!(b.text(), "win.\n\nlose.\n");
        b.edited("no newline".into());
        assert_eq!(b.append_block("x.\n"), 2);
        assert_eq!(b.text(), "no newline\n\nx.\n");
    }
}
