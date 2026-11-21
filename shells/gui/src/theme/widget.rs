//! The window's own small widgets: a pane's bar, a hairline, a status's
//! lamp, a split's line and a tooltip.

use iced::font;
use iced::widget::{column, container, row, scrollable, space, text, tooltip};
use iced::{border, Element, Fill, Font, Theme, Widget};
use iced_split::{Split, Style, StyleSheet};

use super::colour::calm;
use super::{faint, quiet, rule, tip, CAPTION, SIZE};

/// The height of a pane's bar.
const BAR: f32 = 24.0;

/// The room a split's line takes between its panes.
const LINE: f32 = 1.0;

/// How far a split's grip reaches past its line into each pane.
const GRIP: f32 = 4.0;

/// The font of a word that leads: the phase in the toolbar.
pub const STRONG: Font = Font {
    weight: font::Weight::Semibold,
    ..Font::DEFAULT
};

/// A status's tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// What asks to be looked at: a pause.
    Accent,
    Good,
    Quiet,
    Bad,
}

/// A status's lamp: a dot in its tone.
pub fn lamp<'a, Message: 'a>(tone: Tone) -> Element<'a, Message> {
    container(space())
        .width(7)
        .height(7)
        .style(move |theme: &Theme| {
            let palette = theme.palette();
            let color = match tone {
                Tone::Accent => palette.primary.base.color,
                Tone::Good => calm(palette.success.base.color, theme),
                Tone::Quiet => quiet(theme),
                Tone::Bad => calm(palette.danger.base.color, theme),
            };
            container::Style::default()
                .background(color)
                .border(border::rounded(3.5))
        })
        .boxed()
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

/// A pane's bar: its caption, upper case and quiet, then what the pane
/// says of what it shows, over a hairline.
pub fn bar<'a, Message: 'a>(label: &str, detail: Element<'a, Message>) -> Element<'a, Message> {
    let caption = text(label.to_uppercase()).size(CAPTION).style(faint);
    column![
        container(row![caption, detail].spacing(10).align_y(iced::Center))
            .width(Fill)
            .height(BAR)
            .padding([0, 10])
            .align_y(iced::Center),
        hairline(),
    ]
    .boxed()
}

/// A pane's bar with its caption alone.
pub fn heading<'a, Message: 'a>(label: &str) -> Element<'a, Message> {
    bar(label, space().boxed())
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

/// A scrollable's bar: thin, as an editor's.
#[must_use]
pub fn scrollbar() -> scrollable::Direction {
    scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6))
}

/// A hairline across its pane.
pub fn hairline<'a, Message: 'a>() -> Element<'a, Message> {
    container(space())
        .width(Fill)
        .height(1)
        .style(|theme| container::Style::default().background(rule(theme)))
        .boxed()
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
