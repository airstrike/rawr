//! Rawr-authored color seeds lowered into native iced themes.

use std::fmt;
use std::str::FromStr;

/// Six source colors sufficient for iced to generate an extended palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    seed: iced::theme::palette::Seed,
}

/// One named palette color was not a valid iced hexadecimal color.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    role: &'static str,
    value: String,
}

impl Palette {
    /// Transform six hexadecimal source colors into an iced palette seed.
    pub fn from_hex(
        background: &str,
        text: &str,
        primary: &str,
        success: &str,
        warning: &str,
        danger: &str,
    ) -> Result<Self, Error> {
        Ok(Self {
            seed: iced::theme::palette::Seed {
                background: color("background", background)?,
                text: color("text", text)?,
                primary: color("primary", primary)?,
                success: color("success", success)?,
                warning: color("warning", warning)?,
                danger: color("danger", danger)?,
            },
        })
    }

    pub(crate) const fn seed(&self) -> iced::theme::palette::Seed {
        self.seed
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "theme palette {} color `{}` is not a valid hexadecimal iced color",
            self.role, self.value
        )
    }
}

impl std::error::Error for Error {}

pub(crate) fn color(role: &'static str, value: &str) -> Result<iced::Color, Error> {
    iced::Color::from_str(value).map_err(|_| Error {
        role,
        value: value.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::Palette;

    #[test]
    fn palette_reports_the_exact_invalid_color_role() {
        let error = Palette::from_hex(
            "#101010", "#f0f0f0", "not-blue", "#00ff00", "#ffff00", "#ff0000",
        )
        .unwrap_err();
        assert!(error.to_string().contains("primary"));
        assert!(error.to_string().contains("not-blue"));
    }
}
