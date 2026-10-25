//! Invaders: a space-invaders clone on a small engine of its own.
//!
//! This file is the engine, and it is native Rust. It owns the window and
//! winit's event loop, reads the keys, keeps the game's state and paints a
//! pixel framebuffer through softbuffer. The game itself lives in the
//! modules that start with `// rawr`: the rules in `game.rs`, the
//! invaders' march in `march.rs`, hits and score in `collide.rs`, and what
//! a frame shows in `draw.rs`. Under `rawr run` those are interpreted, and
//! a save reaches the running game at its next frame. `cargo build
//! --release` compiles every file natively.

mod collide;
mod draw;
mod game;
mod march;

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// The time between two frames.
const FRAME: Duration = Duration::from_micros(16_667);

fn main() {
    let event_loop = EventLoop::new().expect("an event loop on the main thread");
    let mut engine = Engine::new();
    event_loop
        .run_app(&mut engine)
        .expect("the event loop runs");
}

/// The window and the surface the framebuffer is shown on.
struct Screen {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
}

struct Engine {
    screen: Option<Screen>,
    game: game::Game,
    /// The keys held now.
    input: game::Input,
    /// The framebuffer, at the size the last frame asked for.
    canvas: Canvas,
    title: String,
    last: Instant,
    next: Instant,
}

impl Engine {
    fn new() -> Engine {
        let now = Instant::now();
        Engine {
            screen: None,
            game: game::Game::new(),
            input: game::Input {
                left: false,
                right: false,
                fire: false,
                restart: false,
            },
            canvas: Canvas::default(),
            title: String::new(),
            last: now,
            next: now,
        }
    }

    fn key(&mut self, code: KeyCode, held: bool, event_loop: &ActiveEventLoop) {
        match code {
            KeyCode::ArrowLeft | KeyCode::KeyA => self.input.left = held,
            KeyCode::ArrowRight | KeyCode::KeyD => self.input.right = held,
            KeyCode::Space => self.input.fire = held,
            KeyCode::KeyR => self.input.restart = held,
            KeyCode::Escape | KeyCode::KeyQ if held => event_loop.exit(),
            _ => {}
        }
    }

    /// One frame: the game moves on by the time since the last one, and
    /// what it draws is painted and shown.
    fn frame(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        self.game.update(&self.input, elapsed);
        let frame = draw::frame(&self.game);
        self.canvas.paint(&frame);
        let Some(screen) = &mut self.screen else {
            return;
        };
        let title = game::title(&self.game);
        if title != self.title {
            screen.window.set_title(&title);
            self.title = title;
        }
        screen.show(&self.canvas, frame.background);
    }
}

impl ApplicationHandler for Engine {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Invaders")
            .with_inner_size(LogicalSize::new(640.0, 520.0));
        let window = Rc::new(event_loop.create_window(attributes).expect("a window"));
        let context = softbuffer::Context::new(window.clone()).expect("a softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("a surface");
        self.screen = Some(Screen { window, surface });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => self.key(code, state == ElementState::Pressed, event_loop),
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next {
            self.next = now + FRAME;
            if let Some(screen) = &self.screen {
                screen.window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
    }
}

/// A framebuffer of `0RGB` pixels.
#[derive(Default)]
struct Canvas {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
}

impl Canvas {
    /// Paint `frame`'s items in order over its background.
    fn paint(&mut self, frame: &draw::Frame) {
        self.width = frame.width as usize;
        self.height = frame.height as usize;
        self.pixels.clear();
        self.pixels
            .resize(self.width * self.height, frame.background);
        for item in &frame.items {
            match item {
                draw::Item::Rect {
                    x,
                    y,
                    width,
                    height,
                    color,
                } => self.fill(*x, *y, *width, *height, *color),
                draw::Item::Sprite {
                    x,
                    y,
                    scale,
                    color,
                    rows,
                } => {
                    // A sprite's row is a line of cells: `.` and space are
                    // clear, and anything else is lit.
                    for (down, row) in rows.iter().enumerate() {
                        for (across, cell) in row.bytes().enumerate() {
                            if cell != b'.' && cell != b' ' {
                                let left = x + across as i32 * scale;
                                let top = y + down as i32 * scale;
                                self.fill(left, top, *scale, *scale, *color);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Fill a rectangle, clipped to the canvas.
    fn fill(&mut self, x: i32, y: i32, width: i32, height: i32, color: u32) {
        let clip = |value: i32, limit: usize| value.clamp(0, limit as i32) as usize;
        let (left, right) = (clip(x, self.width), clip(x + width, self.width));
        let (top, bottom) = (clip(y, self.height), clip(y + height, self.height));
        for row in top..bottom {
            let start = row * self.width;
            self.pixels[start + left..start + right].fill(color);
        }
    }
}

impl Screen {
    /// Show `canvas` scaled to the window: by the largest whole factor that
    /// fits, centred, with `border` around it.
    fn show(&mut self, canvas: &Canvas, border: u32) {
        let size = self.window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if canvas.width == 0 || canvas.height == 0 || self.surface.resize(width, height).is_err() {
            return;
        }
        let Ok(mut buffer) = self.surface.buffer_mut() else {
            return;
        };
        let (screen_width, screen_height) = (size.width as usize, size.height as usize);
        buffer.fill(border);
        let fit = (screen_width as f32 / canvas.width as f32)
            .min(screen_height as f32 / canvas.height as f32);
        let scale = if fit >= 1.0 { fit.floor() } else { fit };
        let shown_width = ((canvas.width as f32 * scale) as usize).min(screen_width);
        let shown_height = ((canvas.height as f32 * scale) as usize).min(screen_height);
        let left = (screen_width - shown_width) / 2;
        let top = (screen_height - shown_height) / 2;
        let mut line = vec![0; shown_width];
        let mut source = usize::MAX;
        for y in 0..shown_height {
            let row = ((y as f32 / scale) as usize).min(canvas.height - 1);
            if row != source {
                let from = &canvas.pixels[row * canvas.width..(row + 1) * canvas.width];
                for (x, pixel) in line.iter_mut().enumerate() {
                    *pixel = from[((x as f32 / scale) as usize).min(canvas.width - 1)];
                }
                source = row;
            }
            let start = (top + y) * screen_width + left;
            buffer[start..start + shown_width].copy_from_slice(&line);
        }
        let _ = buffer.present();
    }
}
