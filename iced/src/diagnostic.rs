//! Source-runtime failures retained by the native application adapter.

use iced::widget::Widget as _;

use crate::runtime;

pub(crate) const OVERLAY_TITLE: &str = "Rawr runtime diagnostic";

/// The application callback that produced a diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    /// The state-derived title callback failed.
    Title,
    /// A passive subscription callback or stream plan failed.
    Subscription,
    /// A durable task could not issue or complete.
    Task,
    /// The update callback failed.
    Update,
    /// The state-derived theme callback failed.
    Theme,
    /// The view callback failed.
    View,
}

/// One deterministic diagnostic retained for the host overlay layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    stage: Stage,
    report: rawr::diagnostic::Report,
}

impl Entry {
    pub(crate) fn new(stage: Stage, report: rawr::diagnostic::Report) -> Self {
        Self { stage, report }
    }

    /// Return the callback stage that failed.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// Return the canonical report message for compatibility with text probes.
    #[must_use]
    pub fn message(&self) -> &str {
        self.report.message()
    }

    /// Return the canonical diagnostic retained by the host.
    #[must_use]
    pub const fn report(&self) -> &rawr::diagnostic::Report {
        &self.report
    }
}

pub(crate) fn overlay(
    content: iced::Element<'static, runtime::Message>,
    entries: &[Entry],
) -> iced::Element<'static, runtime::Message> {
    if entries.is_empty() {
        return content;
    }

    let overlay = panel(entries.iter().map(detail));
    let content = iced::widget::container(content)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill);

    iced::widget::stack![content, overlay].boxed()
}

pub(crate) fn lifecycle(
    entries: &[crate::widget::diagnostic::Entry],
) -> iced::Element<'static, runtime::Message> {
    panel(
        entries
            .iter()
            .map(|entry| report_detail(&entry.context(), entry.report())),
    )
}

pub(crate) fn lifecycle_fallback() -> iced::Element<'static, runtime::Message> {
    panel([iced::widget::text("A scripted widget hook failed.")
        .size(14)
        .boxed()])
}

fn panel<Message: 'static>(
    details: impl IntoIterator<Item = iced::Element<'static, Message>>,
) -> iced::Element<'static, Message> {
    let card = iced::widget::container(
        iced::widget::column(
            std::iter::once(iced::widget::text(OVERLAY_TITLE).size(16).boxed()).chain(details),
        )
        .spacing(10),
    )
    .padding(12)
    .style(|_theme| iced::widget::container::Style {
        text_color: Some(iced::Color::from_rgb8(0xff, 0xf1, 0xf2)),
        background: Some(iced::Color::from_rgb8(0x45, 0x0a, 0x0a).into()),
        border: iced::Border {
            color: iced::Color::from_rgb8(0xfb, 0x71, 0x71),
            width: 1.0,
            radius: 6.0.into(),
        },
        ..iced::widget::container::Style::default()
    });
    let overlay = iced::widget::container(card)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .padding(12)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom);

    overlay.boxed()
}

fn detail(entry: &Entry) -> iced::Element<'static, runtime::Message> {
    report_detail(entry.stage.label(), entry.report())
}

pub(crate) fn report_detail<Message: 'static>(
    context: &str,
    report: &rawr::diagnostic::Report,
) -> iced::Element<'static, Message> {
    let heading = format!(
        "{} · {} · {}[{}]",
        context,
        report.category(),
        report.severity(),
        report.code()
    );
    let labels = report.labels().iter().map(|label| {
        let mut text = format!(
            "{}: {}:{}",
            label.kind(),
            label.source().identity(),
            label.span()
        );
        if let Some(message) = label.message() {
            text.push_str(&format!(" — {message}"));
        }
        iced::widget::text(text).size(12).boxed()
    });
    let sources = report
        .sources()
        .iter()
        .filter(|source| {
            !report
                .labels()
                .iter()
                .any(|label| label.source() == *source)
        })
        .map(|source| {
            iced::widget::text(format!("source: {}", source.identity()))
                .size(12)
                .boxed()
        });
    let help = report
        .help()
        .into_iter()
        .map(|help| iced::widget::text(format!("help: {help}")).size(12).boxed());
    let notes = report
        .notes()
        .map(|note| iced::widget::text(format!("note: {note}")).size(12).boxed());
    iced::widget::column(
        [
            iced::widget::text(heading).size(12).boxed(),
            iced::widget::text(report.message().to_owned())
                .size(14)
                .boxed(),
        ]
        .into_iter()
        .chain(sources)
        .chain(labels)
        .chain(help)
        .chain(notes),
    )
    .spacing(4)
    .boxed()
}

impl Stage {
    const fn label(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Subscription => "subscription",
            Self::Task => "task",
            Self::Update => "update",
            Self::Theme => "theme",
            Self::View => "view",
        }
    }
}
