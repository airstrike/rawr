// The link preview for rawr.airstrike.fyi/todo, drawn by rawr itself in the
// work plan's style: black on white, one red, Abitaro Sans. Render it with
// static Abitaro Sans instances (400, 600, 800) beside the ?play fonts:
//   rawr-play site/og/todo.rs --size 600x315 --theme light --fonts target/og/fonts -o site/public/og/todo.png
use iced::font::Weight;
use iced::widget::{column, container, row, space, text};
use iced::{Border, Center, Color, Fill, Font, Length, Theme, Widget};

pub fn main() -> iced::Result {
    iced::application(Card::default, Card::update, Card::view)
        .theme(Card::theme)
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

/// The chart's epics, each spanning its rows on the page's axis of 66 units
/// of complexity (plan/gantt.json at 0.3.28).
const ROWS: [(&str, &str, u16, u16, State); 7] = [
    ("0.2", "Foundations", 0, 26, State::Done),
    ("0.3", "Shared dependency cache", 26, 10, State::Done),
    ("0.4", "No glue", 22, 31, State::Now),
    ("0.4", "Shared bindings", 37, 14, State::Now),
    ("0.3", "The gnarly snippet", 51, 6, State::Done),
    ("", "Playground", 36, 27, State::Next),
    ("", "Retire rawr-iced", 63, 3, State::Next),
];

impl Card {
    fn update(&mut self, _message: Message) {}

    fn theme(&self) -> Theme {
        Theme::Light
    }

    fn view(&self) -> impl Widget<Message> {
        let header = row![
            text("rawr").size(12).font(abitaro(Weight::ExtraBold)),
            space().width(Fill),
            text("rawr.airstrike.fyi/todo")
                .size(11)
                .font(abitaro(Weight::Normal))
                .color(QUIET),
        ]
        .align_y(Center);

        let title = text("Work plan")
            .size(54)
            .line_height(1.0)
            .font(abitaro(Weight::ExtraBold));

        let chart = column(ROWS.iter().map(|&(milestone, name, start, size, state)| {
            chart_row(milestone, name, start, size, state)
        }))
        .spacing(5);

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
        space().width(Length::FillPortion((66 - start - size).max(1))),
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
