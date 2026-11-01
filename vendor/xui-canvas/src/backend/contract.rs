#![forbid(unsafe_code)]

//! The [`Backend`] contract over the `winit` event loop.

use std::rc::Rc;
use std::time::Instant;

use winit::window::Window;
use xui_core::backend::{
    Backend, BackendError, Cursor, Event, FontSpec, ImplKind, NodeKind, NodeSpec, Painter,
    ParentRef, PlatformSpec, Result as BackendResult, TextLayout, TextMetrics, TextShaper,
    TextStyle, TimerId, Waker, WidgetId, WindowId,
};
use xui_core::image::Image;
use xui_core::router::WidgetHost;
use xui_core::{Dip, Rect, Theme};

use crate::Surface;
use crate::text_layout::CosmicShaper;

use super::{DEFAULT_DPI, UserEvent, WindowState, WinitBackend, app, render};
use crate::scene::Node;

impl Backend for WinitBackend {
    fn run(&self) -> i32 {
        let Some(event_loop) = self.event_loop.take() else {
            return self.shared.exit_code.get();
        };
        app::run(event_loop, Rc::clone(&self.shared));
        self.shared.exit_code.get()
    }

    fn run_with(&self, window: WindowId, on_ready: &mut dyn FnMut()) -> i32 {
        // `open_window` only recorded the request; carry the builder into the
        // handler so it runs once the real window exists and its DPI is known.
        let Some(event_loop) = self.event_loop.take() else {
            return self.shared.exit_code.get();
        };
        app::run_with(event_loop, Rc::clone(&self.shared), window, on_ready);
        self.shared.exit_code.get()
    }

    fn quit(&self, code: i32) {
        self.shared.exit_code.set(code);
        self.shared.quit.set(true);
        let _ = self
            .shared
            .proxy
            .send_event(UserEvent::Wake(WindowId::from_raw(0)));
    }

    fn wake(&self, window: WindowId) {
        let _ = self.shared.proxy.send_event(UserEvent::Wake(window));
    }

    fn waker(&self, window: WindowId) -> Waker {
        let proxy = self.shared.proxy.clone();
        Box::new(move || {
            let _ = proxy.send_event(UserEvent::Wake(window));
        })
    }

    fn set_event_sink(&self, window: WindowId, sink: Rc<dyn WidgetHost>) {
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            state.sink = Some(sink);
        }
    }

    fn open_window(&self, spec: &PlatformSpec) -> BackendResult<WindowId> {
        let id = WindowId::from_raw(Self::allocate(&self.shared.next_window));
        let mut state = WindowState::new(spec);
        state.primary = self.shared.windows.borrow().is_empty();
        self.shared.windows.borrow_mut().insert(id.raw(), state);
        Ok(id)
    }

    fn close_window(&self, window: WindowId) {
        self.teardown_window_gl(window);
        self.shared.windows.borrow_mut().remove(&window.raw());
        self.shared.nodes.borrow_mut().remove_window(window);
        self.shared
            .timers
            .borrow_mut()
            .retain(|_, (owner, _, _)| *owner != window);
    }

    fn set_window_title(&self, window: WindowId, title: &str) {
        let title = title.to_string();
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            state.title = title.clone();
        }
        self.window_handle(window, |handle| handle.set_title(&title));
    }

    fn set_window_icon(&self, window: WindowId, icon: &Image) {
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            state.icon = Some(icon.clone());
        }
        if let Some(icon) = super::to_winit_icon(icon) {
            self.window_handle(window, |handle| handle.set_window_icon(Some(icon)));
        }
    }

    fn capture(&self, window: WindowId) -> BackendResult<Image> {
        let (width, height) = self
            .shared
            .windows
            .borrow()
            .get(&window.raw())
            .map(|state| state.size)
            .ok_or_else(|| BackendError::Other("no such window".into()))?;
        let mut surface = Surface::new(width.max(1), height.max(1));
        render::composite(&self.shared, window, &mut surface);
        let image = surface.to_image();
        Image::from_rgba(image.width, image.height, image.pixels)
            .map_err(|error| BackendError::Other(error.to_string()))
    }

    fn minimize(&self, window: WindowId) {
        self.window_handle(window, |handle| handle.set_minimized(true));
    }

    fn toggle_maximize(&self, window: WindowId) {
        self.window_handle(window, |handle| {
            handle.set_maximized(!handle.is_maximized());
        });
    }

    fn is_maximized(&self, window: WindowId) -> bool {
        self.window_handle(window, Window::is_maximized)
            .unwrap_or(false)
    }

    fn caption_inset(&self, window: WindowId) -> Dip {
        self.shared
            .windows
            .borrow()
            .get(&window.raw())
            .map_or(Dip(0.0), |state| state.caption_inset)
    }

    fn create(&self, parent: ParentRef, spec: &NodeSpec) -> BackendResult<WidgetId> {
        let window = match parent {
            ParentRef::Window(window) => window,
            ParentRef::Widget(widget) => self
                .window_of(widget)
                .ok_or(BackendError::CreateFailed("parent node"))?,
        };
        if !self.shared.windows.borrow().contains_key(&window.raw()) {
            return Err(BackendError::CreateFailed("window"));
        }
        let id = WidgetId::from_raw(Self::allocate(&self.shared.next_widget));
        let mut node = Node::new(window, parent, spec.bounds);
        node.visible = spec.visible;
        node.enabled = spec.enabled;
        node.text = spec.text.clone();
        node.popup = spec.popup;
        self.shared.nodes.borrow_mut().insert(id, node);
        Ok(id)
    }

    fn destroy(&self, id: WidgetId) {
        let window = self.window_of(id);
        self.shared.nodes.borrow_mut().remove(id);
        if let Some(window) = window {
            self.shared.request_redraw(window);
        }
    }

    fn apply_moves(&self, window: WindowId, moves: &[(WidgetId, Rect)]) {
        let mut resized = Vec::new();
        {
            let mut nodes = self.shared.nodes.borrow_mut();
            for (id, rect) in moves {
                if let Some(previous) = nodes.set_bounds(*id, *rect)
                    && previous.size() != rect.size()
                {
                    resized.push((*id, *rect));
                }
            }
        }
        self.shared.request_redraw(window);
        // A moved node gets no other size notification here (there is no HWND
        // to fire WM_SIZE on), so a resized widget must be told directly or its
        // own cached layout (e.g. a scrollbar's track) goes stale.
        for (id, rect) in resized {
            self.shared.deliver(
                window,
                id,
                &Event::Resize {
                    width: rect.width(),
                    height: rect.height(),
                },
            );
        }
    }

    fn set_visible(&self, id: WidgetId, visible: bool) {
        let changed = self.shared.nodes.borrow_mut().set_visible(id, visible);
        self.redraw_if(changed, id);
    }

    fn set_enabled(&self, id: WidgetId, enabled: bool) {
        self.with_node(id, |node| node.enabled = enabled);
    }

    fn raise(&self, id: WidgetId) {
        let moved = self.shared.nodes.borrow_mut().raise(id);
        self.redraw_if(moved, id);
    }

    fn set_drag_region(&self, id: WidgetId, drag: bool) {
        self.with_node(id, |node| node.drag_region = drag);
    }

    fn set_cursor(&self, id: WidgetId, cursor: Cursor) {
        self.shared.cursors.borrow_mut().insert(id.raw(), cursor);
    }

    fn set_clip(&self, id: WidgetId, rect: Option<Rect>) {
        let changed = self.shared.nodes.borrow_mut().set_clip(id, rect);
        self.redraw_if(changed, id);
    }

    fn set_capture(&self, id: WidgetId) {
        *self.shared.captured.borrow_mut() = Some(id);
    }

    fn release_capture(&self) {
        let previous = self.shared.captured.borrow_mut().take();
        if let Some(id) = previous
            && let Some(window) = self.window_of(id)
        {
            self.shared
                .deliver(window, id, &xui_core::backend::Event::CaptureChanged);
        }
    }

    fn focus(&self, id: WidgetId) {
        let window = self.window_of(id);
        let Some(window) = window else {
            return;
        };
        let previous = {
            let mut windows = self.shared.windows.borrow_mut();
            let Some(state) = windows.get_mut(&window.raw()) else {
                return;
            };
            state.focused.replace(id)
        };
        if let Some(previous) = previous
            && previous != id
        {
            self.shared
                .deliver(window, previous, &xui_core::backend::Event::KillFocus);
        }
        self.shared
            .deliver(window, id, &xui_core::backend::Event::SetFocus);
    }

    fn focus_holder(&self, window: WindowId) -> Option<WidgetId> {
        self.shared
            .windows
            .borrow()
            .get(&window.raw())
            .and_then(|state| state.focused)
    }

    fn set_text(&self, id: WidgetId, text: &str) {
        let changed = self.shared.nodes.borrow_mut().set_text(id, text);
        self.redraw_if(changed, id);
    }

    fn text(&self, id: WidgetId) -> String {
        self.shared
            .nodes
            .borrow()
            .get(id)
            .map(|node| node.text.clone())
            .unwrap_or_default()
    }

    fn clipboard_text(&self) -> Option<String> {
        crate::clipboard::get()
    }

    fn set_clipboard_text(&self, text: &str) {
        crate::clipboard::set(text);
    }

    fn bounds(&self, id: WidgetId) -> Rect {
        self.shared
            .nodes
            .borrow()
            .get(id)
            .map_or(Rect::default(), |node| node.bounds)
    }

    fn invalidate(&self, id: WidgetId) {
        let damaged = self.shared.nodes.borrow_mut().invalidate(id);
        self.redraw_if(damaged, id);
    }

    fn invalidate_rect(&self, id: WidgetId, rect: Rect) {
        let damaged = self.shared.nodes.borrow_mut().invalidate_rect(id, rect);
        self.redraw_if(damaged, id);
    }

    fn set_painter(&self, id: WidgetId, painter: Painter) {
        let changed = self.shared.nodes.borrow_mut().set_painter(id, painter);
        self.redraw_if(changed, id);
    }

    fn measure_text(&self, text: &str, style: &TextStyle, dpi: u32) -> TextMetrics {
        crate::text::measure(text, style, dpi, i32::MAX)
    }

    fn text_shaper(&self) -> Box<dyn TextShaper> {
        Box::new(self.text.get_or_init(CosmicShaper::new).clone())
    }

    fn layout_text(
        &self,
        text: &str,
        spec: &FontSpec,
        max_width: f32,
        dpi: u32,
    ) -> Box<dyn TextLayout> {
        self.text
            .get_or_init(CosmicShaper::new)
            .layout(text, spec, max_width, dpi)
    }

    fn dpi(&self, window: WindowId) -> u32 {
        self.shared
            .windows
            .borrow()
            .get(&window.raw())
            .map_or(DEFAULT_DPI, |state| state.dpi)
    }

    fn client_rect(&self, window: WindowId) -> Rect {
        self.shared
            .windows
            .borrow()
            .get(&window.raw())
            .map_or(Rect::default(), |state| {
                Rect::new(0, 0, state.size.0 as i32, state.size.1 as i32)
            })
    }

    fn set_theme(&self, window: WindowId, theme: &Theme) {
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            state.theme = *theme;
        }
        // Every painter reads the live theme.
        self.shared.nodes.borrow_mut().invalidate_window(window);
        self.shared.request_redraw(window);
    }

    fn prefers_reduced_motion(&self) -> bool {
        crate::sys::animation::reduced_motion()
    }

    fn set_timer(&self, window: WindowId, millis: u32) -> TimerId {
        let id = self.shared.next_timer.get();
        self.shared.next_timer.set(id + 1);
        let period = std::time::Duration::from_millis(u64::from(millis.max(1)));
        self.shared
            .timers
            .borrow_mut()
            .insert(id, (window, Instant::now() + period, period));
        TimerId(id)
    }

    fn kill_timer(&self, _window: WindowId, id: TimerId) {
        self.shared.timers.borrow_mut().remove(&id.0);
    }

    fn supports(&self, _kind: NodeKind) -> ImplKind {
        ImplKind::Painted
    }
}
