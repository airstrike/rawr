//! How the window's panes are drawn, dense as DevTools' panes: one text
//! size for every row, section bars over the panes, the split lines between
//! them, and the colours a value, a selected row and the marked line take.
//! Each colour comes from the window's theme, so light and dark both hold.
//! The window is dark in Oxocarbon and light in iced's light theme.

use iced::font;
use iced::widget::text::{Ellipsis, Wrapping};
use iced::widget::{button, container, row, space, text, text_input, tooltip};
use iced::{border, Background, Code, Color, Element, Fill, Font, Theme, Widget};
use iced_split::{Split, Style, StyleSheet};

/// The size of every row's text.
pub const SIZE: f32 = 12.0;

/// The size of a section bar's caption.
const CAPTION: f32 = 11.0;

/// The room a split's line takes between its panes.
const LINE: f32 = 1.0;

/// How far a split's grip reaches past its line into each pane.
const GRIP: f32 = 4.0;

/// The window's theme, light or dark as the system is.
#[must_use]
pub fn window(light: bool) -> Theme {
    if light {
        Theme::Light
    } else {
        Theme::Oxocarbon
    }
}

/// A split whose panes meet at its line: the line takes one pixel, and its
/// grip, unseen, reaches a few pixels into each pane.
pub fn split<'a, Message, Start, End>(
    split: Split<'a, Message, Start, End, Theme>,
) -> Split<'a, Message, Start, End, Theme>
where
    Message: 'a,
    Start: Widget<Message, Theme, iced::Renderer> + 'a,
    End: Widget<Message, Theme, iced::Renderer> + 'a,
{
    split
        .handle_width(LINE + 2.0 * GRIP)
        .spacing(-GRIP)
        .style(divider)
}

/// A pane's section bar: its caption, upper case, on a band of its own,
/// and what the pane shows, such as its file, when it says.
pub fn heading<'a, Message: 'a>(label: &str, detail: Option<&'a str>) -> Element<'a, Message> {
    let caption = text(label.to_uppercase())
        .size(CAPTION)
        .style(text::secondary);
    let bar = match detail {
        Some(detail) => row![
            caption,
            text(detail)
                .font(Font::MONOSPACE)
                .size(CAPTION)
                .wrapping(Wrapping::None)
                .ellipsis(Ellipsis::Start),
        ]
        .spacing(8)
        .boxed(),
        None => caption.boxed(),
    };
    container(bar)
        .width(Fill)
        .padding([2, 8])
        .style(band)
        .boxed()
}

/// A section bar's band: a shade off the background.
pub fn band(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().background.weak.color)
}

/// The pane a split holds: the window's background, so the split's line
/// alone tells the panes apart.
pub fn pane(theme: &Theme) -> container::Style {
    container::Style::default().background(theme.palette().background.base.color)
}

/// The line a split draws between its panes: a hairline at rest, the accent
/// while it is hovered or dragged.
fn divider(theme: &Theme) -> Style {
    Style {
        unfocused: StyleSheet {
            color: rule(theme),
            width: LINE,
            radius: 0.0.into(),
        },
        focused: StyleSheet {
            color: theme.palette().primary.base.color,
            width: 3.0,
            radius: 1.5.into(),
        },
        snap: true,
    }
}

/// The colour of a hairline: a split's line and the rule between the
/// console's entries.
pub fn rule(theme: &Theme) -> Color {
    theme.palette().background.strong.color
}

/// A hairline across its pane.
pub fn hairline<'a, Message: 'a>() -> Element<'a, Message> {
    container(space())
        .width(Fill)
        .height(1)
        .style(|theme| container::Style::default().background(rule(theme)))
        .boxed()
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
fn calm(color: Color, theme: &Theme) -> Color {
    color.mix(theme.palette().background.base.text, 0.25)
}

/// The source line where the program is.
pub fn marked(theme: &Theme) -> container::Style {
    container::Style::default().background(selection(theme))
}

/// A selected row's or the marked line's background: the accent, faint
/// enough that text in any colour reads on it.
fn selection(theme: &Theme) -> Color {
    theme.palette().primary.base.color.scale_alpha(0.22)
}

/// A row of a table: no chrome, a shade while hovered, and the accent's
/// weak shade when `selected`.
pub fn item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
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

/// `content` with `tip` shown over it while the pointer rests there.
pub fn tipped<'a, Message: 'a>(
    content: impl Widget<Message> + 'a,
    tip: impl Into<String>,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(tip.into()).size(SIZE)).padding([2, 6]),
        position,
    )
    .style(self::tip)
    .boxed()
}

/// A field's, an item's or a binding's name.
pub fn name(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().primary.base.color),
        ..text::Style::default()
    }
}

/// The colour of a value's text, as source colours the literal it starts
/// with: a string or a character, a number, or neither; or why it has
/// none, in the colour of an error.
pub fn value(value: Result<&str, &str>) -> impl Fn(&Theme) -> text::Style {
    let code = match value.map(|value| (value.chars().next(), value.len())) {
        Err(_) => Some(Code::Invalid),
        Ok((Some('"' | '\''), _)) => Some(Code::String),
        Ok((Some(first), length)) if first.is_ascii_digit() || (first == '-' && length > 1) => {
            Some(Code::Constant)
        }
        Ok(_) => None,
    };
    move |theme| text::Style {
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
