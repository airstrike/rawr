// The link preview for rawr.airstrike.fyi, drawn by Rawr itself in
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
        let brand = row![mark, text("RAWR").size(13).font(Font::MONOSPACE), caption("PRE-ALPHA")]
            .spacing(10)
            .align_y(Center);

        let lede = column![
            row![
                text("//").size(11).font(Font::MONOSPACE).style(text::primary),
                caption("A SCRIPTING LANGUAGE THAT READS LIKE RUST"),
            ]
            .spacing(8),
            row![word("R", "ust"), word("A", "ugmented"), word("W", "ith"), word("R", "ust")]
                .spacing(9),
            text("Embed it in any Rust program. With iced, apps reload in about 0.2 s.")
                .size(14)
                .style(text::secondary),
        ]
        .spacing(10);

        container(
            column![
                brand,
                lede,
                row![file("main.rs", "cargo run"), file("cart.rawr", "rawr run cart.rawr")].spacing(16),
                row![
                    caption("RAWR.AIRSTRIKE.FYI"),
                    space().width(Fill),
                    caption("WRITTEN IN RAWR · RENDERED BY ICED"),
                ],
            ]
            .spacing(10),
        )
        .padding([14, 28])
        .width(Fill)
        .height(Fill)
    }
}

/// One word of the tagline, its initial in pink: R-A-W-R.
fn word(initial: &str, rest: &str) -> impl Widget<Message> {
    row![
        text(initial.to_string()).size(26).style(text::primary),
        text(rest.to_string()).size(26),
    ]
}

/// The same cart file as Rust and as Rawr: only the command differs.
fn file(name: &str, command: &str) -> impl Widget<Message> {
    let source = container(
        column![
            code("fn total(items: &[Item]) -> f64 {"),
            code("    let mut sum = 0.0;"),
            code("    for item in items {"),
            code("        sum += item.price();"),
        ]
        .spacing(1),
    )
    .padding([8, 12])
    .width(Fill)
    .style(panel);
    let shell = container(
        row![text("$").size(11).font(Font::MONOSPACE).style(text::secondary), code(command)]
            .spacing(7),
    )
    .padding([5, 12])
    .width(Fill)
    .style(terminal);

    column![caption(name), column![source, shell]]
        .spacing(6)
        .width(Fill)
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
