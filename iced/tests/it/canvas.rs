//! iced's `canvas`: a script `canvas::Program` draws with `Frame`, `Path`,
//! `Stroke` and `Cache`, and its `update` turns iced's events into
//! Messages through `canvas::Action`.

use rawr_iced::program;

fn boot(source: &str) -> program::Adapter {
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter()
}

const DOTS: &str = r#"
use iced::mouse;
use iced::widget::canvas::{self, Action, Cache, Event, Frame, Geometry, LineCap, Path, Stroke};
use iced::widget::{column, text};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Theme, Vector};

#[derive(Default)]
struct Dots {
    dots: Vec<Point>,
    cache: Cache,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Add(Point),
}

impl Dots {
    fn update(&mut self, message: Message) {
        match message {
            Message::Add(point) => {
                self.dots.push(point);
                self.cache.clear();
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            text(format!("{} dots", self.dots.len())),
            canvas(self).width(Fill).height(200),
        ]
        .boxed()
    }
}

impl canvas::Program<Message> for Dots {
    type State = Option<Point>;

    fn update(
        &self,
        hovered: &mut Option<Point>,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let position = cursor.position_in(bounds)?;
        *hovered = Some(position);
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(Action::publish(Message::Add(position)).and_capture())
            }
            _ => Some(Action::request_redraw()),
        }
    }

    fn draw(
        &self,
        hovered: &Option<Point>,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let dots = self.cache.draw(renderer, bounds.size(), |frame| {
            for dot in &self.dots {
                frame.fill(&Path::circle(*dot, 4.0), Color::BLACK);
            }
            frame.with_save(|frame| {
                frame.translate(Vector::new(10.0, 10.0));
                frame.rotate(0.5);
                frame.stroke(
                    &Path::new(|b| {
                        b.move_to(Point::ORIGIN);
                        b.line_to(Point::new(20.0, 0.0));
                    }),
                    Stroke::default().with_width(2.0).with_line_cap(LineCap::Round),
                );
            });
        });
        let mut frame = Frame::new(renderer, bounds.size());
        if let Some(point) = hovered {
            frame.stroke(&Path::circle(*point, 8.0), Stroke::default().with_color(Color::WHITE));
        }
        vec![dots, frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Option<Point>,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

pub fn main() -> iced::Result {
    iced::run(Dots::update, Dots::view)
}
"#;

#[test]
fn a_canvas_program_draws_and_publishes_its_clicks() {
    let mut adapter = boot(DOTS);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.draw(&iced::Theme::Dark);
    simulator.point_at((40.0, 120.0));
    simulator.simulate(iced_test::simulator::click());
    let messages = simulator.into_messages().collect::<Vec<_>>();
    assert_eq!(messages.len(), 1, "the press publishes one Message");
    for message in messages {
        adapter.update(message).unwrap();
    }
    adapter.refresh_view().unwrap();
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find("1 dots").unwrap();
    simulator.draw(&iced::Theme::Dark);
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}

/// The program as iced's own canvas examples write it: generic over the
/// Message, with only `draw`, and `canvas(self as &Self)`'s `&self`.
const SQUARE: &str = r#"
use iced::mouse;
use iced::widget::{canvas, container};
use iced::{Fill, Point, Rectangle, Renderer, Theme, Widget};

#[derive(Default)]
struct Square;

#[derive(Debug, Clone)]
enum Message {}

impl Square {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        container(canvas(self).width(Fill).height(Fill)).padding(20)
    }
}

impl<Message> canvas::Program<Message> for Square {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let palette = theme.palette();
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), palette.background.strong.color);
        frame.fill_text(canvas::Text {
            content: "square".to_string(),
            position: frame.center(),
            size: 20.0,
            align_x: iced::widget::text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            shaping: iced::widget::text::Shaping::Advanced,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}

pub fn main() -> iced::Result {
    iced::application(Square::default, Square::update, Square::view)
        .theme(Theme::Dark)
        .run()
}
"#;

#[test]
fn a_generic_draw_only_program_and_a_theme_value_check() {
    let adapter = boot(SQUARE);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.draw(&iced::Theme::Dark);
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}


/// bezier_tool's shape: the Program borrows the State and a slice of it.
const BORROWED: &str = r#"
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::{Color, Fill, Point, Rectangle, Renderer, Theme, Widget};

#[derive(Default)]
struct State {
    cache: canvas::Cache,
    dots: Vec<Point>,
}

#[derive(Debug, Clone, Copy)]
enum Message {}

struct Painter<'a> {
    state: &'a State,
    dots: &'a [Point],
}

impl<'a> canvas::Program<Message> for Painter<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let dots = self.state.cache.draw(renderer, bounds.size(), |frame: &mut Frame| {
            for dot in self.dots {
                frame.fill(&Path::circle(*dot, 4.0), Color::BLACK);
            }
        });
        vec![dots]
    }
}

impl State {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        Canvas::new(Painter { state: self, dots: &self.dots }).width(Fill).height(Fill)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}
"#;

#[test]
fn a_program_that_borrows_its_state_draws() {
    let adapter = boot(BORROWED);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.draw(&iced::Theme::Dark);
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}

/// the_matrix's shape: a Program whose State is a `RefCell` of caches,
/// filled on the first draw through `borrow_mut`.
const CELLED: &str = r#"
use std::cell::RefCell;
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Fill, Point, Rectangle, Renderer, Theme, Widget};

#[derive(Default)]
struct Matrix {
    tick: usize,
}

#[derive(Debug, Clone, Copy)]
enum Message {}

impl<Message> canvas::Program<Message> for Matrix {
    type State = RefCell<Vec<canvas::Cache>>;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut caches = state.borrow_mut();
        if caches.is_empty() {
            let group = canvas::Group::unique();
            caches.resize_with(3, || canvas::Cache::with_group(group));
        }
        vec![caches[self.tick % caches.len()].draw(renderer, bounds.size(), |frame| {
            frame.fill_rectangle(Point::ORIGIN, frame.size(), Color::BLACK);
        })]
    }
}

impl Matrix {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl Widget<Message> {
        canvas(self).width(Fill).height(Fill)
    }
}

pub fn main() -> iced::Result {
    iced::application(Matrix::default, Matrix::update, Matrix::view).run()
}
"#;

#[test]
fn a_program_state_in_a_ref_cell_draws() {
    let adapter = boot(CELLED);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.draw(&iced::Theme::Dark);
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.draw(&iced::Theme::Dark);
    assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
}
