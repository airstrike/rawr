// rawr
//! The app's theme, marked, as inboard's `theme.rs`: it declares native
//! child modules, wraps one's theme in its own enum, converts to and from
//! that native theme and the crate's, and implements the crate's traits.
pub mod catalog;
mod palette;

pub use palette::Hue;

#[derive(Debug, Clone, PartialEq)]
pub enum Theme {
    Selected(palette::Theme),
}

impl Theme {
    pub fn accent(&self) -> u32 {
        match self {
            Theme::Selected(theme) => theme.colors.accent,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from(palette::Theme::default())
    }
}

impl From<palette::Theme> for Theme {
    fn from(theme: palette::Theme) -> Self {
        Theme::Selected(theme)
    }
}

impl From<Theme> for ui::Theme {
    fn from(val: Theme) -> Self {
        match val {
            Theme::Selected(theme) => ui::Theme::custom(theme.name, theme.colors.accent),
        }
    }
}

impl From<ui::Theme> for Theme {
    fn from(val: ui::Theme) -> Self {
        let mut theme = palette::Theme::default();
        theme.name = format!("{} again", val.name);
        Theme::from(theme)
    }
}

impl ui::Design for Theme {
    fn background(&self) -> u32 {
        self.accent() + 1
    }

    fn name(&self) -> String {
        match self {
            Theme::Selected(theme) => theme.name.clone(),
        }
    }

    fn corners(&self) -> f32 {
        4.0
    }
}

pub fn dark() -> Theme {
    Theme::from(palette::Theme::dark())
}

impl From<&Theme> for ui::Settings {
    fn from(theme: &Theme) -> Self {
        let mut settings = Self::with_text_size(14.0);
        settings.code_size = theme.accent() as f32 / 10.0;
        settings
    }
}
