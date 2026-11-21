//! The window's colours, each from its theme's palette: a value's, a
//! selected row's, the marked line's, the code's, and each pane's.

use iced::font;
use iced::widget::{button, container, text, text_editor, text_input};
use iced::{border, Background, Code, Color, Theme};

/// The window's theme, light or dark as the system is.
#[must_use]
pub fn window(light: bool) -> Theme {
    if light {
        Theme::Light
    } else {
        Theme::Oxocarbon
    }
}

/// The pane a split holds: the window's background, so the split's line
/// alone tells the panes apart.
pub fn pane(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().background.base.color)
}

/// A pane beside the source, and the toolbar: a shade under the source's
/// background, so the source reads as the window's surface.
pub fn side(theme: &Theme) -> container::Style {
    container::Style::default().background(iced::theme::palette::darken(
        theme.palette().background.base.color,
        0.025,
    ))
}

/// The colour of a hairline: a split's line and the rule between the
/// console's entries.
pub fn rule(theme: &Theme) -> Color {
    theme.palette().background.strong.color
}

/// A text input with no box: its text alone, on the pane's background.
pub fn bare(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: border::width(0),
        placeholder: quiet(theme),
        value: palette.background.base.text,
        selection: palette.primary.weak.color,
    }
}

/// The colour of quiet text: a console's marks, an answer and a gutter's
/// numbers.
pub fn quiet(theme: &Theme) -> Color {
    theme.palette().secondary.base.color
}

/// Quiet text.
pub fn faint(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(quiet(theme)),
        ..text::Style::default()
    }
}

/// The console's prompt mark, in the accent: it says where typing goes.
pub fn mark(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().primary.base.color),
        ..text::Style::default()
    }
}

/// The breakpoint dot in the gutter.
pub fn breakpoint(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(calm(theme.palette().danger.base.color, theme)),
        ..text::Style::default()
    }
}

/// How code is drawn: each kind in a hue of the theme's palette, softened
/// toward its text so no hue shouts. Names and anything plain take the
/// text's own colour.
#[must_use]
pub fn code(code: Code, theme: &Theme) -> text::highlighter::Style {
    let palette = theme.palette();
    let color = match code {
        Code::Keyword | Code::Support => Some(calm(palette.warning.base.color, theme)),
        Code::Function => Some(calm(palette.primary.base.color, theme)),
        Code::Type | Code::Path => Some(
            palette
                .primary
                .base
                .color
                .mix(palette.background.base.text, 0.5),
        ),
        Code::String => Some(calm(palette.success.base.color, theme)),
        Code::Constant => Some(
            palette
                .danger
                .base
                .color
                .mix(palette.background.base.text, 0.45),
        ),
        Code::Punctuation | Code::Comment => Some(quiet(theme)),
        Code::Invalid => Some(palette.danger.base.color),
        Code::Variable | Code::Other => None,
    };
    text::highlighter::Style {
        color,
        style: (code == Code::Comment).then_some(font::Style::Italic),
    }
}

/// `color` a quarter of the way to the theme's text, so it reads calm.
pub(super) fn calm(color: Color, theme: &Theme) -> Color {
    color.mix(theme.palette().background.base.text, 0.25)
}

/// The band under the source line where the program is: the accent,
/// faint.
pub fn marked(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().primary.base.color.scale_alpha(0.16))
}

/// The marked line's edge, at the gutter's left: the accent, solid.
pub fn edge(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().primary.base.color)
}

/// The band under the line a failed save names.
pub fn failed(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().danger.base.color.scale_alpha(0.12))
}

/// The source's editor: the code alone on the pane, with the accent's
/// faint shade under a selection.
pub fn editor(theme: &Theme, _status: text_editor::Status) -> text_editor::Style {
    let palette = theme.palette();
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: border::width(0),
        placeholder: quiet(theme),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.3),
    }
}

/// A value shown at the end of a line of source: quiet, so the code reads
/// first.
pub fn inline(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(quiet(theme).scale_alpha(0.9)),
        ..text::Style::default()
    }
}

/// What went well: a save swapped in.
pub fn done(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(calm(theme.palette().success.base.color, theme)),
        ..text::Style::default()
    }
}

/// A selected row's or the marked line's background: the accent, faint
/// enough that text in any colour reads on it.
fn selection(theme: &Theme) -> Color {
    theme.palette().primary.base.color.scale_alpha(0.22)
}

/// A row of a table: no chrome, a shade while hovered, and the accent's
/// weak shade when `selected`.
pub fn item(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let palette = theme.palette();
    let background = match (selected, status) {
        (true, _) => Some(selection(theme)),
        (false, button::Status::Hovered) => Some(palette.background.weak.color),
        (false, _) => None,
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: palette.background.base.text,
        ..button::Style::default()
    }
}

/// A toolbar button: its icon alone at rest, a shade under it while
/// hovered, faded while it has nothing to act on.
pub fn control(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let base = button::Style {
        text_color: palette.background.base.text,
        border: border::rounded(3),
        ..button::Style::default()
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => {
            base.with_background(palette.background.weak.color)
        }
        button::Status::Disabled => button::Style {
            text_color: palette.background.base.text.scale_alpha(0.35),
            ..base
        },
        button::Status::Active => base,
    }
}

/// A tooltip's box.
pub fn tip(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style::default()
        .background(palette.background.weakest.color)
        .color(palette.background.base.text)
        .border(
            border::rounded(3)
                .width(1)
                .color(palette.background.strong.color),
        )
}

/// A field's, an item's or a binding's name.
pub fn name(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(calm(theme.palette().primary.base.color, theme)),
        ..text::Style::default()
    }
}

/// The colour of a value's text, as source colours the literal it starts
/// with: a string or a character, a number, or neither; or why it has
/// none, in the colour of an error.
pub fn value(theme: &Theme, value: Result<&str, &str>) -> text::Style {
    let code = match value.map(|value| (value.chars().next(), value.len())) {
        Err(_) => Some(Code::Invalid),
        Ok((Some('"' | '\''), _)) => Some(Code::String),
        Ok((Some(first), length)) if first.is_ascii_digit() || (first == '-' && length > 1) => {
            Some(Code::Constant)
        }
        Ok(_) => None,
    };
    text::Style {
        color: code.and_then(|code| self::code(code, theme).color),
        ..text::Style::default()
    }
}

/// Why a value has none to show.
pub fn error(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().danger.base.color),
        ..text::Style::default()
    }
}
