// The link preview for rawr.airstrike.fyi/bot/invite, drawn by Rawr itself
// in RawrGallery's Ink sheet. Render it with:
//   rawr-play play/deploy/cloudflare/og/card.rs --size 600x315 -o play/deploy/cloudflare/public/og/invite.png
use iced::theme::palette::Seed;
use iced::widget::{button, column, container, row, space, text};
use iced::{Border, Center, Color, Fill, Font, Theme, Widget, border};

pub fn main() -> iced::Result {
    iced::application(Card::default, Card::update, Card::view)
        .theme(Card::theme)
        .run()
}

#[derive(Default)]
struct Card;

#[derive(Debug, Clone)]
enum Message {
    Increment,
}

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
        let brand = row![
            mark,
            text("RAWR").size(13).font(Font::MONOSPACE),
            caption("PLAY"),
        ]
        .spacing(10)
        .align_y(Center);

        let code = container(
            column![
                text("?play").size(13).font(Font::MONOSPACE).style(text::primary),
                text("column![").size(13).font(Font::MONOSPACE),
                text("    button(\"Increment\"),").size(13).font(Font::MONOSPACE),
                text("    text(3).size(40),").size(13).font(Font::MONOSPACE),
                text("]").size(13).font(Font::MONOSPACE),
            ]
            .spacing(1),
        )
        .padding(14)
        .width(Fill)
        .style(panel);

        let rendered = container(
            column![
                button(text("Increment").size(14)).on_press(Message::Increment).style(key),
                text(3).size(40),
            ]
            .spacing(8)
            .align_x(Center),
        )
        .padding(16)
        .center_x(150)
        .style(panel);

        let lede = column![
            row![
                text("//").size(11).font(Font::MONOSPACE).style(text::primary),
                caption("DISCORD · ICED · FIRST FRAME"),
            ]
            .spacing(8),
            text("Paste iced code, get the frame").size(26),
            text("Write ?play and a view or a whole app. Rawr renders it.")
                .size(14)
                .style(text::secondary),
        ]
        .spacing(10);

        container(
            column![
                brand,
                lede,
                row![code, rendered].spacing(16).align_y(Center),
                row![
                    caption("RAWR-BOT.AIRSTRIKE.FYI/INVITE"),
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

fn frame(color: Color) -> Border {
    border::rounded(0).width(1).color(color)
}

fn panel(theme: &Theme) -> container::Style {
    let palette = theme.palette();
    container::Style::default()
        .background(palette.background.weakest.color)
        .border(frame(palette.background.strong.color))
}

fn key(theme: &Theme, _status: button::Status) -> button::Style {
    let palette = theme.palette();
    button::Style {
        background: Some(palette.primary.base.color.into()),
        text_color: palette.primary.base.text,
        border: frame(palette.primary.base.color),
        ..button::Style::default()
    }
}
