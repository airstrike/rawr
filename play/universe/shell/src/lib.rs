//! ?play's iced shell (#434). iced hands a program to its shell's `run`,
//! and in ?play's universe this crate is that shell in place of
//! iced_winit. It opens no window. It runs the program headless, as
//! iced_test's emulator does, on the same renderer, and draws it as
//! iced_winit would: at the program's own scale factor (#529), with each
//! redraw broadcast to its subscriptions, and its widgets' redraw requests
//! honoured.
//!
//! It waits until the boot and the tasks it starts have finished. From that
//! frame on it keeps drawing, on the program's own clock, while the program
//! still changes: an update, a redraw a widget asked for, a pending task,
//! or a subscription that changed the frame within [`QUIET`]. A program
//! that never changes again is written as a PNG of its frame. One that
//! changed is written as a GIF that plays once and holds its last frame
//! (#527). Either goes to the file [`FRAME`] names, and `run` returns, as
//! iced's does when its window closes.
//!
//! A GIF plays at the program's pace, not the machine's (#889): a frame the
//! window's redraws changed is shown for [`INTERVAL`] a redraw, however
//! long it took to draw, and one the program's own clock changed (a timer,
//! a task) for the time that clock ran.
//!
//! Its public items are the ones iced takes from iced_winit.

pub use iced_debug as debug;
pub use iced_program as program;
pub use iced_runtime as runtime;
pub use program::core;
pub use program::graphics;
pub use runtime::futures;

use crate::core::backend;
use crate::core::mouse;
use crate::core::renderer::{self, Headless as _};
use crate::core::shell;
use crate::core::theme;
use crate::core::time::Instant;
use crate::core::widget;
use crate::core::window;
use crate::core::{Point, Renderer as _, Size};
use crate::futures::futures::channel::mpsc;
use crate::futures::futures::{executor, StreamExt};
use crate::futures::subscription;
use crate::futures::{Executor as _, Runtime};
use crate::program::Program;
use crate::runtime::user_interface::{self, UserInterface};
use crate::runtime::{task, Action, Task};

use std::sync::atomic::{self, AtomicBool};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The variable naming the file the frame is written to.
pub const FRAME: &str = "PLAY_FRAME";
/// The variable naming the run's deadline, in milliseconds since the Unix
/// epoch: the last frame is drawn and written before it.
pub const DEADLINE: &str = "PLAY_DEADLINE";
/// The variable naming a file that may give a sooner deadline, in the same
/// form, as the run goes on: the bot writes one there once another snippet
/// waits. It is read at each frame.
pub const DEADLINE_FILE: &str = "PLAY_DEADLINE_FILE";

/// The time between two frames: 20 frames a second. A GIF's delays are in
/// hundredths of a second, and browsers slow any delay under two of them to
/// ten, so 50 ms is a delay every viewer plays at its own speed. It also
/// leaves the headless renderer time to draw each frame.
pub const INTERVAL: Duration = Duration::from_millis(50);
/// The most a GIF shows: 100 frames, five seconds at [`INTERVAL`] on a
/// machine that draws each within it.
pub const FRAMES: usize = 100;
/// How long a program with subscriptions and nothing else pending may stay
/// unchanged before its frame counts as settled.
pub const QUIET: Duration = Duration::from_secs(1);
/// Time kept before the deadline to write the frame and exit.
const FINISH: Duration = Duration::from_millis(200);
/// How long a GIF holds its last frame, for a viewer that loops it anyway.
const HOLD: Duration = Duration::from_secs(3);
/// A GIF stops taking frames once it has this many bytes, so the last one
/// still fits under Discord's 8 MiB.
const GIF_BYTES: usize = 6 * 1024 * 1024;

/// An error that stopped a program, as iced_winit names them.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The futures executor could not be created.
    #[error("the futures executor could not be created")]
    ExecutorCreationFailed(futures::futures::io::Error),

    /// The application window could not be created: here, its frame could
    /// not be written.
    #[error("the application window could not be created")]
    WindowCreationFailed(std::io::Error),

    /// The application graphics context could not be created.
    #[error("the application graphics context could not be created")]
    GraphicsCreationFailed(backend::Error),
}

impl From<backend::Error> for Error {
    fn from(error: backend::Error) -> Error {
        Error::GraphicsCreationFailed(error)
    }
}

/// Runs a [`Program`] until it settles, and writes what it drew.
pub fn run<P>(program: P) -> Result<(), Error>
where
    P: Program + 'static,
{
    let Some(path) = std::env::var_os(FRAME) else {
        return Err(Error::WindowCreationFailed(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no display",
        )));
    };
    let mut deadline = Deadline::from_env();
    let mut shell = Shell::new(&program)?;
    shell.settle(&program);
    let mut film = Film::default();
    let mut next = Instant::now();
    let mut clock = Clock::new(next);
    loop {
        shell.updated = false;
        let drawn = Instant::now();
        let at = clock.frame(drawn, std::mem::take(&mut shell.timed));
        let (frame, redraw) = shell.redraw(&program);
        let pushed = film.push(frame, at).map_err(Error::WindowCreationFailed)?;
        let now = Instant::now();
        // Quiet counts from the end of a changed frame: drawing it is the
        // shell's time, not the program's, however slow the machine.
        if pushed {
            shell.changed = now;
            clock.changed(drawn);
        }
        let step = now - drawn;
        let late = deadline
            .at(now)
            .is_some_and(|at| now + step * 2 + INTERVAL + FINISH >= at);
        if film.full() || shell.exited || late {
            break;
        }
        // What arrived while the frame was drawn, such as the message a
        // subscription made of its redraw.
        shell.wait(&program, now);
        // Whether the program may still change: what it did since this
        // frame began, and what it has pending. A program with
        // subscriptions may change when one fires, until it has been
        // unchanged for [`QUIET`].
        let busy = shell.updated
            || shell.pending > 0
            || !matches!(redraw, window::RedrawRequest::Wait)
            || (shell.subscribed && shell.changed.elapsed() < QUIET);
        if !busy {
            break;
        }
        next = (next + INTERVAL).max(now);
        shell.wait(&program, next);
    }
    let bytes = film.finish().map_err(Error::WindowCreationFailed)?;
    std::fs::write(path, bytes).map_err(Error::WindowCreationFailed)
}

/// The GIF's clock: when each frame shows, in the time the program ran
/// in rather than the time the machine took to draw it (#889).
///
/// A redraw is a frame of the program's window, so it advances the clock by
/// [`INTERVAL`], as a window that draws each frame in time would. A program
/// that counts frames, or a widget that animates a step a redraw, then plays
/// at its own pace on a machine that takes seconds to draw each one. Once a
/// message comes on the program's own clock (a timer, a task), the frame it
/// changes shows at the wall time since the last change instead: that clock
/// ran on through the draws, and its ticks say when the change was due.
///
/// The program itself still sees the machine's time, in its redraw events
/// and through `Instant::now`: a widget compares one with the other, and a
/// clock of the shell's own would stop one that started after it fell
/// behind.
struct Clock {
    /// When the last frame shows.
    at: Instant,
    /// When the last changed frame was drawn, and when it shows.
    changed: (Instant, Instant),
    /// Whether a message came on the program's clock since that change.
    timed: bool,
}

impl Clock {
    fn new(now: Instant) -> Self {
        Self {
            at: now,
            changed: (now, now),
            timed: false,
        }
    }

    /// When the frame drawn at `drawn` shows, given whether a message came
    /// on the program's own clock since the last frame.
    fn frame(&mut self, drawn: Instant, timed: bool) -> Instant {
        self.timed |= timed;
        let (wall, shown) = self.changed;
        self.at = if self.timed {
            (shown + (drawn - wall)).max(self.at)
        } else {
            self.at + INTERVAL
        };
        self.at
    }

    /// The last frame, drawn at `drawn`, changed.
    fn changed(&mut self, drawn: Instant) {
        self.changed = (drawn, self.at);
        self.timed = false;
    }
}

/// The run's deadline: the one [`DEADLINE`] gives, or the sooner one the
/// file [`DEADLINE_FILE`] names once that file gives one. A run with
/// neither has none: [`FRAMES`] and [`QUIET`] end it, counted in frames
/// and in the program's own time, so a slow machine draws the frames a
/// fast one does, later.
struct Deadline {
    fixed: Option<Instant>,
    file: Option<std::path::PathBuf>,
    set: Option<Instant>,
}

impl Deadline {
    fn from_env() -> Self {
        let now = Instant::now();
        Self {
            fixed: std::env::var(DEADLINE)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .map(|millis| instant(millis, now)),
            file: std::env::var_os(DEADLINE_FILE).map(std::path::PathBuf::from),
            set: None,
        }
    }

    /// The deadline at `now`, reading the file while it gives none.
    fn at(&mut self, now: Instant) -> Option<Instant> {
        if self.set.is_none()
            && let Some(file) = &self.file
        {
            self.set = std::fs::read_to_string(file)
                .ok()
                .and_then(|text| text.trim().parse::<u64>().ok())
                .map(|millis| instant(millis, now));
        }
        match (self.set, self.fixed) {
            (Some(set), Some(fixed)) => Some(set.min(fixed)),
            (set, fixed) => set.or(fixed),
        }
    }
}

/// `millis` since the Unix epoch on the monotonic clock, as seen at `now`.
fn instant(millis: u64, now: Instant) -> Instant {
    let at = UNIX_EPOCH + Duration::from_millis(millis);
    at.duration_since(SystemTime::now())
        .map_or(now, |left| now + left)
}

/// What the program's tasks and subscriptions send the shell.
enum Event<Message> {
    /// An action to perform. A message among them came on the program's
    /// own clock: a task's, or a subscription's that doesn't listen to the
    /// window.
    Action(Action<Message>),
    /// A message a subscription made of the window's events, such as a
    /// redraw.
    Window(Message),
    /// A task finished.
    Done,
}

/// The program's state, its runtime, and its one headless window.
struct Shell<P: Program> {
    state: P::State,
    runtime: Runtime<P::Executor, mpsc::Sender<Event<P::Message>>, Event<P::Message>>,
    receiver: mpsc::Receiver<Event<P::Message>>,
    renderer: P::Renderer,
    window: window::Id,
    size: Size,
    cache: Option<user_interface::Cache>,
    /// Events the program's tasks sent its window.
    events: Vec<core::Event>,
    /// Tasks still running.
    pending: usize,
    /// Whether the program has any subscription.
    subscribed: bool,
    /// Whether the program updated since the last frame.
    updated: bool,
    /// Whether a message came on the program's own clock since the last
    /// frame.
    timed: bool,
    /// When the frame last changed.
    changed: Instant,
    /// Whether the program asked to exit or closed its window.
    exited: bool,
}

impl<P: Program + 'static> Shell<P> {
    fn new(program: &P) -> Result<Self, Error> {
        let settings = program.settings();
        for font in &settings.fonts {
            graphics::text::font_system()
                .write()
                .expect("Write to font system")
                .load_font(font.clone());
        }
        let executor = P::Executor::new().map_err(Error::ExecutorCreationFailed)?;
        // The image has no GPU: iced_test's variable picks tiny-skia.
        let backend = std::env::var("ICED_TEST_BACKEND").ok();
        let renderer = executor
            .block_on(P::Renderer::new(
                renderer::Settings::from(&settings),
                backend.as_deref(),
            ))
            .ok_or_else(|| {
                Error::GraphicsCreationFailed(backend::Error::BackendError(
                    "no headless renderer".to_owned(),
                ))
            })?;
        let (sender, receiver) = mpsc::channel(100);
        let runtime = Runtime::new(executor, sender);
        let (state, task) = runtime.enter(|| program.boot());
        let mut shell = Self {
            state,
            runtime,
            receiver,
            renderer,
            window: window::Id::unique(),
            size: program.window().unwrap_or_default().size,
            cache: Some(user_interface::Cache::default()),
            events: Vec::new(),
            pending: 0,
            subscribed: false,
            updated: false,
            timed: false,
            changed: Instant::now(),
            exited: false,
        };
        shell.resubscribe(program);
        shell.spawn(task);
        Ok(shell)
    }

    /// Perform actions until the boot's tasks, and the tasks they start,
    /// have finished.
    fn settle(&mut self, program: &P) {
        while self.pending > 0 && !self.exited {
            match executor::block_on(self.receiver.next()) {
                Some(event) => self.perform(program, event),
                None => break,
            }
        }
    }

    /// Perform what arrives until `until`.
    fn wait(&mut self, program: &P, until: Instant) {
        loop {
            while let Ok(event) = self.receiver.try_recv() {
                self.perform(program, event);
            }
            let now = Instant::now();
            if now >= until || self.exited {
                return;
            }
            std::thread::sleep((until - now).min(Duration::from_millis(5)));
        }
    }

    /// Draw a frame, as iced_winit does on a redraw: the screenshot, and
    /// when the program's widgets want the next one.
    fn redraw(&mut self, program: &P) -> (window::Screenshot, window::RedrawRequest) {
        let scale_factor = program.scale_factor(&self.state, self.window);
        let theme = program
            .theme(&self.state, self.window)
            .unwrap_or_else(|| <P::Theme as theme::Base>::default(theme::Mode::None));
        let style = program.style(&self.state, &theme);
        let now = Instant::now();
        let mut events = std::mem::take(&mut self.events);
        events.push(core::Event::Window(window::Event::RedrawRequested(now)));
        let mut messages = shell::Bus::new();
        let mut interface = UserInterface::build(
            program.view(&self.state, self.window),
            self.size,
            self.cache.take().unwrap_or_default(),
            &mut self.renderer,
        );
        let (state, _) = interface.update(
            &window::Headless,
            &shell::Waker::noop(),
            &events,
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut messages,
        );
        interface.draw(
            &mut self.renderer,
            &theme,
            &renderer::Style {
                text_color: style.text_color,
            },
            mouse::Cursor::Unavailable,
        );
        self.cache = Some(interface.into_cache());
        // The window is the program's size in logical pixels, as iced_winit
        // opens it, and the frame is its physical pixels.
        let physical = Size::new(
            (self.size.width * scale_factor).round() as u32,
            (self.size.height * scale_factor).round() as u32,
        );
        let rgba = self
            .renderer
            .screenshot(physical, scale_factor, style.background_color);
        self.runtime.broadcast(subscription::Event::Interaction {
            window: self.window,
            event: core::Event::Window(window::Event::RedrawRequested(now)),
            status: core::event::Status::Ignored,
        });
        let redraw = match state {
            user_interface::State::Updated { redraw_request, .. } => redraw_request,
            user_interface::State::Outdated => window::RedrawRequest::NextFrame,
        };
        for message in messages {
            self.update(program, message);
        }
        (
            window::Screenshot {
                rgba: rgba.into(),
                size: physical,
                scale_factor,
            },
            redraw,
        )
    }

    fn update(&mut self, program: &P, message: P::Message) {
        let task = self
            .runtime
            .enter(|| program.update(&mut self.state, message));
        self.updated = true;
        self.resubscribe(program);
        self.spawn(task);
    }

    /// Run `task`, counting it until it finishes.
    fn spawn(&mut self, task: Task<P::Message>) {
        if let Some(stream) = task::into_stream(task) {
            self.pending += 1;
            self.runtime.run(
                stream
                    .map(Event::Action)
                    .chain(futures::futures::stream::once(async { Event::Done }))
                    .boxed(),
            );
        }
    }

    fn resubscribe(&mut self, program: &P) {
        let recipes =
            subscription::into_recipes(self.runtime.enter(|| program.subscription(&self.state)));
        self.subscribed = !recipes.is_empty();
        self.runtime.track(recipes.into_iter().map(|recipe| {
            Box::new(Source { recipe }) as Box<dyn subscription::Recipe<Output = _>>
        }));
    }

    fn perform(&mut self, program: &P, event: Event<P::Message>) {
        let action = match event {
            Event::Done => {
                self.pending = self.pending.saturating_sub(1);
                return;
            }
            Event::Window(message) => return self.update(program, message),
            Event::Action(action) => action,
        };
        match action {
            Action::Output(message) => {
                self.timed = true;
                self.update(program, message);
            }
            Action::Widget(operation) => {
                let mut interface = UserInterface::build(
                    program.view(&self.state, self.window),
                    self.size,
                    self.cache.take().unwrap_or_default(),
                    &mut self.renderer,
                );
                let mut operation = Some(operation);
                while let Some(mut current) = operation.take() {
                    interface.operate(&self.renderer, &mut current);
                    if let widget::operation::Outcome::Chain(next) = current.finish() {
                        operation = Some(next);
                    }
                }
                self.cache = Some(interface.into_cache());
                self.updated = true;
            }
            Action::Window(action) => self.window_action(action),
            Action::Font(runtime::font::Action::Load { bytes, channel }) => {
                graphics::text::font_system()
                    .write()
                    .expect("Write to font system")
                    .load_font(bytes);
                let _ = channel.send(Ok(()));
            }
            Action::Event { window, event } if window == self.window => {
                self.events.push(event);
            }
            Action::Tick => self.renderer.tick(),
            Action::Exit => self.exited = true,
            // A window with no clipboard, system or image cache to ask.
            _ => {}
        }
    }

    fn window_action(&mut self, action: runtime::window::Action) {
        use runtime::window::Action;
        match action {
            Action::Open(id, _settings, sender) => {
                self.window = id;
                let _ = sender.send(id);
            }
            Action::Close(id) if id == self.window => self.exited = true,
            Action::GetOldest(sender) | Action::GetLatest(sender) => {
                let _ = sender.send(Some(self.window));
            }
            Action::GetSize(id, sender) if id == self.window => {
                let _ = sender.send(self.size);
            }
            Action::GetMaximized(id, sender) if id == self.window => {
                let _ = sender.send(false);
            }
            Action::GetMinimized(id, sender) if id == self.window => {
                let _ = sender.send(None);
            }
            Action::GetPosition(id, sender) if id == self.window => {
                let _ = sender.send(Some(Point::ORIGIN));
            }
            // The display's own scale: iced_winit's window reports it, and
            // the program's scale_factor multiplies it.
            Action::GetScaleFactor(id, sender) if id == self.window => {
                let _ = sender.send(1.0);
            }
            Action::GetMode(id, sender) if id == self.window => {
                let _ = sender.send(window::Mode::Windowed);
            }
            _ => {}
        }
    }
}

/// A subscription's recipe whose messages say where they came from: of the
/// window's events, once its stream has taken one, or of the program's own
/// clock, such as `time::every`'s, which never takes one.
struct Source<Message> {
    recipe: Box<dyn subscription::Recipe<Output = Message>>,
}

impl<Message: futures::MaybeSend + 'static> subscription::Recipe for Source<Message> {
    type Output = Event<Message>;

    fn hash(&self, state: &mut subscription::Hasher) {
        self.recipe.hash(state);
    }

    fn stream(
        self: Box<Self>,
        input: subscription::EventStream,
    ) -> futures::BoxStream<Self::Output> {
        let listened = Arc::new(AtomicBool::new(false));
        let input = futures::boxed_stream(input.inspect({
            let listened = Arc::clone(&listened);
            move |_| listened.store(true, atomic::Ordering::Relaxed)
        }));
        futures::boxed_stream(self.recipe.stream(input).map(move |message| {
            if listened.load(atomic::Ordering::Relaxed) {
                Event::Window(message)
            } else {
                Event::Action(Action::Output(message))
            }
        }))
    }
}

/// The frames drawn so far: a PNG while there is one, and a GIF from the
/// second. Each GIF frame is the rectangle that changed, written once the
/// next frame gives its delay.
#[derive(Default)]
struct Film {
    /// The last frame's pixels and size.
    last: Option<window::Screenshot>,
    /// The last frame's changed rectangle, its pixels, and when it shows.
    pending: Option<(Rect, Vec<u8>, Instant)>,
    gif: Option<gif::Encoder<Vec<u8>>>,
    /// Frames that differ from the one before.
    frames: usize,
    /// Whether a frame came at another size: one GIF has one size.
    resized: bool,
}

#[derive(Clone, Copy)]
struct Rect {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

impl Film {
    /// Add a frame that shows at `at`: whether it differs from the last one.
    fn push(&mut self, frame: window::Screenshot, at: Instant) -> std::io::Result<bool> {
        let rect = match &self.last {
            None => Rect {
                left: 0,
                top: 0,
                width: frame.size.width,
                height: frame.size.height,
            },
            // A frame of another size can't follow in the same GIF.
            Some(last) if last.size != frame.size => {
                self.resized = true;
                return Ok(false);
            }
            Some(last) => match changed(&last.rgba, &frame.rgba, frame.size.width) {
                Some(rect) => rect,
                None => return Ok(false),
            },
        };
        if let Some(pending) = self.pending.take() {
            let delay = at - pending.2;
            self.write(pending, delay)?;
        }
        self.pending = Some((rect, crop(&frame.rgba, frame.size.width, rect), at));
        self.last = Some(frame);
        self.frames += 1;
        Ok(true)
    }

    /// Whether it takes no more frames: [`FRAMES`] of them, [`GIF_BYTES`]
    /// of GIF, or a frame of another size.
    fn full(&self) -> bool {
        self.frames >= FRAMES
            || self.resized
            || self
                .gif
                .as_ref()
                .is_some_and(|gif| gif.get_ref().len() >= GIF_BYTES)
    }

    /// Write `pending`, shown for `delay`, starting the GIF at the first.
    fn write(&mut self, pending: (Rect, Vec<u8>, Instant), delay: Duration) -> std::io::Result<()> {
        let (rect, mut pixels, _) = pending;
        let other = |error: gif::EncodingError| std::io::Error::other(error);
        let size = self.last.as_ref().map_or(Size::new(0, 0), |last| last.size);
        let gif = match &mut self.gif {
            Some(gif) => gif,
            None => {
                // No loop extension: it plays once and holds its last frame.
                let gif = gif::Encoder::new(
                    Vec::new(),
                    dimension(size.width)?,
                    dimension(size.height)?,
                    &[],
                )
                .map_err(other)?;
                self.gif.insert(gif)
            }
        };
        let mut frame = gif::Frame::from_rgba_speed(
            dimension(rect.width)?,
            dimension(rect.height)?,
            &mut pixels,
            10,
        );
        frame.left = dimension(rect.left)?;
        frame.top = dimension(rect.top)?;
        frame.dispose = gif::DisposalMethod::Keep;
        frame.delay = u16::try_from(delay.as_millis().div_ceil(10))
            .unwrap_or(u16::MAX)
            .max(2);
        gif.write_frame(&frame).map_err(other)
    }

    /// The PNG of the only frame, or the finished GIF.
    fn finish(mut self) -> std::io::Result<Vec<u8>> {
        let Some(last) = self.last.take() else {
            return Err(std::io::Error::other("no frame was drawn"));
        };
        let pending = self.pending.take();
        match (self.gif.is_some(), pending) {
            (true, Some(pending)) => {
                self.last = Some(last);
                self.write(pending, HOLD)?;
                self.gif
                    .take()
                    .expect("the GIF was started")
                    .into_inner()
                    .map_err(std::io::Error::other)
            }
            _ => png(&last),
        }
    }
}

fn dimension(value: u32) -> std::io::Result<u16> {
    u16::try_from(value).map_err(|_| std::io::Error::other("the frame is too large for a GIF"))
}

/// The smallest rectangle holding every pixel that differs, or `None` when
/// none does.
fn changed(before: &[u8], after: &[u8], width: u32) -> Option<Rect> {
    let width = width as usize;
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
    for (y, (a, b)) in before
        .chunks_exact(width * 4)
        .zip(after.chunks_exact(width * 4))
        .enumerate()
    {
        if a == b {
            continue;
        }
        let first = a
            .chunks_exact(4)
            .zip(b.chunks_exact(4))
            .position(|(a, b)| a != b)
            .expect("the rows differ");
        let last = width
            - 1
            - a.chunks_exact(4)
                .rev()
                .zip(b.chunks_exact(4).rev())
                .position(|(a, b)| a != b)
                .expect("the rows differ");
        left = left.min(first);
        right = right.max(last);
        top = top.min(y);
        bottom = y;
    }
    (top != usize::MAX).then(|| Rect {
        left: left as u32,
        top: top as u32,
        width: (right - left + 1) as u32,
        height: (bottom - top + 1) as u32,
    })
}

/// The pixels of `rect` in an image `width` pixels wide.
fn crop(rgba: &[u8], width: u32, rect: Rect) -> Vec<u8> {
    let (width, left, right) = (
        width as usize * 4,
        rect.left as usize * 4,
        (rect.left + rect.width) as usize * 4,
    );
    rgba.chunks_exact(width)
        .skip(rect.top as usize)
        .take(rect.height as usize)
        .flat_map(|row| &row[left..right])
        .copied()
        .collect()
}

fn png(screenshot: &window::Screenshot) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, screenshot.size.width, screenshot.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&screenshot.rgba)
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)?;
    Ok(bytes)
}
