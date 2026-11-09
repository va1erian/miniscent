//! View > Theme: which xui theme the window wears, and whether its accent is the
//! system's. The window starts on xui's own pick (the LazyOS pair, light or dark
//! after the desktop) with the system accent on.

use xui::Color;
use xui::prelude::Ui;
use xui::theme::Theme;
use xui::widget::{MenuId, MenuScope};

use super::app::{Editor, Msg};

/// Menu ids for the Theme submenu (the View menu's own is 102).
pub(super) mod id {
    use xui::widget::MenuId;
    pub const SUBMENU: MenuId = MenuId::new(140);
    pub const SYSTEM_ACCENT: MenuId = MenuId::new(141);
}

/// The themes on offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Look {
    /// What xui chose at start-up: LazyOS Daylight or Nightfall after the desktop.
    Default,
    Daylight,
    Nightfall,
    Midnight,
    WindowsLight,
    WindowsDark,
}

/// Every look with its menu id and label, in menu order.
const LOOKS: [(Look, MenuId, &str); 6] = [
    (
        Look::Default,
        MenuId::new(142),
        "&Default (follows the desktop)",
    ),
    (Look::Daylight, MenuId::new(143), "LazyOS &Daylight"),
    (Look::Nightfall, MenuId::new(144), "LazyOS &Nightfall"),
    (Look::Midnight, MenuId::new(145), "LazyOS &Midnight"),
    (Look::WindowsLight, MenuId::new(146), "Windows &light"),
    (Look::WindowsDark, MenuId::new(147), "Windows d&ark"),
];

/// The Theme submenu, appended to the View menu.
pub(super) fn menu(v: &mut MenuScope<'_>) {
    v.separator();
    v.submenu(id::SUBMENU, "&Theme", |t| {
        for (look, mid, label) in LOOKS {
            t.radio(mid, label, look == Look::Default);
        }
        t.separator();
        t.check(id::SYSTEM_ACCENT, "Use the system &accent colour", true);
    });
}

/// The window's theme choice.
pub(super) struct Appearance {
    pub(super) look: Look,
    /// The theme the window opened with, which `Look::Default` returns to.
    base: Theme,
    /// The desktop's accent, once asked for. `None` keeps each theme's own.
    system: Option<Color>,
    pub(super) use_system_accent: bool,
}

impl Appearance {
    pub(super) fn new(base: Theme) -> Appearance {
        Appearance {
            look: Look::Default,
            base,
            system: None,
            use_system_accent: true,
        }
    }

    /// The theme to wear now.
    fn theme(&self) -> Theme {
        let theme = match self.look {
            Look::Default => self.base,
            Look::Daylight => Theme::daylight(),
            Look::Nightfall => Theme::nightfall(),
            Look::Midnight => Theme::midnight(),
            Look::WindowsLight => Theme::light(),
            Look::WindowsDark => Theme::dark(),
        };
        match self.system.filter(|_| self.use_system_accent) {
            Some(accent) => with_accent(theme, accent),
            None => theme,
        }
    }
}

/// `theme` with `accent` in place of its own, and ink on it that can be read.
fn with_accent(mut theme: Theme, accent: Color) -> Theme {
    theme.accent = accent;
    theme.border_focused = accent;
    let (light, dark) = (Color::rgb(255, 255, 255), Color::rgb(0, 0, 0));
    theme.text_on_accent = if light.contrast_ratio(accent) >= dark.contrast_ratio(accent) {
        light
    } else {
        dark
    };
    theme
}

/// The desktop's accent colour, or `None` where there is no such thing or it
/// cannot be read. Asked of the registry through `reg`, as xui has no hook for it
/// and the editor keeps `unsafe` out of its own code.
#[cfg(windows)]
pub(super) fn system_accent() -> Option<Color> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\DWM",
            "/v",
            "AccentColor",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_accent(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(windows))]
pub(super) fn system_accent() -> Option<Color> {
    None
}

/// The colour in `reg query` output: `AccentColor  REG_DWORD  0xAABBGGRR`.
fn parse_accent(text: &str) -> Option<Color> {
    let line = text.lines().find(|l| l.contains("AccentColor"))?;
    let hex = line.split_whitespace().last()?.strip_prefix("0x")?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8))
}

impl Editor {
    /// Reads the desktop's accent and puts it on the window. Called once, by the
    /// real window; scripted runs keep each theme's own accent so their pictures
    /// do not depend on the machine.
    pub fn use_system_accent(&mut self, ui: &Ui<Msg>) {
        self.appearance.system = system_accent();
        self.apply_appearance(ui);
    }

    /// Handles a Theme menu command; `false` for any other.
    pub(super) fn appearance_command(&mut self, cmd: MenuId, ui: &Ui<Msg>) -> bool {
        if cmd == id::SYSTEM_ACCENT {
            self.appearance.use_system_accent = !self.appearance.use_system_accent;
        } else if let Some((look, _, _)) = LOOKS.iter().find(|(_, mid, _)| *mid == cmd) {
            self.appearance.look = *look;
        } else {
            return false;
        }
        self.apply_appearance(ui);
        true
    }

    fn apply_appearance(&self, ui: &Ui<Msg>) {
        let menu = self.menu.get();
        for (look, mid, _) in LOOKS {
            menu.set_checked(mid, look == self.appearance.look);
        }
        menu.set_checked(id::SYSTEM_ACCENT, self.appearance.use_system_accent);
        ui.set_theme(self.appearance.theme());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_registry_dword_as_abgr() {
        let out = "\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM\n    AccentColor    REG_DWORD    0xffd77800\n\n";
        assert_eq!(parse_accent(out), Some(Color::rgb(0x00, 0x78, 0xd7)));
    }

    #[test]
    fn refuses_what_is_not_a_colour() {
        assert_eq!(parse_accent(""), None);
        assert_eq!(parse_accent("AccentColor REG_DWORD zzz"), None);
        assert_eq!(
            parse_accent("ERROR: The system was unable to find it."),
            None
        );
    }

    #[test]
    fn accent_replaces_the_themes_and_keeps_its_ink_readable() {
        for accent in [Color::rgb(255, 235, 59), Color::rgb(0, 40, 120)] {
            let t = with_accent(Theme::nightfall(), accent);
            assert_eq!((t.accent, t.border_focused), (accent, accent));
            assert!(t.text_on_accent.contrast_ratio(accent) >= 4.5);
        }
    }

    #[test]
    fn default_look_is_the_opening_theme() {
        let base = Theme::nightfall();
        let mut a = Appearance::new(base);
        a.use_system_accent = false;
        assert_eq!(a.theme(), base);
        a.look = Look::WindowsLight;
        assert_eq!(a.theme(), Theme::light());
    }
}
