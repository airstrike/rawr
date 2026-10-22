// The link preview of a shared playground snippet that did not render (a
// code error or a limit), drawn by Rawr in RawrGallery's Ink sheet:
//   rawr-play play/deploy/cloudflare/og/error.rs --size 600x315 -o play/deploy/cloudflare/public/og/play-error.png
use iced::theme::palette::Seed;
use iced::widget::{column, container, row, space, text};
use iced::{Center, Color, Fill, Font, Theme, Widget};

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
        let brand = row![mark, text("RAWR").size(13).font(Font::MONOSPACE), caption("PLAYGROUND")]
            .spacing(10)
            .align_y(Center);
        let lede = column![
            row![
                text("//").size(11).font(Font::MONOSPACE).style(text::primary),
                caption("NO FRAME"),
            ]
            .spacing(8),
            text("This snippet did not render").size(30),
            text("Open the link to see the error and fix it.")
                .size(15)
                .style(text::secondary),
        ]
        .spacing(12);
        container(
            column![
                brand,
                space().height(Fill),
                lede,
                space().height(Fill),
                row![
                    caption("RAWR-BOT.AIRSTRIKE.FYI/PLAY"),
                    space().width(Fill),
                    caption("WRITTEN IN RAWR · RENDERED BY ICED"),
                ],
            ]
            .spacing(14),
        )
        .padding([18, 28])
        .width(Fill)
        .height(Fill)
    }
}

fn caption(content: &str) -> iced::widget::Text<'static> {
    text(content.to_string()).size(11).font(Font::MONOSPACE).style(text::secondary)
}
