// The link preview for rawr.airstrike.fyi, drawn by rawr itself in
// RawrGallery's Ink sheet. Render it with:
//   rawr-play site/og/card.rs --size 600x315 --fonts target/play/fonts -o site/public/og/card.png
use iced::theme::palette::Seed;
use iced::widget::{column, container, row, space, text};
use iced::{Border, Center, Color, Fill, Font, Theme, Widget, border};

pub fn main() -> iced::Result {
    iced::application(Card::default, Card::update, Card::view)
        .theme(Card::theme)
        .run()
}

#[derive(Default)]
struct Card;

#[derive(Debug, Clone)]
enum Message {}

impl Card {
    fn update(&mut self, _message: Message) {}

    fn theme(&self) -> Theme {
        Theme::custom(
            "Ink",
            Seed {
                background: Color::from_rgb8(0x14, 0x14, 0x16),
                text: Color::from_rgb8(0xe4, 0xe4, 0xe8),
                primary: Color::from_rgb8(0xff, 0x2e, 0x93),
                success: Color::from_rgb8(0xff, 0x8c, 0xc6),
                warning: Color::from_rgb8(0x9a, 0x9a, 0xa3),
                danger: Color::from_rgb8(0xff, 0x2e, 0x93),
            },
        )
    }

    fn view(&self) -> impl Widget<Message> {
        let mark = container(space().width(8).height(8)).style(|theme: &Theme| {
            container::Style::default().background(theme.palette().primary.base.color)
        });
        let brand = row![mark, text("rawr").size(13).font(Font::MONOSPACE), caption("pre-alpha")]
            .spacing(10)
            .align_y(Center);

        // The site's title: the word, and a line under it as wide as the word.
        let title = column![
            text("rawr").size(100).line_height(1.0).style(text::primary),
            text("is rust augmented with rust").size(16),
        ]
        .spacing(0);
        let lede = column![
            title,
            space().height(22),
            text("A Rust interpreter for your dev builds.").size(17),
        ]
        .width(216);

        let files = column![
            file("compiled by Cargo", "cargo run"),
            file("interpreted by rawr", "rawr run"),
        ]
        .spacing(10)
        .width(Fill);

        container(
            column![
                brand,
                row![lede, files].spacing(26).height(Fill),
                caption("rawr.airstrike.fyi"),
            ]
            .spacing(12),
        )
        .padding([16, 28])
        .width(Fill)
        .height(Fill)
    }
}

/// One copy of the cart's `src/main.rs`, and the command that runs it.
fn file(how: &str, command: &str) -> impl Widget<Message> {
    let header = row![code("src/main.rs"), space().width(Fill), caption(how)];
    let source = container(
        column![
            code("fn total(items: &[Item]) -> f64 {"),
            code("    let mut sum = 0.0;"),
            code("    for item in items {"),
        ]
        .spacing(1),
    )
    .padding([7, 12])
    .width(Fill)
    .style(panel);
    let shell = container(
        row![
            text("$").size(11).font(Font::MONOSPACE).style(text::secondary),
            code(command),
            space().width(Fill),
            text("total: 27").size(11).font(Font::MONOSPACE).style(text::success),
        ]
        .spacing(7),
    )
    .padding([5, 12])
    .width(Fill)
    .style(terminal);

    column![header, column![source, shell]].spacing(5).width(Fill)
}

fn code(line: &str) -> iced::widget::Text<'static> {
    text(line.to_string()).size(11).font(Font::MONOSPACE)
}

fn caption(content: &str) -> iced::widget::Text<'static> {
    text(content.to_string()).size(11).font(Font::MONOSPACE).style(text::secondary)
}

fn frame(color: Color) -> Border {
    border::rounded(0).width(1).color(color)
}

fn panel(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style::default()
        .background(palette.background.weakest.color)
        .border(frame(palette.background.strong.color))
}

/// The terminal strip under each file: the command that runs it.
fn terminal(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style::default()
        .background(palette.background.base.color)
        .border(frame(palette.background.strong.color))
}
