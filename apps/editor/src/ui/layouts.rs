//! The two ways to arrange the four views, both with draggable splitters:
//! the classic one (top, front and side in a row over the 3D view) and the
//! Hammer one (two columns, two rows). A layout cannot move a widget between
//! containers, so both arrangements are built, each with its own views over
//! the same state, and the inactive one is hidden.

use std::cell::RefCell;
use std::rc::Rc;

use xui::prelude::*;
use xui::widget::{MenuId, Split};

use super::app::{Editor, Msg, cmd};
use super::view3d::View3d;
use super::viewport::{ViewEvent, Viewport};
use crate::model::{EditorState, View};

/// Which arrangement of the views is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Top, front and side in a row over the 3D view.
    Classic,
    /// Top and 3D over front and side, as in Hammer.
    Quad,
}

/// The views of one arrangement and its root splitter.
#[derive(Default)]
pub struct Arrangement {
    pub viewports: [Handle<Viewport<Msg>>; 3],
    pub view3d: Handle<View3d<Msg>>,
    pub root: Handle<Split<Msg>>,
}

impl Arrangement {
    /// Shows or hides the whole arrangement.
    pub fn set_visible(&self, visible: bool) {
        self.root.get().set_visible(visible);
    }
}

fn pane(
    arr: &Arrangement,
    i: usize,
    state: &Rc<RefCell<EditorState>>,
    emit: &Rc<dyn Fn(ViewEvent) -> Msg>,
) -> Layout<Msg> {
    let (state, emit) = (state.clone(), emit.clone());
    let view = View::ALL[i];
    column().children((build(move |ui| Viewport::new(ui, view, state, emit))
        .bind(&arr.viewports[i])
        .fill(1),))
}

fn pane3d(arr: &Arrangement) -> Layout<Msg> {
    column().children((build(View3d::new).bind(&arr.view3d).fill(1),))
}

fn one(split: SplitBuild<Msg>) -> Layout<Msg> {
    column().children((split.fill(1),))
}

/// Top | (front | side) over the 3D view.
pub fn classic(
    arr: &Arrangement,
    state: &Rc<RefCell<EditorState>>,
    emit: &Rc<dyn Fn(ViewEvent) -> Msg>,
) -> Layout<Msg> {
    let ortho = split(
        pane(arr, 0, state, emit),
        one(split(pane(arr, 1, state, emit), pane(arr, 2, state, emit)).min(60, 60)),
    )
    .position(270)
    .min(60, 120);
    column().children((split(one(ortho), pane3d(arr))
        .stacked()
        .min(60, 60)
        .bind(&arr.root)
        .fill(1),))
}

/// Top | 3D over front | side.
pub fn quad(
    arr: &Arrangement,
    state: &Rc<RefCell<EditorState>>,
    emit: &Rc<dyn Fn(ViewEvent) -> Msg>,
) -> Layout<Msg> {
    let upper = split(pane(arr, 0, state, emit), pane3d(arr)).min(60, 60);
    let lower = split(pane(arr, 1, state, emit), pane(arr, 2, state, emit)).min(60, 60);
    column().children((split(one(upper), one(lower))
        .stacked()
        .min(60, 60)
        .bind(&arr.root)
        .fill(1),))
}

impl Editor {
    /// The arrangement on show.
    fn arrangement(&self) -> &Arrangement {
        match self.disposition {
            Disposition::Classic => &self.classic,
            Disposition::Quad => &self.quad,
        }
    }

    /// The visible 3D pane.
    pub(super) fn pane3d(&self) -> Rc<View3d<Msg>> {
        self.arrangement().view3d.get()
    }

    /// The 2D views of both arrangements.
    pub(super) fn all_viewports(&self) -> impl Iterator<Item = &Handle<Viewport<Msg>>> {
        self.classic.viewports.iter().chain(&self.quad.viewports)
    }

    /// Shows `disposition`, carrying the current 3D picture over.
    pub(super) fn set_disposition(&mut self, disposition: Disposition) {
        let old = self.pane3d();
        let was_focused = self.studio.mode == crate::studio::Mode::Edit;
        self.disposition = disposition;
        self.classic
            .set_visible(disposition == Disposition::Classic);
        self.quad.set_visible(disposition == Disposition::Quad);
        let new = self.pane3d();
        if !Rc::ptr_eq(&old, &new) {
            if let Some((w, h, rgba)) = old.frame_pixels() {
                new.set_rgba(w, h, rgba);
            }
            old.take_events();
            if was_focused {
                new.focus();
            }
        }
        for v in self.all_viewports() {
            v.get().refit();
        }
    }

    /// The View menu's layout commands; `false` for any other command.
    pub(super) fn layout_command(&mut self, id: MenuId) -> bool {
        match id {
            cmd::LAYOUT_CLASSIC => self.set_disposition(Disposition::Classic),
            cmd::LAYOUT_QUAD => self.set_disposition(Disposition::Quad),
            _ => return false,
        }
        true
    }
}
