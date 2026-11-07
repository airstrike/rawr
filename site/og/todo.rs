// The link preview for rawr.airstrike.fyi/todo, drawn in iced in the
// work plan's style: black on white, one red, Abitaro Sans, and the site's
// mark (site/public/favicon.svg). site/public/og/todo.png is committed as it
// was drawn: ?play's headless shell (play/universe/shell) writes the frame
// to the file PLAY_FRAME names, at 600 by 315 points and twice the scale.
// It builds in a Cargo project whose [patch] puts that shell in place of
// iced_winit, as play/universe/Cargo.toml does.
use iced::font::Weight;
use iced::widget::canvas::{Frame, Geometry, Path};
use iced::widget::{canvas, column, container, row, space, text};
use iced::{
    mouse, Border, Center, Color, Fill, Font, Length, Point, Rectangle, Renderer, Size, Theme,
    Widget,
};

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

/// Where a row's work stands, drawn as the page draws it.
#[derive(Clone, Copy)]
enum State {
    Done,
    Now,
    Next,
}

/// The page's axis, in units of complexity.
const AXIS: u16 = 88;

/// The chart's groups, each spanning its rows on the page's axis, with the
/// last milestone it reaches (plan/gantt.json at 0.3.171).
const ROWS: [(&str, &str, u16, u16, State); 10] = [
    ("0.2", "Foundations", 0, 26, State::Done),
    ("0.3", "Shared dependency cache", 26, 10, State::Done),
    ("0.3", "The gnarly snippet", 51, 6, State::Done),
    ("0.4", "Shared bindings", 37, 14, State::Now),
    ("0.4", "No glue", 22, 31, State::Now),
    ("0.3", "Retire rawr-iced", 63, 3, State::Done),
    ("0.3", "Rust as rustc has it", 66, 14, State::Now),
    ("0.3", "Shipping", 70, 16, State::Now),
    ("0.3", "Playground", 36, 52, State::Now),
    ("0.5", "Debugger", 53, 14, State::Now),
];

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
            space().width(Fill),
            text("rawr.airstrike.fyi/todo")
                .size(11)
                .font(abitaro(Weight::Normal))
                .color(QUIET),
        ]
        .align_y(Center);

        let title = text("Work plan")
            .size(46)
            .line_height(1.0)
            .font(abitaro(Weight::ExtraBold));

        let chart = column(ROWS.iter().map(|&(milestone, name, start, size, state)| {
            chart_row(milestone, name, start, size, state)
        }))
        .spacing(1);

        let legend = row![
            key(State::Done, "Done"),
            key(State::Now, "Now"),
            key(State::Next, "Next"),
            space().width(Fill),
            text("Bar length is complexity, never time.")
                .size(10)
                .font(abitaro(Weight::Normal))
                .color(QUIET),
        ]
        .spacing(16)
        .align_y(Center);

        container(column![
            header,
            space().height(8),
            title,
            space().height(10),
            rule(3.0, INK),
            space().height(10),
            chart,
            space().height(Fill),
            rule(1.0, INK),
            space().height(8),
            legend,
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

/// One epic: its milestone, its name, and its bar on the shared axis.
fn chart_row<'a>(
    milestone: &'a str,
    name: &'a str,
    start: u16,
    size: u16,
    state: State,
) -> impl Widget<Message> + 'a {
    let track = row![
        space().width(Length::FillPortion(start.max(1))),
        bar(state, Length::FillPortion(size)),
        space().width(Length::FillPortion((AXIS - start - size).max(1))),
    ];
    let track = container(track)
        .width(Fill)
        .style(|_| container::Style::default().border(Border::default().color(GRID).width(0.0)));

    row![
        text(milestone)
            .size(10)
            .width(24)
            .font(abitaro(Weight::ExtraBold)),
        text(name)
            .size(11)
            .width(150)
            .font(abitaro(Weight::Semibold)),
        track,
    ]
    .spacing(10)
    .align_y(Center)
}

fn bar<'a>(state: State, width: Length) -> impl Widget<Message> + 'a {
    container(space().height(9).width(Fill))
        .width(width)
        .style(move |_| match state {
            State::Done => container::Style::default().background(INK),
            State::Now => container::Style::default().background(RED),
            State::Next => {
                container::Style::default().border(Border::default().color(INK).width(1.0))
            }
        })
}

fn key<'a>(state: State, label: &'a str) -> impl Widget<Message> + 'a {
    row![
        bar(state, Length::Fixed(20.0)),
        text(label).size(10).font(abitaro(Weight::Semibold)),
    ]
    .spacing(6)
    .align_y(Center)
}

fn rule<'a>(height: f32, color: Color) -> impl Widget<Message> + 'a {
    container(space().height(height).width(Fill))
        .style(move |_| container::Style::default().background(color))
}

/// The site's mark, as site/public/favicon.svg draws it on its 32-unit
/// grid: a red disc over a heavy rule.
fn mark<'a>(size: f32) -> impl Widget<Message> + 'a {
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
