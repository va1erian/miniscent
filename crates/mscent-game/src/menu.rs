//! Menu state (M6.6): which screen is up, which item is selected, and what
//! pressing a key means. Pure state; `hud.rs` draws it.

use crate::game::Phase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// No menu: the game is running.
    Hidden,
    Main,
    Paused,
    Won,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Up,
    Down,
    Select,
    Back,
}

/// What the host should do after a key press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    None,
    Start,
    Resume,
    Restart,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Menu {
    screen: Screen,
    selected: usize,
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu {
    /// Starts on the main menu.
    pub fn new() -> Self {
        Self {
            screen: Screen::Main,
            selected: 0,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn title(&self) -> &'static str {
        match self.screen {
            Screen::Hidden => "",
            Screen::Main => "MINISCENT",
            Screen::Paused => "PAUSED",
            Screen::Won => "LEVEL COMPLETE",
            Screen::Lost => "YOU DIED",
        }
    }

    /// Labels of the current screen, top to bottom.
    pub fn items(&self) -> &'static [&'static str] {
        match self.screen {
            Screen::Hidden => &[],
            Screen::Main => &["Start", "Quit"],
            Screen::Paused => &["Resume", "Restart", "Quit"],
            Screen::Won | Screen::Lost => &["Restart", "Quit"],
        }
    }

    /// Opens the end-of-level screen when the game stops being `Playing`.
    pub fn sync_phase(&mut self, phase: Phase) {
        let target = match phase {
            Phase::Won => Screen::Won,
            Phase::Lost => Screen::Lost,
            Phase::Playing => return,
        };
        if self.screen != target {
            self.show(target);
        }
    }

    pub fn press(&mut self, cmd: MenuCommand) -> MenuAction {
        let n = self.items().len();
        match cmd {
            MenuCommand::Up if n > 0 => {
                self.selected = (self.selected + n - 1) % n;
                MenuAction::None
            }
            MenuCommand::Down if n > 0 => {
                self.selected = (self.selected + 1) % n;
                MenuAction::None
            }
            MenuCommand::Select => self.select(),
            MenuCommand::Back => self.back(),
            _ => MenuAction::None,
        }
    }

    fn select(&mut self) -> MenuAction {
        let Some(label) = self.items().get(self.selected) else {
            return MenuAction::None;
        };
        let action = match *label {
            "Start" => MenuAction::Start,
            "Resume" => MenuAction::Resume,
            "Restart" => MenuAction::Restart,
            _ => MenuAction::Quit,
        };
        if action != MenuAction::Quit {
            self.show(Screen::Hidden);
        }
        action
    }

    /// Escape: pauses a running game, resumes a paused one, and does nothing
    /// on screens where there is nothing to go back to.
    fn back(&mut self) -> MenuAction {
        match self.screen {
            Screen::Hidden => {
                self.show(Screen::Paused);
                MenuAction::None
            }
            Screen::Paused => {
                self.show(Screen::Hidden);
                MenuAction::Resume
            }
            _ => MenuAction::None,
        }
    }

    fn show(&mut self, screen: Screen) {
        self.screen = screen;
        self.selected = 0;
    }
}
