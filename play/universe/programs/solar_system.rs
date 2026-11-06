//! An animated solar system, drawn with plain shapes.
//!
//! iced's solar_system example, with circles in place of its images.
use iced::mouse;
use iced::widget::canvas::stroke::{self, Stroke};
use iced::widget::canvas;
use iced::widget::canvas::{Geometry, Path};
use iced::window;
use iced::{
    Color, Fill, Point, Rectangle, Renderer, Size, Subscription, Theme, Vector, Widget,
};

use std::time::Instant;

pub fn main() -> iced::Result {
    iced::application::timed(
        SolarSystem::new,
        SolarSystem::update,
        SolarSystem::subscription,
        SolarSystem::view,
    )
    .theme(SolarSystem::theme)
    .run()
}

struct SolarSystem {
    state: State,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick,
}

impl SolarSystem {
    fn new() -> Self {
        Self {
            state: State::new(),
        }
    }

    fn update(&mut self, message: Message, now: Instant) {
        match message {
            Message::Tick => self.state.update(now),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        canvas(&self.state).width(Fill).height(Fill)
    }

    fn theme(&self) -> Theme {
        Theme::Moonfly
    }

    fn subscription(&self) -> Subscription<Message> {
        window::frames().map(|_| Message::Tick)
    }
}

#[derive(Debug)]
struct State {
    space_cache: canvas::Cache,
    system_cache: canvas::Cache,
    start: Instant,
    now: Instant,
    stars: Vec<(Point, f32)>,
}

impl State {
    const SUN_RADIUS: f32 = 70.0;
    const ORBIT_RADIUS: f32 = 150.0;
    const EARTH_RADIUS: f32 = 12.0;
    const MOON_RADIUS: f32 = 4.0;
    const MOON_DISTANCE: f32 = 28.0;

    fn new() -> State {
        let now = Instant::now();
        let size = window::Settings::default().size;

        State {
            space_cache: canvas::Cache::default(),
            system_cache: canvas::Cache::default(),
            start: now,
            now,
            stars: Self::generate_stars(size.width, size.height),
        }
    }

    fn update(&mut self, now: Instant) {
        self.start = self.start.min(now);
        self.now = now;
        self.system_cache.clear();
    }

    /// 100 stars from a small xorshift generator, in place of `rand`.
    fn generate_stars(width: f32, height: f32) -> Vec<(Point, f32)> {
        let mut seed: u32 = 0x9E37_79B9;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };

        (0..100)
            .map(|_| {
                (
                    Point::new((next() - 0.5) * width, (next() - 0.5) * height),
                    0.5 + next() * 0.5,
                )
            })
            .collect()
    }
}

impl<Message> canvas::Program<Message> for State {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        use std::f32::consts::PI;

        let background = self.space_cache.draw(renderer, bounds.size(), |frame| {
            frame.fill_rectangle(Point::ORIGIN, frame.size(), Color::BLACK);

            let stars = Path::new(|path| {
                for (p, size) in &self.stars {
                    path.rectangle(*p, Size::new(*size, *size));
                }
            });

            frame.translate(frame.center() - Point::ORIGIN);
            frame.fill(&stars, Color::WHITE);
        });

        let system = self.system_cache.draw(renderer, bounds.size(), |frame| {
            let center = frame.center();
            frame.translate(Vector::new(center.x, center.y));

            let sun = Path::circle(Point::ORIGIN, Self::SUN_RADIUS);
            frame.fill(&sun, Color::from_rgb8(0xF9, 0xB2, 0x2E));

            let orbit = Path::circle(Point::ORIGIN, Self::ORBIT_RADIUS);
            frame.stroke(
                &orbit,
                Stroke {
                    style: stroke::Style::Solid(Color::WHITE.scale_alpha(0.1)),
                    width: 1.0,
                    line_dash: canvas::LineDash {
                        offset: 0,
                        segments: &[3.0, 6.0],
                    },
                    ..Stroke::default()
                },
            );

            let elapsed = self.now - self.start;
            let rotation = (2.0 * PI / 60.0) * elapsed.as_secs() as f32
                + (2.0 * PI / 60_000.0) * elapsed.subsec_millis() as f32;

            frame.rotate(rotation);
            frame.translate(Vector::new(Self::ORBIT_RADIUS, 0.0));

            let earth = Path::circle(Point::ORIGIN, Self::EARTH_RADIUS);
            frame.fill(&earth, Color::from_rgb8(0x3B, 0x82, 0xF6));

            frame.rotate(rotation * 10.0);
            frame.translate(Vector::new(0.0, Self::MOON_DISTANCE));

            let moon = Path::circle(Point::ORIGIN, Self::MOON_RADIUS);
            frame.fill(&moon, Color::from_rgb8(0xC8, 0xC8, 0xC8));
        });

        vec![background, system]
    }
}
