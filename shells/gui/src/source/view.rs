//! How the source is drawn: its bar, with the file, whether it holds
//! edits and what the last save did; the gutter; and the editor over the
//! marked line's band, with the values near the pause at its lines' ends.

use std::path::Path;
use std::sync::LazyLock;

use iced::advanced::graphics::text::Paragraph;
use iced::advanced::text::{self as core_text, Paragraph as _};
use iced::alignment::Vertical;
use iced::widget::text::{Ellipsis, LineHeight, Shaping, Wrapping};
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, stack, text, text_editor,
    tooltip, Id,
};
use iced::{Color, Element, Fill, Font, Pixels, Point, Size, Widget};

use super::hover::{Hover, Said};
use super::inline;
use super::{Message, Reload, State};
use crate::theme;

/// The height of a line of source, in the gutter, the editor and the
/// values over it alike.
pub const LINE: f32 = 16.0;

/// The room left of the code.
const PAD: f32 = 12.0;

/// How a line's end is drawn: a value's quiet colour or an error's.
type Paint = fn(&iced::Theme) -> text::Style;

/// The width of the code's monospace characters.
static ADVANCE: LazyLock<f32> = LazyLock::new(|| {
    let sample = "0".repeat(64);
    let paragraph = Paragraph::with_text(core_text::Text {
        content: sample.as_str(),
        bounds: Size::INFINITE,
        size: Pixels(theme::SIZE),
        line_height: line_height(),
        font: Font::MONOSPACE,
        align_x: core_text::Alignment::Default,
        align_y: Vertical::Top,
        shaping: Shaping::Basic,
        wrapping: Wrapping::None,
        ellipsis: Ellipsis::None,
        hint_factor: None,
    });
    paragraph.min_width() / 64.0
});

/// The scrollable the source scrolls in.
pub fn scroller() -> Id {
    Id::new("source")
}

/// The line and column, from 0, at `point` in the code.
#[must_use]
pub fn under(point: Point) -> Option<(usize, usize)> {
    let x = point.x - PAD;
    if x < 0.0 || point.y < 0.0 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both are positive, and far under usize's range"
    )]
    Some(((point.y / LINE) as usize, (x / *ADVANCE) as usize))
}

fn line_height() -> LineHeight {
    LineHeight::Absolute(Pixels(LINE))
}

impl State {
    /// The source's bar over the code: the file, whether its buffer holds
    /// edits, and what the last save did.
    pub fn heading(&self) -> Element<'_, Message> {
        let file = self
            .file
            .as_deref()
            .map(super::short_path)
            .unwrap_or_default();
        let edited = if self.edited() {
            text("edited").size(theme::CAPTION).style(theme::faint)
        } else {
            text("")
        };
        let status: Element<'_, Message> = match &self.reload {
            None => space().boxed(),
            Some(reload) => self.status(reload),
        };
        theme::bar(
            "Source",
            row![
                text(file).font(Font::MONOSPACE).size(theme::CAPTION),
                edited,
                space().width(Fill),
                status,
            ]
            .spacing(8)
            .align_y(iced::Center)
            .boxed(),
        )
    }

    /// What the last save did, as the bar tells it.
    fn status<'a>(&'a self, reload: &'a Reload) -> Element<'a, Message> {
        let said = |words: &'a str| text(words).size(theme::CAPTION);
        match reload {
            Reload::Saving => said("saved").style(theme::faint).boxed(),
            Reload::Preparing => said("building the save…").style(theme::faint).boxed(),
            Reload::Queued => said("queued for the next call").style(theme::faint).boxed(),
            Reload::Swapped => said("swapped in, state kept").style(theme::done).boxed(),
            Reload::Restarted => said("restarted").style(theme::faint).boxed(),
            Reload::Failed(failure) => {
                let at = self.file.as_deref().and_then(|file| reload.failed_at(file));
                let told = match (at, &failure.file, failure.line) {
                    (Some((line, _)), _, _) => format!("error at line {line}"),
                    (None, Some(file), Some(line)) => {
                        format!("error in {}:{line}", super::short_path(file))
                    }
                    _ => failure.message.clone(),
                };
                text(told)
                    .size(theme::CAPTION)
                    .wrapping(Wrapping::None)
                    .ellipsis(Ellipsis::End)
                    .style(theme::error)
                    .boxed()
            }
        }
    }

    /// The code with its gutter, under the bar: a click in the gutter sets
    /// or clears a breakpoint at each line in `breakpoints`. `values` are
    /// the bindings of the frame paused in this file, by name, to show at
    /// the ends of the lines near the marked one.
    pub fn view<'a>(
        &'a self,
        breakpoints: &[u64],
        values: &[(&'a str, &'a str)],
    ) -> Element<'a, Message> {
        let Some(file) = &self.file else {
            return note("No source yet: the program's file shows once it starts.");
        };
        if let Some(error) = &self.unread {
            return container(
                text(format!("{}: {error}", crate::home::shorten(file)))
                    .size(theme::SIZE)
                    .style(theme::error),
            )
            .padding(8)
            .boxed();
        }
        let Some(buffer) = self.buffer() else {
            return note("Reading…");
        };
        let lines = buffer.lines();
        let failed = self
            .reload
            .as_ref()
            .and_then(|reload| reload.failed_at(file));
        let gutter = column((1..=lines as u64).map(|number| {
            gutter(
                number,
                breakpoints.contains(&number),
                self.marked == Some(number),
                failed.is_some_and(|(line, _)| line == number),
            )
        }));
        let token = Path::new(file)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("txt")
            .to_owned();
        let editor = text_editor(buffer.content())
            .highlight_with::<iced::highlighter::Parser>(
                iced::highlighter::Settings { token },
                theme::code,
            )
            .on_action(Message::Edit)
            .font(Font::MONOSPACE)
            .size(theme::SIZE)
            .line_height(line_height())
            .padding([0.0, PAD])
            .wrapping(Wrapping::None)
            .key_binding(|press| {
                if super::saves(&press.key, press.modifiers) {
                    return Some(text_editor::Binding::Custom(Message::Save));
                }
                text_editor::Binding::from_key_press(press)
            })
            .style(theme::editor);
        let pointed = mouse_area(editor)
            .on_move(Message::Point)
            .on_exit(Message::Leave);
        let hovered = tooltip(pointed, self.hover(), tooltip::Position::FollowCursor)
            .gap(6)
            .style(|_| container::Style::default());
        let code = stack![hovered].push_under(self.inline(values, failed));
        let height = lines as f32 * LINE;
        let bands = self.bands(failed.map(|(line, _)| line), height);
        scrollable(stack![row![gutter, code]].push_under(bands))
            .id(scroller())
            .on_scroll(|scroll| Message::Scrolled(scroll.destination()))
            .direction(theme::scrollbar())
            .height(Fill)
            .width(Fill)
            .boxed()
    }

    /// The band under the marked line, and under the line a failed save
    /// names, in a layer as tall as the code.
    fn bands(&self, failed: Option<u64>, height: f32) -> Element<'_, Message> {
        let band = |line: u64, style: fn(&iced::Theme) -> container::Style| {
            column![
                space().height(line.saturating_sub(1) as f32 * LINE),
                container(space()).width(Fill).height(LINE).style(style),
            ]
            .boxed()
        };
        let mut layers = stack![space().width(Fill).height(height).boxed()];
        if let Some(line) = failed {
            layers = layers.push(band(line, theme::failed));
        }
        if let Some(line) = self.marked {
            layers = layers.push(band(line, theme::marked)).push(
                column![
                    space().height(line.saturating_sub(1) as f32 * LINE),
                    container(space()).width(2).height(LINE).style(theme::edge),
                ]
                .boxed(),
            );
        }
        layers.boxed()
    }

    /// The values near the pause, each at the end of the line that names
    /// it last, and a failed save's error at the end of its line.
    fn inline<'a>(
        &'a self,
        values: &[(&'a str, &'a str)],
        failed: Option<(u64, &'a str)>,
    ) -> Element<'a, Message> {
        let Some(buffer) = self.buffer() else {
            return space().boxed();
        };
        let mut ends: Vec<(usize, String, Paint)> = Vec::new();
        if let (Some(marked), false) = (self.marked, values.is_empty()) {
            let names: Vec<&str> = values.iter().map(|(name, _)| *name).collect();
            let marked = usize::try_from(marked.saturating_sub(1)).unwrap_or_default();
            for (line, shown) in inline::placed(|index| buffer.line(index), marked, &names) {
                let said: Vec<String> = shown
                    .into_iter()
                    .map(|at| format!("{} = {}", values[at].0, inline::short(values[at].1)))
                    .collect();
                ends.push((line, said.join(", "), theme::inline));
            }
        }
        if let Some((line, message)) = failed {
            let line = usize::try_from(line.saturating_sub(1)).unwrap_or_default();
            ends.retain(|(shown, _, _)| *shown != line);
            ends.push((line, message.to_owned(), theme::error));
        }
        ends.sort_by_key(|(line, _, _)| *line);
        let mut layer = column![].padding([0.0, PAD]);
        let mut next = 0;
        for (line, said, style) in ends {
            let written = buffer.line(line).unwrap_or_default().into_owned();
            layer = layer
                .push(space().height((line - next) as f32 * LINE).boxed())
                .push(
                    row![
                        code_text(written).color(Color::TRANSPARENT),
                        code_text(format!("    {said}")).style(style),
                    ]
                    .boxed(),
                );
            next = line + 1;
        }
        layer.boxed()
    }

    /// What the paused program's hover read said of the expression under
    /// the pointer: its value, or why a hover doesn't run it.
    fn hover(&self) -> Element<'_, Message> {
        let shown = match self.hover.as_ref().map(Hover::said) {
            Some(Said::Value { expression, value }) => row![
                code_text(expression.as_str()).style(theme::name),
                code_text(" = "),
                code_text(value.as_str()).style(move |theme| theme::value(theme, Ok(value))),
            ],
            Some(Said::Refused { expression, why }) => row![
                code_text(expression.as_str()).style(theme::name),
                code_text("  "),
                code_text(why.as_str()).style(theme::faint),
            ],
            Some(Said::Waiting | Said::Nothing) | None => return space().boxed(),
        };
        container(shown.wrap())
            .padding([3, 8])
            .style(theme::tip)
            .boxed()
    }
}

/// Line `number`'s place in the gutter: its breakpoint's dot when `set`,
/// and its number, in the accent on the marked line and the error's
/// colour on a failed save's.
fn gutter<'a>(number: u64, set: bool, marked: bool, failed: bool) -> Element<'a, Message> {
    let style: fn(&iced::Theme) -> text::Style = match (marked, failed) {
        (true, _) => theme::mark,
        (false, true) => theme::error,
        (false, false) => theme::faint,
    };
    button(
        row![
            code_text(if set { "●" } else { " " }).style(theme::breakpoint),
            code_text(format!("{number:>4}")).style(style),
        ]
        .spacing(4),
    )
    .padding([0, 6])
    .style(button::text)
    .on_press(Message::Toggle(number))
    .boxed()
}

/// Text of the code's font, size and line height.
fn code_text<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a> {
    text(content)
        .font(Font::MONOSPACE)
        .size(theme::SIZE)
        .line_height(line_height())
        .wrapping(Wrapping::None)
}

/// A quiet word where the code would be.
fn note<'a>(said: &'a str) -> Element<'a, Message> {
    container(text(said).size(theme::SIZE).style(theme::faint))
        .padding(8)
        .boxed()
}
