//! The toolbar's controls, each with its icon, its label and its key, as
//! DevTools binds them: F8 continues a paused program or pauses a running
//! one, F10 steps over, F11 into and Shift+F11 out.

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::Text;

use crate::icon;
use crate::session::Phase;

/// One of the toolbar's controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Continue,
    Pause,
    Step(Step),
    /// Ask the running program to draw its window again.
    Refresh,
}

/// How a step goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Over,
    Into,
    Out,
}

/// A control's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    F8,
    F10,
    F11,
    ShiftF11,
}

impl Control {
    /// The toolbar's controls, in its order.
    pub const ALL: [Self; 6] = [
        Self::Continue,
        Self::Pause,
        Self::Step(Step::Over),
        Self::Step(Step::Into),
        Self::Step(Step::Out),
        Self::Refresh,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Continue => "Continue",
            Self::Pause => "Pause",
            Self::Step(Step::Over) => "Step over",
            Self::Step(Step::Into) => "Step into",
            Self::Step(Step::Out) => "Step out",
            Self::Refresh => "Refresh frame",
        }
    }

    /// The icon's name among the window's icons (fonts/icons.toml).
    const fn icon_name(self) -> &'static str {
        match self {
            Self::Continue => "resume",
            Self::Pause => "pause",
            Self::Step(Step::Over) => "step_over",
            Self::Step(Step::Into) => "step_into",
            Self::Step(Step::Out) => "step_out",
            Self::Refresh => "refresh",
        }
    }

    /// The icon's glyph in the icons' font: the text a click on the
    /// control finds.
    #[must_use]
    pub fn glyph(self) -> &'static str {
        icon::ALL_ICONS
            .iter()
            .find(|(name, _)| *name == self.icon_name())
            .map_or("", |(_, glyph)| glyph)
    }

    /// The control's icon.
    pub fn icon<'a>(self) -> Text<'a> {
        icon::render(self.glyph())
    }

    #[must_use]
    pub const fn shortcut(self) -> Option<Shortcut> {
        match self {
            Self::Continue | Self::Pause => Some(Shortcut::F8),
            Self::Step(Step::Over) => Some(Shortcut::F10),
            Self::Step(Step::Into) => Some(Shortcut::F11),
            Self::Step(Step::Out) => Some(Shortcut::ShiftF11),
            Self::Refresh => None,
        }
    }

    /// Whether the program, at `phase`, has anything for it to act on.
    #[must_use]
    pub fn enabled(self, phase: &Phase) -> bool {
        match self {
            Self::Continue | Self::Step(_) => phase.stop().is_some(),
            Self::Pause | Self::Refresh => *phase == Phase::Running,
        }
    }

    /// The control `shortcut` acts as at `phase`: of the two on F8, the
    /// one with something to act on.
    #[must_use]
    pub fn keyed(shortcut: Shortcut, phase: &Phase) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|control| control.shortcut() == Some(shortcut) && control.enabled(phase))
    }

    /// The tooltip: the label, and the key when it has one.
    #[must_use]
    pub fn tip(self) -> String {
        match self.shortcut() {
            Some(shortcut) => format!("{} ({})", self.label(), shortcut.name()),
            None => self.label().to_owned(),
        }
    }
}

impl Step {
    /// The step's kind, as the debugger names it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Over => "over",
            Self::Into => "into",
            Self::Out => "out",
        }
    }
}

impl Shortcut {
    /// The shortcut a key press is, if any.
    #[must_use]
    pub fn of(event: keyboard::Event) -> Option<Self> {
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return None;
        };
        match (key, modifiers.shift()) {
            (Key::Named(Named::F8), false) => Some(Self::F8),
            (Key::Named(Named::F10), false) => Some(Self::F10),
            (Key::Named(Named::F11), false) => Some(Self::F11),
            (Key::Named(Named::F11), true) => Some(Self::ShiftF11),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::F8 => "F8",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::ShiftF11 => "Shift+F11",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Control, Shortcut, Step};
    use crate::session::{Phase, Stop};

    #[test]
    fn f8_continues_a_paused_program_and_pauses_a_running_one() {
        let paused = Phase::Paused(Stop {
            file: None,
            line: Some(5),
            function: None,
            reason: "breakpoint".to_owned(),
        });
        assert_eq!(
            Control::keyed(Shortcut::F8, &paused),
            Some(Control::Continue)
        );
        assert_eq!(
            Control::keyed(Shortcut::F8, &Phase::Running),
            Some(Control::Pause)
        );
        assert_eq!(
            Control::keyed(Shortcut::ShiftF11, &paused),
            Some(Control::Step(Step::Out))
        );
        assert_eq!(
            Control::keyed(Shortcut::F10, &Phase::Running),
            None,
            "a running program has no step to take"
        );
        assert_eq!(Control::Step(Step::Out).tip(), "Step out (Shift+F11)");
    }
}
