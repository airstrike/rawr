// rawr
//! The app's theme, marked, over the native `themedef::Theme` of its
//! child module, as inboard's `theme.rs`.
mod themedef;

pub use themedef::Subpalette;

#[derive(Debug, Clone, PartialEq)]
pub enum Theme {
    Selected(themedef::Theme),
}

impl Theme {
    pub fn colors(&self) -> &themedef::Colors {
        match self {
            Theme::Selected(theme) => &theme.colors,
        }
    }

    /// The default theme with its accent moved into from its error, a
    /// field of a field of an owned local (#1893).
    pub fn alarmed() -> Self {
        let mut theme = themedef::Theme::default();
        theme.colors.accent = theme.colors.error;
        theme.colors.label = theme.name.clone();
        Self::from(theme)
    }

    /// The default theme's error and colors, each read through a `&self`
    /// method of a field path, which only borrows it (#1894).
    #[allow(clippy::clone_on_copy)]
    pub fn split() -> (Subpalette, themedef::Colors) {
        let theme = themedef::Theme::default();
        let accent = theme.colors.error.clone();
        let base = theme.colors.clone();
        (accent, base)
    }

    /// The default theme with its accent from its error, made by
    /// struct-update syntax over native types (#1895).
    #[allow(clippy::clone_on_copy)]
    pub fn updated() -> Self {
        let theme = themedef::Theme::default();
        let colors = themedef::Colors {
            accent: theme.colors.error.clone(),
            ..theme.colors.clone()
        };
        Self::from(themedef::Theme { colors, ..theme })
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from(themedef::Theme::default())
    }
}

impl From<themedef::Theme> for Theme {
    fn from(theme: themedef::Theme) -> Self {
        Theme::Selected(theme)
    }
}

impl ui::Catalog for Theme {
    fn ink(&self) -> String {
        format!("ink {}", self.colors().text.s600)
    }
}
