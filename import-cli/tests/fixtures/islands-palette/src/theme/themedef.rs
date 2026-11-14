//! Native: the theme's definition, as inboard's `theme/themedef.rs`. A
//! theme owns its name and its colors, which are not `Copy`; each of the
//! colors' subpalettes is.

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
    pub palette: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "light".to_owned(),
            colors: Colors::default(),
            palette: Palette { contrast: 7 },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub accent: Subpalette,
    pub error: Subpalette,
    pub text: Subpalette,
    pub label: String,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            accent: Subpalette { s600: 1 },
            error: Subpalette { s600: 2 },
            text: Subpalette { s600: 3 },
            label: "tailwind".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Subpalette {
    pub s600: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub contrast: u32,
}
