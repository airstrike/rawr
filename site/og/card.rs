// The link preview for rawr.airstrike.fyi, drawn in iced in the
// site's look: black on white, one red, Abitaro Sans, heavy rules, and the
// mark of site/public/favicon.svg. site/public/og/card.png is committed as
// it was drawn: ?play's headless shell (play/universe/shell) writes the
// frame to the file PLAY_FRAME names, at 600 by 315 points and twice the
// scale. It builds in a Cargo project whose [patch] puts that shell in place
// of iced_winit, as play/universe/Cargo.toml does.
use iced::font::Weight;
use iced::widget::canvas::{Frame, Geometry, Path};
use iced::widget::text::Wrapping;
use iced::widget::{canvas, column, container, row, space, text};
use iced::{mouse, Color, Fill, Font, Point, Rectangle, Renderer, Size, Theme, Widget};

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
enum Message {}

const INK: Color = Color::BLACK;
const PAPER: Color = Color::WHITE;
const RED: Color = Color::from_rgb8(0xe4, 0x00, 0x2b);
const QUIET: Color = Color::from_rgb8(0x6b, 0x6b, 0x6b);
const GRID: Color = Color::from_rgb8(0xe8, 0xe8, 0xe8);
/// Code blocks, and the terminal strip under them.
const WASH: Color = Color::from_rgb8(0xf4, 0xf4, 0xf4);
const TERM: Color = Color::from_rgb8(0xeb, 0xeb, 0xeb);
/// The rawr side of the landing page's file, and its terminal strip.
const RAWR_PANE: Color = Color::from_rgb8(0xfd, 0xf3, 0xf5);
const RAWR_TERM: Color = Color::from_rgb8(0xf6, 0xe6, 0xea);

impl Card {
    fn update(&mut self, _message: Message) {}

    fn theme(&self) -> Theme {
        Theme::Light
    }

    fn view(&self) -> impl Widget<Message> {
        let header = row![
            mark(14.0),
            text("rawr").size(12).font(abitaro(Weight::ExtraBold)),
            label("Pre-alpha", QUIET),
            space().width(Fill),
            text("rawr.airstrike.fyi")
                .size(11)
                .font(abitaro(Weight::Normal))
                .color(QUIET),
        ]
        .spacing(8)
        .align_y(iced::Center);

        // The landing page's title: the word, and a heavy rule as wide as it.
        let word = column![
            text("rawr")
                .size(96)
                .line_height(0.92)
                .font(abitaro(Weight::ExtraBold)),
            rule(4.0, INK),
        ]
        .width(192)
        .spacing(4);
        let lede = column![
            word,
            space().height(6),
            text("is rust augmented with rust")
                .size(15)
                .font(abitaro(Weight::Medium)),
            space().height(Fill),
            text("A Rust interpreter for your dev builds.")
                .size(19)
                .line_height(1.15)
                .font(abitaro(Weight::Bold)),
        ]
        .width(196)
        .height(Fill);

        let files = column![
            row![
                label("Cargo run", INK),
                space().width(Fill),
                label("Rawr run", INK)
            ],
            space().height(5),
            rule(2.0, INK),
            space().height(6),
            row![
                file(INK, WASH, TERM, "cargo run"),
                rule_across(RED),
                file(RED, RAWR_PANE, RAWR_TERM, "rawr run"),
            ]
            .height(Fill),
        ]
        .width(Fill)
        .height(Fill);

        container(column![
            header,
            space().height(16),
            row![lede, files].spacing(24).height(Fill),
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

/// One side of the landing page's file: `src/main.rs`, a few of its lines,
/// and the command that runs it with what it printed.
fn file(name: Color, pane: Color, term: Color, command: &str) -> impl Widget<Message> + 'static {
    let lines = column![
        code(&[("// rawr", QUIET)]),
        code(&[("fn ", RED), ("total(cart: &[Item]) -> f64 {", INK)]),
        code(&[
            ("    let mut ", RED),
            ("sum = ", INK),
            ("0.0", LIT),
            (";", INK)
        ]),
        code(&[
            ("    for ", RED),
            ("item ", INK),
            ("in ", RED),
            ("cart {", INK)
        ]),
        code(&[("        sum += item.price();", INK)]),
        code(&[("    }", INK)]),
        code(&[("    sum", INK)]),
        code(&[("}", INK)]),
    ]
    .spacing(2);
    let header = container(
        text("src/main.rs")
            .size(8)
            .font(MONO.weight(Weight::Bold))
            .color(name),
    )
    .padding([6, 8])
    .width(Fill)
    .style(move |_| container::Style::default().background(pane));
    let source = container(lines)
        .padding([8, 8])
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style::default().background(pane));
    let shell = container(column![
        row![
            text("$ ").size(8).font(MONO).color(QUIET),
            text(command.to_owned()).size(8).font(MONO)
        ],
        text("total: 27").size(8).font(MONO),
    ])
    .padding([6, 8])
    .width(Fill)
    .style(move |_| container::Style::default().background(term));

    column![header, rule(1.0, GRID), source, shell].width(Fill)
}

/// Literals in code.
const LIT: Color = Color::from_rgb8(0x00, 0x62, 0xb1);

/// One line of code, its pieces each in its colour.
fn code(pieces: &[(&'static str, Color)]) -> impl Widget<Message> + 'static {
    row(pieces.iter().map(|&(piece, color)| {
        text(piece)
            .size(7.5)
            .font(MONO)
            .color(color)
            .wrapping(Wrapping::None)
    }))
}

fn rule(height: f32, color: Color) -> impl Widget<Message> + 'static {
    container(space().height(height).width(Fill))
        .style(move |_| container::Style::default().background(color))
}

/// The landing page's divider between Cargo's side and rawr's.
fn rule_across(color: Color) -> impl Widget<Message> + 'static {
    container(space().width(2).height(Fill))
        .style(move |_| container::Style::default().background(color))
}

/// The site's mark, as site/public/favicon.svg draws it on its 32-unit
/// grid: a red disc over a heavy rule.
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
        frame.fill(
            &Path::circle(Point::new(16.0 * unit, 12.5 * unit), 8.5 * unit),
            RED,
        );
        frame.fill_rectangle(
            Point::new(3.0 * unit, 24.0 * unit),
            Size::new(26.0 * unit, 5.0 * unit),
            INK,
        );
        vec![frame.into_geometry()]
    }
}
