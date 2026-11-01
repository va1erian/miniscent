//! The offscreen backend reports no reduced-motion preference unless a test
//! scripts one, so a headless render does not depend on the accessibility
//! setting of the machine it runs on.

use super::*;

struct Idle;

impl App for Idle {
    type Msg = ();
    fn update(&mut self, _msg: (), _ui: &mut Ui<()>) {}
}

/// What an app sees through `Ui` when it is built on `backend`.
fn seen_through_ui(backend: &Rc<OffscreenBackend>) -> bool {
    let backend_for_run: Rc<dyn Backend> = backend.clone();
    let mut seen = None;
    let result = run_app(backend_for_run, PlatformSpec::new("motion"), |ui| {
        seen = Some(ui.prefers_reduced_motion());
        Idle
    });
    assert!(result.is_ok(), "the app ran");
    seen.expect("the app was built")
}

#[test]
fn the_offscreen_backend_reports_no_reduced_motion_by_default() {
    assert!(!OffscreenBackend::new().prefers_reduced_motion());
    assert!(!OffscreenBackend::with_dpi(144).prefers_reduced_motion());
}

#[test]
fn an_app_on_the_offscreen_backend_sees_no_reduced_motion_by_default() {
    assert!(!seen_through_ui(&Rc::new(OffscreenBackend::new())));
}

#[test]
fn a_scripted_preference_is_seen_through_the_ui() {
    let backend = Rc::new(OffscreenBackend::new());
    backend.set_prefers_reduced_motion(true);
    assert!(backend.prefers_reduced_motion());
    assert!(seen_through_ui(&backend));

    backend.set_prefers_reduced_motion(false);
    assert!(!seen_through_ui(&backend), "the setting is not sticky");
}
