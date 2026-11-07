// The link preview for rawr.airstrike.fyi/bot/invite and /play, drawn in
// iced in the site's look: black on white, one red, Abitaro Sans,
// heavy rules, and the playground's mark (site/public/play.svg).
// public/og/invite.png is committed as it was drawn: ?play's headless shell
// (play/universe/shell) writes the frame to the file PLAY_FRAME names, at
// 600 by 315 points and twice the scale. It builds in a Cargo project whose
// [patch] puts that shell in place of iced_winit, as play/universe/Cargo.toml
// does.
use iced::font::Weight;
use iced::widget::canvas::{Frame, Geometry, Path};
use iced::widget::{button, canvas, column, container, row, space, text};
use iced::{mouse, Center, Color, Fill, Font, Point, Rectangle, Renderer, Size, Theme, Widget};

pub fn main() -> iced::Result {
    iced::application(Card::default, Card::update, Card::view)
        .theme(Card::theme)
        .window_size((600.0, 315.0))
        .scale_factor(|_| 2.0)
        .run()
}

#[derive(Default)]
struct Card;

#[derive(Debug, Clone)]
enum Message {
    Increment,
}

const INK: Color = Color::BLACK;
const PAPER: Color = Color::WHITE;
const RED: Color = Color::from_rgb8(0xe4, 0x00, 0x2b);
const QUIET: Color = Color::from_rgb8(0x6b, 0x6b, 0x6b);
/// Code blocks.
const WASH: Color = Color::from_rgb8(0xf4, 0xf4, 0xf4);
/// Literals in code.
const LIT: Color = Color::from_rgb8(0x00, 0x62, 0xb1);

impl Card {
    fn update(&mut self, _message: Message) {}

    fn theme(&self) -> Theme {
        Theme::Light
    }

    fn view(&self) -> impl Widget<Message> {
        let header = row![
            mark(14.0),
            space().width(8),
            text("rawr").size(12).font(abitaro(Weight::ExtraBold)),
            space().width(8),
            label("Play bot", QUIET),
            space().width(Fill),
            text("rawr.airstrike.fyi/bot/invite")
                .size(11)
                .font(abitaro(Weight::Normal))
                .color(QUIET),
        ]
        .align_y(Center);

        let title = column![
            text("Rawr Play")
                .size(44)
                .line_height(1.0)
                .font(abitaro(Weight::ExtraBold)),
            text("Paste iced code after ?play in Discord, and get its frame back.")
                .size(13)
                .font(abitaro(Weight::Medium)),
        ]
        .spacing(4);

        let source = container(
            column![
                code(&[("?play", RED)]),
                code(&[("column![", INK)]),
                code(&[("    button(", INK), ("\"Increment\"", LIT), ("),", INK)]),
                code(&[
                    ("    text(", INK),
                    ("3", LIT),
                    (").size(", INK),
                    ("40", LIT),
                    ("),", INK)
                ]),
                code(&[("]", INK)]),
            ]
            .spacing(2),
        )
        .padding([10, 14])
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style::default().background(WASH));

        let frame = container(
            column![
                button(text("Increment").size(10).font(abitaro(Weight::Semibold)))
                    .on_press(Message::Increment)
                    .padding([5, 10])
                    .style(key),
                text("3").size(40).font(abitaro(Weight::Normal)),
            ]
            .spacing(6)
            .align_x(Center),
        )
        .center(Fill)
        .width(150)
        .style(|_| {
            container::Style::default()
                .background(PAPER)
                .border(iced::Border::default().width(1.0).color(INK))
        });

        let shown = column![
            row![
                label("Your message", INK),
                space().width(Fill),
                label("Its frame", INK)
            ],
            space().height(5),
            rule(2.0, INK),
            space().height(6),
            row![source, frame].spacing(10).height(Fill),
        ]
        .height(Fill);

        container(column![
            header,
            space().height(10),
            title,
            space().height(10),
            rule(4.0, INK),
            space().height(10),
            shown,
            space().height(10),
            rule(1.0, INK),
            space().height(6),
            row![
                label("Written in Rust", QUIET),
                space().width(Fill),
                label("Rendered by iced", QUIET),
            ],
        ])
        .padding([22, 28])
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style::default().background(PAPER).color(INK))
    }
}

fn abitaro(weight: Weight) -> Font {
    Font::new("Abitaro Sans").weight(weight)
}

/// The site's monospace, as its `--mono` falls back to on a Mac.
const MONO: Font = Font::new("Menlo");

/// A label in the site's way: small, semibold, uppercase.
fn label(content: &str, color: Color) -> iced::widget::Text<'static> {
    text(content.to_uppercase())
        .size(9)
        .font(abitaro(Weight::Semibold))
        .color(color)
}

/// One line of code, its pieces each in its colour.
fn code(pieces: &[(&'static str, Color)]) -> impl Widget<Message> + 'static {
    row(pieces
        .iter()
        .map(|&(piece, color)| text(piece).size(10).font(MONO).color(color)))
}

fn rule(height: f32, color: Color) -> impl Widget<Message> + 'static {
    container(space().height(height).width(Fill))
        .style(move |_| container::Style::default().background(color))
}

/// The site's buttons: solid ink, square.
fn key(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(INK.into()),
        text_color: PAPER,
        border: iced::Border::default().width(1.0).color(INK),
        ..button::Style::default()
    }
}

/// The playground's mark, as site/public/play.svg draws it on its 32-unit
/// grid: a red triangle over a heavy rule.
fn mark(size: f32) -> impl Widget<Message> + 'static {
    canvas(Mark).width(size).height(size)
}

struct Mark;

impl canvas::Program<Message> for Mark {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let unit = bounds.width / 32.0;
        let triangle = Path::new(|path| {
            path.move_to(Point::new(9.0 * unit, 3.5 * unit));
            path.line_to(Point::new(25.5 * unit, 12.5 * unit));
            path.line_to(Point::new(9.0 * unit, 21.5 * unit));
            path.close();
        });
        frame.fill(&triangle, RED);
        frame.fill_rectangle(
            Point::new(3.0 * unit, 24.0 * unit),
            Size::new(26.0 * unit, 5.0 * unit),
            INK,
        );
        vec![frame.into_geometry()]
    }
}
