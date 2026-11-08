//! The editor manual's window: the pages on the left, the page on show as one
//! HTML view on the right, as the script pane's guide is. It is a window of its
//! own so it can sit beside the views while the reader tries what it says.
//! Like the script pane's window it has no app of its own: what happens in it
//! comes to the editor as `Msg::Manual`, which the headless backend of scripted
//! runs can deliver, where it runs no second window's messages.
//!
//! The page is made from the manual's own text by `guide_html.rs`, escaped, with
//! no links: a click in it does nothing but select text, and Ctrl+C copies that.

use std::cell::Cell;

use xui::app::WindowHandle;
use xui::backend::{PlatformSpec, Result as XuiResult};
use xui::geometry::Rect;
use xui::prelude::*;
use xui::widget::{Fill, ListView};
use xui::{Key, dip};
use xui_blitz::{BlitzView, BlitzViewEvent};

use super::app::{Editor, Msg};
use super::guide_html::{self, Examples};
use crate::manual;
use crate::pages::Page;
use crate::studio::Mode;

/// What happens to the manual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManualMsg {
    /// Help, Editor manual or F1: open it on the page about the tool in hand.
    Open,
    /// A page, by index.
    Topic(usize),
    /// The view has laid a page out and wants it drawn.
    Frame,
    /// Text selected in the page, for the clipboard.
    Copy(String),
    /// Esc or the close button.
    Close,
}

/// The size the window opens at.
const SIZE: (f32, f32) = (900.0, 680.0);

/// Width of the list of pages.
const TOPICS_WIDTH: i32 = 210;

fn manual_msg(m: ManualMsg) -> Msg {
    Msg::Manual(m)
}

/// The manual, open.
pub struct Manual {
    window: WindowHandle<Msg>,
    pages: Vec<Page>,
    view: Handle<BlitzView<Msg>>,
    topics: Handle<ListView<Msg>>,
    /// The page on show.
    topic: Cell<usize>,
}

/// The window forwards everything to the editor.
struct ManualWindow {
    editor: Ui<Msg>,
}

impl App for ManualWindow {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, _ui: &mut Ui<Msg>) {
        self.editor.emit(msg);
    }
}

/// The page view. Its pages have no links, and a link the view reports anyway
/// is dropped: the only thing a page can ask for is a copy of its text.
fn page_view(ui: &Ui<Msg>) -> XuiResult<BlitzView<Msg>> {
    BlitzView::builder(
        || manual_msg(ManualMsg::Frame),
        |event| match event {
            BlitzViewEvent::CopyRequested(text) => Some(manual_msg(ManualMsg::Copy(text))),
            _ => None,
        },
    )
    .build(ui, Rect::default())
}

impl Manual {
    /// Opens the window; it shows no page until `show`.
    fn open(ui: &Ui<Msg>) -> XuiResult<Manual> {
        let (view, topics) = (Handle::new(), Handle::new());
        let pages = manual::pages();
        let titles: Vec<String> = pages.iter().map(|p| p.title.clone()).collect();
        let spec = PlatformSpec::new("Editor manual").size(dip(SIZE.0), dip(SIZE.1));
        let (view_in, topics_in, editor) = (view.clone(), topics.clone(), ui.clone());
        let window = ui.open_window(spec, move |ui| {
            ui.on_key(|key, _| (key == Key::ESCAPE).then(|| manual_msg(ManualMsg::Close)));
            ui.on_close(|| Some(manual_msg(ManualMsg::Close)));
            let list = list()
                .column("Pages", Fill)
                .then(|l| l.on_select(|row| Some(manual_msg(ManualMsg::Topic(row)))))
                .bind(&topics_in);
            let root = row().gap(6).padding(4).children((
                column().children((list.fill(1),)).width(TOPICS_WIDTH),
                build(page_view).bind(&view_in).fill(1),
            ));
            if ui.root(root).is_err() {
                ui.close();
            } else {
                topics_in.get().set_model(titles);
            }
            ManualWindow { editor }
        })?;
        Ok(Manual {
            window,
            pages,
            view,
            topics,
            topic: Cell::new(usize::MAX),
        })
    }

    fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// Shows page `topic`; one past the end, or the page on show, changes nothing.
    fn show(&self, topic: usize, theme: &xui::theme::Theme) {
        let Some(page) = self.pages.get(topic) else {
            return;
        };
        if topic == self.topic.replace(topic) {
            return;
        }
        let made = guide_html::render(page, theme, Examples::Plain);
        let view = self.view.get();
        view.set_background(guide_html::background(theme));
        view.load_html(made.html, "about:blank");
        let list = self.topics.get();
        if list.selected() != Some(topic) {
            list.select(Some(topic));
        }
    }

    /// Whether the page on show has been laid out.
    pub fn is_ready(&self) -> bool {
        self.is_open() && self.view.get().is_ready()
    }

    /// The page on show.
    pub fn topic(&self) -> usize {
        self.topic.get()
    }

    /// A picture of the window.
    pub fn capture(&self) -> XuiResult<xui::Image> {
        self.window.capture()
    }
}

impl Editor {
    pub(super) fn manual_message(&mut self, msg: ManualMsg, ui: &Ui<Msg>) {
        let open = self.manual.borrow().as_ref().is_some_and(Manual::is_open);
        match msg {
            ManualMsg::Open => self.open_manual(ui),
            ManualMsg::Topic(topic) if open => {
                if let Some(m) = self.manual.borrow().as_ref() {
                    m.show(topic, &ui.theme());
                }
            }
            ManualMsg::Frame if open => {
                if let Some(m) = self.manual.borrow().as_ref() {
                    m.view.get().update();
                }
            }
            ManualMsg::Copy(text) => ui.set_clipboard_text(&text),
            ManualMsg::Close => {
                if let Some(m) = self.manual.borrow_mut().take() {
                    m.window.close();
                }
            }
            ManualMsg::Topic(_) | ManualMsg::Frame => {}
        }
    }

    /// The page about the tool in hand, in the window that is up or a new one.
    fn open_manual(&mut self, ui: &Ui<Msg>) {
        // Play holds the keyboard and the pointer; a window would take them.
        if self.studio.mode == Mode::Play {
            return;
        }
        let topic = manual::topic_for(self.state.borrow().tool);
        let open = self.manual.borrow().as_ref().is_some_and(Manual::is_open);
        if !open {
            match Manual::open(ui) {
                Ok(m) => *self.manual.borrow_mut() = Some(m),
                Err(e) => {
                    self.state.borrow_mut().message = format!("Cannot open the manual: {e:?}");
                    return;
                }
            }
        }
        if let Some(m) = self.manual.borrow().as_ref() {
            m.show(topic, &ui.theme());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::{TOOLS, key_name, shortcut};
    use super::Msg;
    use crate::manual;
    use crate::pages::Block;
    use xui::{Key, Modifiers};

    /// The rows of the manual's page `title`.
    fn rows(title: &str) -> Vec<(String, String)> {
        let pages = manual::pages();
        let page = pages.iter().find(|p| p.title == title).expect("the page");
        let rows = page.blocks.iter().filter_map(|b| match b {
            Block::Row(a, b) => Some((a.clone(), b.clone())),
            _ => None,
        });
        rows.collect()
    }

    #[test]
    fn the_keys_page_names_every_tool_by_its_key_and_its_palette_name() {
        let rows = rows("Keys");
        for (tip, key, _, _) in TOOLS {
            let name = tip.split([':', '(']).next().unwrap_or(tip).trim();
            let key = key_name(key);
            assert!(
                rows.iter().any(|(k, n)| *k == key && n == name),
                "no row {key} | {name}"
            );
        }
    }

    fn named(key: Key) -> Option<String> {
        let named = [
            (Key::ESCAPE, "Esc"),
            (Key::DELETE, "Delete"),
            (Key::RETURN, "Enter"),
        ];
        if let Some((_, n)) = named.iter().find(|(k, _)| *k == key) {
            return Some((*n).to_string());
        }
        let code = key.code();
        if (0x70..=0x7B).contains(&code) {
            return Some(format!("F{}", code - 0x6F));
        }
        let letter = key_name(key);
        (letter.len() == 1 && letter.chars().all(|c| c.is_ascii_uppercase())).then_some(letter)
    }

    /// What a shortcut does, to tell a key with Shift from the same key without.
    fn what(msg: Option<Msg>) -> Option<String> {
        match msg? {
            Msg::Guarded(inner) => what(Some(*inner)),
            Msg::Menu(id) => Some(format!("menu {id:?}")),
            Msg::Tool(i) => Some(format!("tool {i}")),
            Msg::Escape => Some("escape".into()),
            _ => Some("other".into()),
        }
    }

    /// Every key the window answers to, written as the manual writes it, is on
    /// the Keys page. Shift that changes nothing (Ctrl+Shift+S is Ctrl+S) is not
    /// a key of its own.
    #[test]
    fn every_shortcut_is_on_the_keys_page() {
        let rows = rows("Keys");
        let listed = |name: &str| {
            rows.iter()
                .any(|(keys, _)| keys.split(" or ").any(|k| k == name))
        };
        let mods = [(false, false), (true, false), (true, true), (false, true)];
        for code in 0..=255u16 {
            let key = Key::from_code(code);
            let Some(name) = named(key) else { continue };
            for (ctrl, shift) in mods {
                let m = Modifiers {
                    ctrl,
                    shift,
                    ..Modifiers::default()
                };
                let does = what(shortcut(key, m));
                let unshifted = Modifiers { shift: false, ..m };
                if does.is_none() || (shift && does == what(shortcut(key, unshifted))) {
                    continue;
                }
                let written = match (ctrl, shift) {
                    (true, true) => format!("Ctrl+Shift+{name}"),
                    (true, false) => format!("Ctrl+{name}"),
                    // Shift on a plain key is the key itself to the hook.
                    _ => name.clone(),
                };
                assert!(listed(&written), "{written} is not on the Keys page");
            }
        }
    }
}
