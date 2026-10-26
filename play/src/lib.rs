//! Run untrusted Rawr snippets: render an iced snippet to PNG, or to a GIF
//! when a full app's view changes over time, or run a plain program and
//! return its output as text.
//!
//! [`run`] parses a snippet once and asks rawr-iced's own claims (its
//! manifest) whether it is iced's: it names `iced`, or uses a name of the
//! prelude `?play` imports. A plain program runs on the base language (see
//! [`plain`]); an iced one goes to [`render`].
//!
//! [`render`] accepts one of three snippet shapes, wraps it into a Rawr
//! program with a single `#[preview]`, checks it with the canonical iced
//! facade, and renders that preview headlessly with the same pinned
//! tiny-skia renderer and font as `rawr snapshot`, at 2x by default.
//!
//! - a bare expression: `column![text("hi"), button("go")]`
//! - a stateless `fn view() -> impl Widget<()>`
//! - a full app: a type with `fn view(&self)` and `fn update(&mut self, ..)`
//!
//! A full app runs on a simulated clock (see [`motion`]): a view that
//! changes, after its boot or a message, renders as a GIF.
//!
//! A source that declares its own `#[preview]` is refused: its size would
//! bypass [`MAX_SIDE`].
//!
//! Snippets are untrusted. Every render runs on a fresh Engine with bounded
//! operations, call depth, data sizes, and a wall-clock deadline; the iced
//! facade exposes no filesystem or network functions to scripts.

pub mod budget;
#[cfg(feature = "bot")]
pub mod channels;
pub mod click;
pub mod message;
pub mod motion;
pub mod plain;
pub mod server;
#[cfg(feature = "bot")]
pub mod stats;
#[cfg(feature = "bot")]
pub mod web;

use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub use motion::{Action, Gif, Image, Picture};
pub use plain::Output;
use rawr_iced::snapshot::Deadline;
pub use rawr_iced::snapshot::Step;

/// Set to stop an animation's recording before its next frame. The
/// `rawr-play` binary sets it on `SIGUSR1`, which the bot sends when
/// someone else is waiting for a render that has run past the normal
/// animation budget.
pub static CUT: AtomicBool = AtomicBool::new(false);

/// A rendered PNG.
pub type Png = rawr_iced::snapshot::Png;

/// Image format of a reply.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Format {
    /// Lossless PNG, as rendered.
    #[default]
    Png,
    /// Baseline JPEG at quality 90.
    Jpeg,
    /// Lossless WebP.
    Webp,
}

impl Format {
    /// File extension, without the dot.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

impl std::str::FromStr for Format {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "webp" => Ok(Self::Webp),
            _ => Err(format!(
                "unknown format `{value}`; expected png, jpg or webp"
            )),
        }
    }
}

/// Encode a render in `format`.
pub fn encode(png: &Png, format: Format) -> Result<Vec<u8>, PlayError> {
    if format == Format::Png {
        return Ok(png.bytes.clone());
    }
    let failed = |error: &dyn fmt::Display| PlayError::Render(format!("cannot encode: {error}"));
    let mut reader = png::Decoder::new(std::io::Cursor::new(&png.bytes))
        .read_info()
        .map_err(|error| failed(&error))?;
    let mut rgba = vec![0; reader.output_buffer_size().unwrap_or_default()];
    reader
        .next_frame(&mut rgba)
        .map_err(|error| failed(&error))?;
    let (width, height) = (png.width, png.height);
    let mut bytes = Vec::new();
    match format {
        Format::Png => unreachable!(),
        Format::Jpeg => {
            let side = |side: u32| u16::try_from(side).map_err(|error| failed(&error));
            jpeg_encoder::Encoder::new(&mut bytes, 90)
                .encode(
                    &rgba,
                    side(width)?,
                    side(height)?,
                    jpeg_encoder::ColorType::Rgba,
                )
                .map_err(|error| failed(&error))?;
        }
        Format::Webp => image_webp::WebPEncoder::new(&mut bytes)
            .encode(&rgba, width, height, image_webp::ColorType::Rgba8)
            .map_err(|error| failed(&error))?,
    }
    Ok(bytes)
}

/// Built-in theme used for the render.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    /// iced's `Theme::Light`.
    Light,
    /// iced's `Theme::Dark`.
    #[default]
    Dark,
}

impl Theme {
    const fn variant(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

impl std::str::FromStr for Theme {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(format!("unknown theme `{value}`; expected dark or light")),
        }
    }
}

/// Per-render resource limits for untrusted snippets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum script operations per evaluation (boot, update, view).
    pub operations: u64,
    /// Maximum call depth; never raised above the engine's default.
    pub call_depth: usize,
    /// Maximum string length in bytes.
    pub string_size: usize,
    /// Maximum array length.
    pub array_size: usize,
    /// Maximum object-map size.
    pub map_size: usize,
    /// Wall-clock budget for script evaluation in one render.
    pub time: Duration,
    /// Wall-clock budget for a full app's render once its view changes:
    /// an animation's longer budget. Below `time`, it is `time`.
    pub animation: Duration,
}

impl Limits {
    /// The wall-clock budget an animation has: [`Limits::animation`], and
    /// never less than [`Limits::time`].
    #[must_use]
    pub fn animation_budget(&self) -> Duration {
        self.animation.max(self.time)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            operations: 1_000_000,
            call_depth: 32,
            string_size: 64 * 1024,
            // A 128 by 128 RGBA pixel buffer. The files the playground's
            // examples include (`rawr_play_sandbox::bundle`) are left out of
            // it, as the program's text is.
            array_size: 64 * 1024,
            // One rule for every collection: a `HashMap` already counts
            // against the array limit, so maps get the same 65,536 items.
            map_size: 64 * 1024,
            time: Duration::from_secs(2),
            animation: Duration::from_secs(2),
        }
    }
}

/// Render options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// Logical `(width, height)`; `None` fits the image to the content.
    pub size: Option<(u32, u32)>,
    /// Pixels per logical pixel; lowered so no side passes [`MAX_SIDE`].
    pub scale: u32,
    /// Theme to render with.
    pub theme: Theme,
    /// Messages applied, in order, to a full app's default State before the
    /// view renders, e.g. `Message::Increment`.
    pub messages: Vec<String>,
    /// Clicks on a full app after its messages, in order: each the index
    /// of a target in the reading order of the view as it then is (see
    /// [`Image::actions`]).
    pub steps: Vec<Step>,
    /// Per-render limits.
    pub limits: Limits,
    /// Render the last frame as a still image, never an animation.
    pub still: bool,
}

/// Parse `WxH` (e.g. `400x300`) into a size.
pub fn parse_size(value: &str) -> Result<(u32, u32), String> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("size `{value}` must look like 400x300"))?;
    let side = |text: &str| {
        text.trim()
            .parse::<u32>()
            .map_err(|_| format!("size `{value}` must look like 400x300"))
    };
    Ok((side(width)?, side(height)?))
}

/// Largest accepted width or height.
pub const MAX_SIDE: u32 = 2048;

/// Logical size used on an axis an auto-fit snippet fills (`Fill` width).
pub const FALLBACK_SIZE: (u32, u32) = (480, 360);

/// Background margin around an auto-fit snippet, in logical pixels.
pub const MARGIN: u32 = 12;

impl Default for Options {
    fn default() -> Self {
        Self {
            size: None,
            scale: 2,
            theme: Theme::default(),
            messages: Vec::new(),
            steps: Vec::new(),
            limits: Limits::default(),
            still: false,
        }
    }
}

/// Why a snippet did not render.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlayError {
    /// Options were rejected before compiling.
    Options(String),
    /// The snippet did not check as Rawr; carries rendered diagnostics.
    Compile(String),
    /// The snippet checked but failed while evaluating or rendering.
    Render(String),
    /// The snippet ran past a limit (operations, depth, size, or time).
    Limit(String),
}

impl PlayError {
    /// The human-readable text, suitable for a reply code block.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Options(text) | Self::Compile(text) | Self::Render(text) | Self::Limit(text) => {
                text
            }
        }
    }
}

impl fmt::Display for PlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text())
    }
}

impl std::error::Error for PlayError {}

use rawr_snippet::{app_theme, has_top_level_mod, without_positions, without_tests, MESSAGE};
pub use rawr_snippet::{shape, Shape, SOURCE_NAME};

/// What a snippet gave: an iced snippet's image, or a plain program's
/// output.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// The rendered image.
    Image(Image),
    /// The text a plain program wrote, and how it ended.
    Text(Output),
}

/// What brings a snippet to rawr-iced: its manifest's claims.
#[must_use]
pub fn iced_claims() -> &'static rawr::layer::Claims {
    static CLAIMS: OnceLock<rawr::layer::Claims> = OnceLock::new();
    CLAIMS.get_or_init(|| {
        rawr_cli::layer::Manifest::parse(rawr_iced::MANIFEST)
            .expect("rawr-iced's manifest is one rawr reads")
            .claims
    })
}

/// Why a snippet reads no file: it runs on a server whose files are not
/// its own. Each image or svg path is missing, and `include_str!` and
/// `include_bytes!` fail with this.
const NO_FILES: &str = "?play has no files";

/// A snippet's files: none of the server's, only the playground examples'
/// shipped assets (`rawr_play_sandbox::bundle`). rand is seeded the same
/// for every render, as its clock is frozen the same, so a snippet's frames
/// repeat.
fn no_files() {
    rawr::resolve::deny_files(NO_FILES);
    rawr_play_sandbox::bundle::install();
    rawr_play_sandbox::rand::seed(rawr_play_sandbox::rand::RENDER_SEED);
}

/// Run one snippet: render it when rawr-iced claims it, else run it as a
/// plain program.
pub fn run(source: &str, options: &Options) -> Result<Outcome, PlayError> {
    run_claimed(source, options, iced_claims())
}

/// [`run`], with `claims` deciding what is iced's.
pub fn run_claimed(
    source: &str,
    options: &Options,
    claims: &rawr::layer::Claims,
) -> Result<Outcome, PlayError> {
    validate(options)?;
    no_files();
    let deadline = Instant::now() + options.limits.time;
    let parsed = plain::Parsed::new(source, &options.limits, deadline)?;
    if parsed.claimed(claims) {
        drop(parsed);
        return animate(source, options).map(Outcome::Image);
    }
    // Messages drive an iced app; a plain program runs without them.
    let mut output = match plain::async_main(&without_tests(source)) {
        Some(main) => {
            drop(parsed);
            plain::run_async(source, &main, &options.limits, deadline)?
        }
        None => parsed.run(source, &options.limits)?,
    };
    if !options.messages.is_empty() || !options.steps.is_empty() {
        let note = output
            .note
            .take()
            .map(|note| format!("{note}\n"))
            .unwrap_or_default();
        output.note = Some(format!("{note}{}", plain::MESSAGES_IGNORED));
    }
    Ok(Outcome::Text(output))
}

/// Check, evaluate, and render one iced snippet: a full app on a simulated
/// clock, as [`motion`] decides, and anything else (or any snippet when
/// [`Options::still`] is set) as a still PNG.
pub fn animate(source: &str, options: &Options) -> Result<Image, PlayError> {
    if options.still || shape(source) != Shape::App {
        return still(source, options);
    }
    validate(options)?;
    no_files();
    let program = wrap(source, options)?;
    let start = Instant::now();
    let deadline = Deadline::new(start + options.limits.time);
    let canvas = canvas(options);
    // The app boots on the simulated clock, so the times it reads are on it.
    let _clock = rawr_iced::snapshot::clock::freeze();
    let (mut adapter, messages) = application(&program, source, options, deadline.clone())?;
    let theme = app_theme(source).is_none().then(|| options.theme.variant());
    let bound = options.size.unwrap_or((MAX_SIDE, MAX_SIDE));
    // Still images keep `time`. Once the view changes, the recording and
    // the app's scripts share the animation's budget.
    let budget = options.limits.animation_budget();
    let motion = rawr_iced::snapshot::Motion {
        step: motion::STEP,
        frames: motion::FRAMES,
        phase: motion::PHASE,
        span: motion::SPAN,
        deadline,
        extended: start + budget,
        cut: Some(&CUT),
        animation_scale: Some(motion::SCALE),
        still: false,
    };
    let mut loops = motion::Loops::default();
    let mut encoder = motion::Encoder::new(motion::SCALE);
    let recording = rawr_iced::snapshot::record_application(
        &mut adapter,
        messages,
        &options.steps,
        canvas,
        bound,
        theme,
        motion,
        &mut |frames| loops.check(frames),
        &mut |frames| encoder.push(frames),
    )
    .map_err(|error| classify(&error.to_string(), &options.limits))?;
    // A cut recording stopped before its budget ran out.
    let stopped = if recording.ending == rawr_iced::snapshot::Ending::Cut {
        start.elapsed()
    } else {
        budget
    };
    motion::picture(recording, &loops, stopped, encoder)
}

/// How a render sizes and scales its image.
fn canvas(options: &Options) -> rawr_iced::snapshot::Canvas {
    rawr_iced::snapshot::Canvas {
        fit: options.size.is_none(),
        fallback: FALLBACK_SIZE,
        margin: if options.size.is_none() { MARGIN } else { 0 },
        scale: options.scale as f32,
        max_pixels: MAX_SIDE,
    }
}

/// Check, evaluate, and render one iced snippet to a PNG: a full app once
/// its view settles after its boot, messages and steps.
pub fn render(source: &str, options: &Options) -> Result<Png, PlayError> {
    still(source, options).map(|image| match image.picture {
        Picture::Still(png) => png,
        Picture::Animated(_) => unreachable!("a still render draws a PNG"),
    })
}

/// [`render`] as an [`Image`]: a full app's has its targets.
fn still(source: &str, options: &Options) -> Result<Image, PlayError> {
    validate(options)?;
    no_files();
    let program = wrap(source, options)?;
    let deadline = Instant::now() + options.limits.time;
    let canvas = canvas(options);
    let rendered = if shape(source) == Shape::App {
        return render_application(&program, source, options, canvas, deadline);
    } else {
        let sandbox = sandbox(&options.limits, Deadline::new(deadline));
        let compiler = rawr_iced::preview::Compiler::configured(sandbox)
            .map_err(|error| PlayError::Render(error.to_string()))?;
        // An inline `mod` needs the crate compiler, as in `application`.
        let checked = if has_top_level_mod(&program) {
            compiler.check_named_crate_source(SOURCE_NAME, &program)
        } else {
            compiler.check_named_source(SOURCE_NAME, &program)
        };
        let checked = checked.map_err(|error| {
            PlayError::Compile(report(
                &error.report(diagnostic(&program)),
                &program,
                source,
            ))
        })?;
        checked
            .render_png_with(canvas)
            .map_err(|error| classify(&error.to_string(), &options.limits))
    };
    if Instant::now() >= deadline {
        return Err(PlayError::Limit(TIME_LIMIT.into()));
    }
    rendered.map(|png| Image {
        picture: Picture::Still(png),
        note: None,
        actions: Vec::new(),
        clicked: Vec::new(),
    })
}

/// A full app runs as `rawr run` would show it, headless: booted, given
/// the options' messages and steps, and drawn once its view settles. It
/// runs on the simulated clock as a recording does, so its timers fire and
/// the image is the recording's last frame: the one a GIF ends on, drawn
/// at the still's scale over the same widget trees, so its targets are
/// what it shows. The recording keeps the still budget, and a recording
/// the budget cuts shows its last frame.
fn render_application(
    program: &str,
    source: &str,
    options: &Options,
    canvas: rawr_iced::snapshot::Canvas,
    deadline: Instant,
) -> Result<Image, PlayError> {
    // The app boots on the simulated clock, so the times it reads are on it.
    let _clock = rawr_iced::snapshot::clock::freeze();
    let (mut adapter, messages) = application(program, source, options, Deadline::new(deadline))?;
    // The app's own `.theme(f)` wins over the built-in theme.
    let theme = app_theme(source).is_none().then(|| options.theme.variant());
    let bound = options.size.unwrap_or((MAX_SIDE, MAX_SIDE));
    let motion = rawr_iced::snapshot::Motion {
        step: motion::STEP,
        frames: motion::FRAMES,
        phase: motion::PHASE,
        span: motion::SPAN,
        deadline: Deadline::new(deadline),
        extended: deadline,
        cut: None,
        // Frames after the view changes are drawn small, and the last one
        // again at the still's scale.
        animation_scale: Some(motion::SCALE),
        still: true,
    };
    let mut loops = motion::Loops::default();
    let recording = rawr_iced::snapshot::record_application(
        &mut adapter,
        messages,
        &options.steps,
        canvas,
        bound,
        theme,
        motion,
        &mut |frames| loops.check(frames),
        &mut |_| {},
    )
    .map_err(|error| classify(&error.to_string(), &options.limits))?;
    let last = recording
        .frames
        .last()
        .ok_or_else(|| PlayError::Limit(TIME_LIMIT.into()))?;
    Ok(Image {
        picture: Picture::Still(motion::png(last)?),
        note: None,
        actions: recording.targets.into_iter().map(Action::from).collect(),
        clicked: recording.clicked,
    })
}

/// A full app checked and booted, and the options' messages for it.
fn application(
    program: &str,
    source: &str,
    options: &Options,
    deadline: Deadline,
) -> Result<
    (
        rawr_iced::program::Adapter,
        Vec<rawr_iced::runtime::Message>,
    ),
    PlayError,
> {
    let compiler = rawr_iced::application::Compiler::configured(sandbox(&options.limits, deadline))
        .map_err(|error| PlayError::Render(error.to_string()))?;
    let compile = |error: rawr_iced::application::Error| {
        PlayError::Compile(report(&error.report(diagnostic(program)), program, source))
    };
    // An inline `mod` needs the crate compiler, which reads the one file
    // as a crate's root.
    let checked = if has_top_level_mod(program) {
        compiler.check_named_crate_source(SOURCE_NAME, program)
    } else {
        compiler.check_named_source(SOURCE_NAME, program)
    };
    let adapter = checked
        .map_err(compile)?
        .prepare_with(
            rawr_play_sandbox::runtime_facades()
                .expect("the sandbox's runtime facades bind every item"),
        )
        .map_err(compile)?
        .into_adapter();
    let messages = (0..options.messages.len())
        .map(|index| {
            adapter
                .message_of(&format!("{MESSAGE}{index}"))
                .expect("each message is wrapped as a function")
                .map_err(|error| classify(&error.to_string(), &options.limits))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((adapter, messages))
}

/// The untrusted-snippet limits on an Engine.
fn sandbox(limits: &Limits, deadline: Deadline) -> impl FnOnce(&mut rawr::Engine) + use<> {
    let limits = limits.clone();
    move |engine| {
        // rand and tracing_subscriber, which iced's examples name.
        rawr_play_sandbox::install(engine).expect("the sandbox's facades install");
        let depth = engine.max_call_levels().min(limits.call_depth);
        engine
            .set_max_operations(limits.operations)
            .set_max_call_levels(depth)
            .set_max_string_size(limits.string_size)
            .set_max_array_size(limits.array_size)
            .set_max_map_size(limits.map_size)
            .on_progress(move |_| deadline.past().then(|| TIME_LIMIT.into()))
            // Untyped `fn f(x) { .. }` bodies reach the base
            // language: no output, no module files, no eval, no sleeping.
            .on_print(|_| {})
            .on_debug(|_, _, _| {})
            .on_output(|_, _| {})
            .set_module_resolver(rawr::module_resolvers::DummyModuleResolver::new());
        for symbol in ["import", "eval", "print", "debug"] {
            engine.disable_symbol(symbol);
        }
        let refuse =
            || Err::<(), Box<rawr::EvalAltResult>>("sleep is not available in ?play".into());
        engine
            .register_fn("sleep", move |_: rawr::INT| refuse())
            .register_fn("sleep", move |_: rawr::FLOAT| refuse());
    }
}

const TIME_LIMIT: &str = "render time limit exceeded";

fn validate(options: &Options) -> Result<(), PlayError> {
    if !(1..=4).contains(&options.scale) {
        return Err(PlayError::Options(format!(
            "scale must be between 1 and 4, got {}",
            options.scale
        )));
    }
    let (width, height) = options.size.unwrap_or((MAX_SIDE, MAX_SIDE));
    for (name, side) in [("width", width), ("height", height)] {
        if side == 0 || side > MAX_SIDE {
            return Err(PlayError::Options(format!(
                "{name} must be between 1 and {MAX_SIDE}, got {side}"
            )));
        }
    }
    for message in &options.messages {
        let ok = !message.trim().is_empty()
            && message
                .chars()
                .all(|c| !matches!(c, ';' | '{' | '}' | '\n' | '\r' | '#'));
        if !ok {
            return Err(PlayError::Options(format!(
                "message `{message}` must be one expression like `Message::Increment`"
            )));
        }
    }
    Ok(())
}

fn diagnostic(program: &str) -> Option<rawr::diagnostic::Source> {
    rawr::diagnostic::Source::new(SOURCE_NAME, program).ok()
}

/// A report as the user's own source shows it (see [`rawr_snippet::report`]).
fn report(report: &rawr::diagnostic::Report, program: &str, source: &str) -> String {
    rawr_snippet::report(&rawr::diagnostic::render(report), program, source)
}

/// `report`, a run-time failure, as a Cargo-built program prints its panic.
fn panic(report: &rawr::diagnostic::Report, program: &str, source: &str) -> String {
    rawr_snippet::panic(&rawr::diagnostic::panic(report), program, source)
}

fn classify(text: &str, limits: &Limits) -> PlayError {
    let lower = text.to_ascii_lowercase();
    let reason = if lower.contains(TIME_LIMIT) || lower.contains("terminated") {
        TIME_LIMIT.to_owned()
    } else if lower.contains("too many operations") {
        format!(
            "operation limit exceeded: more than {} operations (an endless loop?)",
            limits.operations
        )
    } else if lower.contains("stack overflow") {
        format!(
            "call depth limit exceeded: deeper than {} calls (endless recursion?)",
            limits.call_depth
        )
    } else if lower.contains("too large") {
        // A position here counts in the lowered program, so it is dropped.
        let what = without_positions(text.rsplit(": ").next().unwrap_or(text));
        format!(
            "size limit exceeded: {what} (strings up to {} bytes, arrays and maps up to {} items)",
            limits.string_size, limits.array_size
        )
    } else {
        return PlayError::Render(text.to_owned());
    };
    PlayError::Limit(reason)
}

/// Wrap a snippet into a program: a view or an expression gets one
/// `#[preview]`; a full app runs as itself (see [`rawr_snippet::wrap`]).
pub fn wrap(source: &str, options: &Options) -> Result<String, PlayError> {
    // Steps click a full app's targets, as messages reach a full app.
    if !options.steps.is_empty() && shape(&without_tests(source)) != Shape::App {
        return Err(PlayError::Options(rawr_snippet::NEEDS_APP.into()));
    }
    let messages = &options.messages;
    rawr_snippet::wrap(
        source,
        &rawr_snippet::Options {
            size: options.size.unwrap_or((MAX_SIDE, MAX_SIDE)),
            theme: options.theme.variant(),
            messages,
            prelude: &iced_claims().prelude_imports(),
        },
    )
    .map_err(PlayError::Options)
}

#[cfg(test)]
mod tokio_tests;

#[cfg(test)]
mod tests {
    use super::{render, shape, Options, PlayError, Shape};

    fn is_png(bytes: &[u8]) -> bool {
        bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    }

    #[test]
    fn render_app_keeps_its_own_theme() {
        let app = |theme: &str| {
            format!(
                "use iced::{{Color, Theme, Widget}};\nuse iced::widget::text;\n\
                 pub fn main() -> iced::Result {{ iced::application(App::default, App::update, App::view){theme}.run() }}\n\
                 #[derive(Default)] struct App;\n#[derive(Debug, Clone)] enum Message {{}}\n\
                 impl App {{\n    fn update(&mut self, _message: Message) {{}}\n\
                 fn theme(&self) -> Theme {{ Theme::custom(\"Red\", iced::theme::palette::Seed {{ background: Color::from_rgb8(0xff, 0, 0), text: Color::WHITE, primary: Color::WHITE, success: Color::WHITE, warning: Color::WHITE, danger: Color::WHITE }}) }}\n\
                 fn view(&self) -> impl Widget<Message> {{ text(\"hi\") }}\n}}\n"
            )
        };
        let options = Options {
            size: Some((40, 20)),
            ..Options::default()
        };
        let red = render(&app(".theme(App::theme)"), &options).unwrap();
        let plain = render(&app(""), &options).unwrap();
        assert!(is_png(&red.bytes));
        assert_ne!(red.bytes, plain.bytes, "the app's theme is drawn");
    }

    /// A radio and a pick list find the selection through the snippet's
    /// own `PartialEq`, as iced's `Some(value) == selected` does: a size
    /// equal by its inches is selected though its name differs.
    #[test]
    fn radio_and_pick_list_compare_by_a_hand_written_eq() {
        let snippet = |selected: &str| {
            format!(
                "use iced::widget::{{column, pick_list, radio}};\nuse iced::Widget;\n\
                 #[derive(Debug, Clone, Copy)] struct Size {{ inches: i64, name: char }}\n\
                 impl PartialEq for Size {{ fn eq(&self, other: &Self) -> bool {{ self.inches == other.inches }} }}\n\
                 impl Eq for Size {{}}\n\
                 impl std::fmt::Display for Size {{ fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{ write!(f, \"{{}}\", self.name) }} }}\n\
                 fn view() -> impl Widget<()> {{\n\
                     let selected = Some({selected});\n\
                     column![\n\
                         radio(\"one\", Size {{ inches: 1, name: 'a' }}, selected, |_| ()),\n\
                         radio(\"two\", Size {{ inches: 2, name: 'b' }}, selected, |_| ()),\n\
                         pick_list(selected, vec![Size {{ inches: 1, name: 'a' }}, Size {{ inches: 2, name: 'b' }}], Size::to_string),\n\
                     ]\n\
                 }}\n"
            )
        };
        let options = Options {
            size: Some((120, 120)),
            ..Options::default()
        };
        let by_inches = render(&snippet("Size { inches: 2, name: 'z' }"), &options).unwrap();
        let same = render(&snippet("Size { inches: 2, name: 'b' }"), &options).unwrap();
        let other = render(&snippet("Size { inches: 3, name: 'b' }"), &options).unwrap();
        assert_eq!(
            by_inches.bytes, same.bytes,
            "`z` is equal to `b` by its `eq`"
        );
        assert_ne!(
            other.bytes, same.bytes,
            "a size of other inches selects no radio"
        );
    }

    #[test]
    fn render_expression_shape() {
        let source = r#"column![text("hi"), button("go")]"#;
        assert_eq!(shape(source), Shape::Expression);
        let options = Options {
            size: Some((120, 80)),
            ..Options::default()
        };
        let png = render(source, &options).unwrap();
        assert_eq!((png.width, png.height), (240, 160), "2x HiDPI");
        assert!(is_png(&png.bytes));
    }

    #[test]
    fn render_view_fn_shape() {
        let source =
            "fn view() -> impl Widget<()> {\n    row![\"left\", text(\"right\")].spacing(8)\n}\n";
        assert_eq!(shape(source), Shape::View);
        let png = render(source, &Options::default()).unwrap();
        assert!(is_png(&png.bytes));
    }

    #[test]
    fn render_app_shape_with_messages() {
        let source = r#"
use iced::widget::{Widget, button, column, text};

#[derive(Default)]
struct State { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Increment }

impl State {
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 1 }
    }

    fn view(&self) -> impl Widget<Message> {
        column![text(self.value), button("+").on_press(Message::Increment)]
    }
}
"#;
        assert_eq!(shape(source), Shape::App);
        let plain = render(source, &Options::default()).unwrap();
        let options = Options {
            messages: vec!["Message::Increment".into(), "Message::Increment".into()],
            ..Options::default()
        };
        let stepped = render(source, &options).unwrap();
        assert!(is_png(&stepped.bytes));
        assert_ne!(plain.bytes, stepped.bytes, "messages must change the view");
    }

    #[test]
    fn render_app_of_iced_run_closures() {
        // No `fn view`: `iced::run` takes closures, as `rawr run` does.
        let source = r#"
use iced::widget::{button, column, text};

#[derive(Debug, Clone)]
enum Message { Roar }

fn main() -> iced::Result {
    iced::run(
        |roars: &mut u32, _: Message| *roars += 1,
        |roars: &u32| column![text(roars.to_string()), button("roar").on_press(Message::Roar)],
    )
}
"#;
        assert_eq!(shape(source), Shape::App);
        let plain = render(source, &Options::default()).unwrap();
        let options = Options {
            messages: vec!["Message::Roar".into()],
            ..Options::default()
        };
        let stepped = render(source, &options).unwrap();
        assert_ne!(plain.bytes, stepped.bytes, "messages must change the view");
    }

    #[test]
    fn render_a_canvas_drawing() {
        // A view of a canvas program fills its frame: the center pixel is
        // the circle's color.
        let source = r#"
use iced::mouse;
use iced::widget::canvas::{self, Geometry, Path};
use iced::{Color, Rectangle, Renderer, Theme, Widget};

struct Dot;

impl<Message> canvas::Program<Message> for Dot {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill(&Path::circle(frame.center(), 40.0), Color::from_rgb8(0xff, 0x2e, 0x93));
        vec![frame.into_geometry()]
    }
}

fn view() -> impl Widget<()> {
    iced::widget::canvas(Dot).width(100).height(100)
}
"#;
        let options = Options {
            size: Some((100, 100)),
            scale: 1,
            ..Options::default()
        };
        let png = render(source, &options).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(&png.bytes))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        let channels = info.color_type.samples();
        let center = ((50 * info.width + 50) as usize) * channels;
        assert_eq!(
            &rgba[center..center + 3],
            &[0xff, 0x2e, 0x93],
            "the circle is drawn"
        );
    }

    #[test]
    fn render_runaway_loop_hits_a_limit() {
        let source = "fn view() -> impl Widget<()> {\n    let mut n = 0;\n    loop { n += 1; }\n    text(n)\n}\n";
        let error = render(source, &Options::default()).unwrap_err();
        assert!(matches!(error, PlayError::Limit(_)), "{error:?}");
        assert!(error.text().contains("operation limit"), "{error}");

        let options = Options {
            limits: super::Limits {
                operations: u64::MAX,
                time: std::time::Duration::from_millis(50),
                ..super::Limits::default()
            },
            ..Options::default()
        };
        let error = render(source, &options).unwrap_err();
        assert!(matches!(error, PlayError::Limit(_)), "{error:?}");
        assert!(error.text().contains(super::TIME_LIMIT), "{error}");
    }

    #[test]
    fn maps_and_arrays_share_one_size_limit() {
        let limits = super::Limits::default();
        assert_eq!((limits.array_size, limits.map_size), (65_536, 65_536));
        let PlayError::Limit(reason) = super::classify("Size of object map too large", &limits)
        else {
            panic!("not a limit");
        };
        assert!(
            reason.contains("arrays and maps up to 65536 items"),
            "{reason}"
        );
    }

    #[test]
    fn render_size_and_depth_limits() {
        let huge = "fn view() -> impl Widget<()> {\n    let mut s = String::new();\n    for i in 0..10000000 { s.push_str(\"xxxxxxxxxxxxxxxxxxxx\"); }\n    text(s)\n}\n";
        let error = render(huge, &Options::default()).unwrap_err();
        assert!(
            matches!(error, PlayError::Limit(_)) && error.text().contains("size limit"),
            "{error:?}"
        );
        let deep =
            "fn f(n: i64) -> i64 { f(n + 1) + 1 }\nfn view() -> impl Widget<()> { text(f(0)) }\n";
        let error = render(deep, &Options::default()).unwrap_err();
        assert!(
            matches!(error, PlayError::Limit(_)) && error.text().contains("call depth"),
            "{error:?}"
        );
    }

    #[test]
    fn render_sandbox_refuses_escapes() {
        let view = "\nfn view() -> impl Widget<()> { leak(1); text(\"x\") }\n";
        for body in [
            "print(\"out\"); x",
            "debug(\"out\"); x",
            "eval(\"40 + 2\")",
            "import \"iced/examples/application_counter\" as m; x",
            "sleep(100); x",
        ] {
            let source = format!("fn leak(x) {{ {body} }}{view}");
            assert!(render(&source, &Options::default()).is_err(), "{body}");
        }
        let preview =
            "#[preview(size = (60000, 60000))]\nfn view() -> impl Widget<()> { text(\"x\") }\n";
        let error = render(preview, &Options::default()).unwrap_err();
        assert!(matches!(error, PlayError::Options(_)), "{error:?}");
    }

    #[test]
    fn render_encodes_jpeg_and_webp() {
        let options = Options {
            size: Some((64, 32)),
            ..Options::default()
        };
        let png = render("text(\"hi\")", &options).unwrap();
        let jpeg = super::encode(&png, super::Format::Jpeg).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        let webp = super::encode(&png, super::Format::Webp).unwrap();
        assert!(webp.starts_with(b"RIFF") && &webp[8..12] == b"WEBP");
    }

    #[test]
    fn a_use_in_a_function_body_keeps_the_prelude() {
        let source = "fn view() -> impl Widget<()> {\n    use iced::widget::column;\n    column![text(\"hi\")]\n}\n";
        render(source, &Options::default()).unwrap();
    }

    /// The prelude has the modules and types iced's examples use most in a
    /// view: `border`, `padding`, `Shadow`, `color!`, `rule`, `font`,
    /// `alignment`, `gradient`, angles, `FillPortion` and `mouse`.
    #[test]
    fn the_prelude_has_what_iced_examples_use_in_views() {
        let source = "fn view() -> impl Widget<()> {\ncolumn![\n\
            container(text(\"bordered\")).padding(padding::all(10)).style(|_| container::Style {\n\
                border: border::rounded(8).color(Color::BLACK).width(2),\n\
                shadow: Shadow { color: Color::BLACK, offset: Vector::new(2.0, 2.0), blur_radius: 4.0 },\n\
                background: Some(color!(0x88ccff).into()),\n\
                ..container::Style::default()\n\
            }),\n\
            rule::horizontal(2),\n\
            text(\"bold\").font(Font { weight: font::Weight::Bold, ..Font::default() }).align_x(alignment::Horizontal::Right),\n\
            container(text(\"fade\")).style(|_| gradient::Linear::new(Degrees(90.0)).add_stop(0.0, Color::WHITE).into()),\n\
            row![text(\"a\").width(FillPortion(1)), text(\"b\").width(FillPortion(2))],\n\
            mouse_area(text(\"hand\")).interaction(mouse::Interaction::Pointer),\n\
            container(text(\"turn\")).style(|_| gradient::Linear::new(Radians(0.5)).add_stop(1.0, Color::BLACK).into()),\n\
            canvas::Canvas::new(Dot).width(40).height(40),\n\
        ]\n}";
        let source = format!(
            "{source}\n#[derive(Debug)] struct Dot;\n\
             impl canvas::Program<()> for Dot {{\n\
                 type State = ();\n\
                 fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {{\n\
                     let mut frame = canvas::Frame::new(renderer, bounds.size());\n\
                     frame.fill_rectangle(Point::ORIGIN, Size::new(4.0, 4.0), Color::BLACK);\n\
                     vec![frame.into_geometry()]\n\
                 }}\n\
             }}\n"
        );
        render(&source, &Options::default()).unwrap_or_else(|error| panic!("{error}"));
    }

    #[test]
    fn render_compile_error_carries_diagnostics() {
        let error = render("column![text(\"hi\")", &Options::default()).unwrap_err();
        let PlayError::Compile(text) = error else {
            panic!("expected a compile error, got {error:?}");
        };
        assert!(text.contains("play.rs"), "{text}");
    }

    #[test]
    fn compile_errors_quote_only_the_snippet() {
        let error = render("text(\"hi\").bogus(1)", &Options::default()).unwrap_err();
        let PlayError::Compile(text) = error else {
            panic!("expected a compile error, got {error:?}");
        };
        // The method's name is underlined, as rustc underlines it.
        assert!(text.contains(" --> play.rs:1:12-1:17\n"), "{text}");
        assert!(text.contains(" 1 | text(\"hi\").bogus(1)\n"), "{text}");
        assert!(text.contains("   |            ^^^^^\n"), "{text}");
        assert!(!text.contains("use iced"), "{text}");
    }

    #[test]
    fn render_fits_the_content() {
        let small = render("text(\"hi\")", &Options::default()).unwrap();
        let tall = render(
            "column![text(\"a\"), text(\"b\"), text(\"c\")]",
            &Options::default(),
        )
        .unwrap();
        // A margin on each side at 2x, plus the text, well under the bound.
        assert!(
            small.width > 4 * super::MARGIN && small.width < 200,
            "{}x{}",
            small.width,
            small.height
        );
        assert!(tall.height > small.height && tall.width < 200);
        // A `Fill` width takes the fallback width.
        let fill = render("text_input(\"name\", \"\")", &Options::default()).unwrap();
        assert_eq!(fill.width, 2 * super::FALLBACK_SIZE.0);
    }

    /// Andy's master component: "hi" shows only once the `()` message
    /// has shown the component, its sensor has fired, and its `update` has
    /// taken that Event, frames after the message.
    #[test]
    fn render_master_component_settles() {
        let source = include_str!("../tests/fixtures/component_sensor.rs");
        assert_eq!(shape(source), Shape::App);
        let shown = Options {
            messages: vec!["()".into()],
            ..Options::default()
        };
        let plain = render(source, &Options::default()).unwrap();
        let hi = render(source, &shown).unwrap();
        assert!(is_png(&hi.bytes));
        assert_eq!(plain.width, hi.width, "the button sets the width");
        assert!(hi.height > plain.height, "\"hi\" is drawn below \"show\"");
    }

    fn text(source: &str) -> super::Output {
        match super::run(source, &Options::default()).unwrap() {
            super::Outcome::Text(output) => output,
            super::Outcome::Image(_) => panic!("{source} rendered an image"),
        }
    }

    fn image(source: &str) -> super::Png {
        match super::run(source, &Options::default()).unwrap() {
            super::Outcome::Image(super::Image {
                picture: super::Picture::Still(png),
                ..
            }) => png,
            super::Outcome::Image(_) => panic!("{source} rendered an animation"),
            super::Outcome::Text(output) => panic!("{source} ran as text: {output:?}"),
        }
    }

    #[test]
    fn run_plain_program_prints_its_output() {
        let cart = text(include_str!("../../examples/scripts/cart.rs"));
        assert_eq!(cart.text(), "total: 27\n");
        assert_eq!((cart.status, cart.truncated, cart.limit), (0, false, false));
    }

    #[test]
    fn run_plain_program_interleaves_streams_in_order() {
        use super::plain::Stream::{Stderr, Stdout};
        let output = text("fn main() {\n    println!(\"a\");\n    eprintln!(\"b\");\n    print!(\"c\");\n    println!(\"d\");\n}\n");
        assert_eq!(
            output.chunks,
            [
                (Stdout, "a\n".into()),
                (Stderr, "b\n".into()),
                (Stdout, "cd\n".into())
            ]
        );
        let json = output.to_json();
        assert!(json.starts_with('{'), "{json}");
        assert_eq!(super::Output::from_json(json.as_bytes()).unwrap(), output);
    }

    #[test]
    fn run_plain_main_returning_err_exits_1() {
        let output = text("fn main() -> Result<(), String> {\n    println!(\"before\");\n    Err(\"bad\".to_string())\n}\n");
        assert_eq!(output.status, 1);
        assert_eq!(output.text(), "before\nError: \"bad\"\n");
        assert!(!output.limit);
    }

    #[test]
    fn run_plain_runtime_errors_point_at_the_users_text() {
        // A Vec's index panics at its `[`, as rustc places it.
        let output = text("let a = vec![1,2,3]; let i = 5; println!(\"{}\", a[i]);");
        let shown = output.text();
        assert_eq!(
            shown,
            format!(
                "thread 'main' panicked at play.rs:1:49:\n\
                 index out of bounds: the len is 3 but the index is 5\n{}\n",
                rawr::diagnostic::BACKTRACE_NOTE
            )
        );
        let output = text("let x: u8 = 255; let y = x + 1; y");
        let shown = output.text();
        assert!(
            shown.contains("panicked at play.rs:1:26:\nattempt to add with overflow\n"),
            "{shown}"
        );
        let limited = text("let mut s = String::new(); loop { s.push_str(\"xxxxxxxxxxxxxxxx\"); }");
        assert!(
            limited.limit && !limited.text().contains("(line "),
            "{}",
            limited.text()
        );
        // The wrapping's own text is never a position in the user's.
        let program = super::plain::program("1 + 2");
        assert!(!rawr_snippet::in_source(&program, "1 + 2", 1, 3));
        assert!(rawr_snippet::in_source(&program, "1 + 2", 1, 25));
        assert!(!rawr_snippet::in_source(&program, "1 + 2", 3, 1));
    }

    #[test]
    fn an_array_index_panics_at_the_expression_start_and_a_vec_index_at_its_bracket() {
        // An index rustc can't see at compile time, which it would refuse.
        let i = "let i: usize = \"5\".parse().unwrap(); ";
        // Each line with the column `cargo run` prints for it.
        let cases = [
            ("let a = [1, 2, 3]; a[i]", 57),
            ("let a = [1, 2, 3]; let r = &a; r[i]", 69),
            ("let mut a = [1, 2, 3]; a[i] = 0;", 61),
            ("let mut a = [1, 2, 3]; a[i] += 1;", 61),
            ("let g = [[1, 2, 3]; 3]; g[1][i]", 62),
            ("let v = vec![[1, 2, 3]]; v[0][i]", 63),
            ("let v = vec![1, 2, 3]; v[i]", 62),
            ("let mut v = vec![1, 2, 3]; v[i] = 0;", 66),
        ];
        let shown = cases
            .iter()
            .map(|(line, _)| {
                let text = text(&format!("{i}{line}")).text().to_owned();
                text.lines().next().unwrap_or_default().to_owned()
            })
            .collect::<Vec<_>>();
        let expected = cases
            .iter()
            .map(|(_, column)| format!("thread 'main' panicked at play.rs:1:{column}:"))
            .collect::<Vec<_>>();
        assert_eq!(shown, expected);
    }

    #[test]
    fn run_plain_expression_prints_its_value() {
        assert_eq!(text("1 + 2").text(), "3\n");
        assert_eq!(text("vec![1, 2, 3].iter().sum::<i64>()").text(), "6\n");
        assert_eq!(text("let a = 20;\na + 7").text(), "27\n");
        assert_eq!(text("println!(\"hi\")").text(), "hi\n");
        // As in a Rust block: a final `;` leaves no value to print.
        assert_eq!(text("1 + 2;").text(), "");
        assert_eq!(text("let a = 2;\n{ let b = a * 3; b }").text(), "6\n");
        assert_eq!(text("\"a\".to_string() + \"b\"").text(), "\"ab\"\n");
    }

    #[test]
    fn run_plain_panic_exits_101_like_rust() {
        let output = text("println!(\"before\");\nassert_eq!(1 + 1, 3);");
        assert_eq!(output.status, 101);
        assert!(!output.limit);
        let text = output.text();
        assert_eq!(
            text,
            format!(
                "before\nthread 'main' panicked at play.rs:2:1:\n\
                 assertion `left == right` failed\n  left: 2\n right: 3\n{}\n",
                rawr::diagnostic::BACKTRACE_NOTE
            )
        );
    }

    /// A plain program's output, with a debug build's longer time budget.
    fn async_text(source: &str) -> super::Output {
        let options = Options {
            limits: super::Limits {
                time: std::time::Duration::from_secs(if cfg!(debug_assertions) { 30 } else { 5 }),
                ..super::Limits::default()
            },
            ..Options::default()
        };
        match super::run(source, &options).unwrap() {
            super::Outcome::Text(output) => output,
            super::Outcome::Image(_) => panic!("{source} rendered an image"),
        }
    }

    /// `#[tokio::main] async fn main` runs on rawr-iced's tokio with no
    /// window: sleeps on the simulated clock, a channel between two joined
    /// futures, and `spawn_blocking`. An `Err` from `main` exits 1, and a
    /// panic exits 101, as a plain program's do.
    #[test]
    fn run_plain_async_main_on_the_simulated_clock() {
        let output = async_text(
            "use std::time::Duration;\n\
             use tokio::sync::mpsc;\n\
             use tokio::time::{sleep, Instant};\n\
             \n\
             #[tokio::main]\n\
             async fn main() {\n\
                 let start = Instant::now();\n\
                 let (sender, mut receiver) = mpsc::channel(4);\n\
                 let produce = async move {\n\
                     for n in 1..=3 {\n\
                         sleep(Duration::from_millis(250)).await;\n\
                         sender.send(n).await.unwrap();\n\
                     }\n\
                 };\n\
                 let consume = async move {\n\
                     let mut total = 0;\n\
                     while let Some(n) = receiver.recv().await {\n\
                         println!(\"got {n} at {} ms\", start.elapsed().as_millis());\n\
                         total += n;\n\
                     }\n\
                     total\n\
                 };\n\
                 let (_, total) = tokio::join!(produce, consume);\n\
                 let primes = tokio::task::spawn_blocking(|| (2..100u64).filter(|n| (2..*n).all(|d| n % d != 0)).count())\n\
                     .await\n\
                     .unwrap();\n\
                 println!(\"total {total}, {primes} primes, done at {} ms\", start.elapsed().as_millis());\n\
             }\n",
        );
        assert_eq!(
            output.text(),
            "got 1 at 250 ms\ngot 2 at 500 ms\ngot 3 at 750 ms\ntotal 6, 25 primes, done at 750 ms\n"
        );
        assert_eq!((output.status, output.limit), (0, false));

        let failed = async_text(
            "use std::time::Duration;\n\
             #[tokio::main]\n\
             async fn main() -> Result<(), String> {\n\
                 tokio::time::sleep(Duration::from_millis(10)).await;\n\
                 println!(\"before\");\n\
                 Err(\"bad\".to_string())\n\
             }\n",
        );
        assert_eq!(failed.text(), "before\nError: \"bad\"\n");
        assert_eq!(failed.status, 1);

        let panicked = async_text(
            "fn third(v: &[i64]) -> i64 {\n    v[3]\n}\n\
             #[tokio::main]\n\
             async fn main() {\n\
                 tokio::task::yield_now().await;\n\
                 println!(\"{}\", third(&Vec::new()));\n\
             }\n",
        );
        assert_eq!(panicked.status, 101);
        let text = panicked.text();
        assert!(
            // rustc places a slice's index at 2:5 and a Vec's at its `[`;
            // rawr places both at the `[` (a known difference).
            text.contains("out of bounds") && text.contains("panicked at play.rs:2:6:"),
            "{text}"
        );

        // A failure in the async body's own statements is placed at the
        // statement, before an await and after one.
        for (body, at) in [
            ("let v: Vec<i64> = Vec::new();\n    let x = v[3];\n    println!(\"{x}\");", "play.rs:4:14:"),
            (
                "tokio::task::yield_now().await;\n    let v: Vec<i64> = Vec::new();\n    println!(\"{}\", v[3]);",
                "play.rs:5:21:",
            ),
        ] {
            let failed = async_text(&format!("#[tokio::main]\nasync fn main() {{\n    {body}\n}}\n"));
            let text = failed.text();
            assert_eq!(failed.status, 101, "{text}");
            assert!(text.contains("out of bounds") && text.contains(at), "{text}");
        }
    }

    #[test]
    fn async_main_is_found_with_its_tokio_attribute() {
        let source = "use std::time::Duration;\n#[tokio::main]\nasync fn main() -> Result<(), String> {\n    Ok(())\n}\n";
        let main = super::plain::async_main(source).unwrap();
        assert_eq!(&source[main.name..main.name + 4], "main");
        assert_eq!(
            main.attribute.clone().map(|span| &source[span]),
            Some("#[tokio::main]")
        );
        assert!(main.returns);
        assert_eq!(
            super::plain::async_main("fn main() {}\nasync fn helper() {}\n"),
            None
        );
    }

    #[test]
    fn run_plain_statement_errors_keep_their_positions() {
        let error = super::run("let x: i64 = \"s\";", &Options::default()).unwrap_err();
        let PlayError::Compile(text) = error else {
            panic!("expected a compile error, got {error:?}");
        };
        assert!(text.contains("play.rs:1:14"), "{text}");
        assert!(text.contains(" 1 | let x: i64 = \"s\";"), "{text}");
        let error = super::run("let a = 1;\na + true", &Options::default()).unwrap_err();
        let PlayError::Compile(text) = error else {
            panic!("expected a compile error, got {error:?}");
        };
        assert!(text.contains("play.rs:2:"), "{text}");
    }

    /// A full app that ticks once, then spins in its `update` until a
    /// deadline stops it.
    const TICK_THEN_SPIN: &str = "use iced::time::{self, Duration};
use iced::widget::text;
use iced::{Subscription, Widget};

pub fn main() -> iced::Result {
    iced::application(Spin::default, Spin::update, Spin::view)
        .subscription(Spin::subscription)
        .run()
}

#[derive(Default)]
struct Spin {
    ticks: u64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Tick,
}

impl Spin {
    fn update(&mut self, message: Message) {
        match message {
            Message::Tick => {
                self.ticks += 1;
                if self.ticks > 1 {
                    let mut n = 0;
                    loop {
                        n += 1;
                    }
                }
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }

    fn view(&self) -> impl Widget<Message> {
        text(format!(\"{} ticks\", self.ticks))
    }
}
";

    /// How long `animate` ran `source` with these budgets, and its note.
    fn animate_for(
        source: &str,
        time: u64,
        animation: u64,
    ) -> (std::time::Duration, Option<String>) {
        let options = Options {
            limits: super::Limits {
                operations: u64::MAX,
                time: std::time::Duration::from_millis(time),
                animation: std::time::Duration::from_millis(animation),
                ..super::Limits::default()
            },
            ..Options::default()
        };
        let started = std::time::Instant::now();
        let image = super::animate(source, &options).unwrap();
        (started.elapsed(), image.note)
    }

    /// The docs' tokio channel example (/docs/play/reference) settles on its
    /// last frame within the bot's still budget: its sleeps are on the paused
    /// tokio clock, so they cost no wall time. A debug build is about seven
    /// times slower, so it gets a larger budget.
    #[test]
    fn tokio_channel_example_settles_within_the_still_budget() {
        let budget = if cfg!(debug_assertions) {
            15_000
        } else {
            2_000
        };
        let source = include_str!("../tests/fixtures/channel.rs");
        let (ran, note) = animate_for(source, budget, budget);
        assert_eq!(note, None, "the example ran to its last frame");
        assert!(ran < std::time::Duration::from_millis(budget), "{ran:?}");
        // Longer sleeps cost no more.
        let slower = source.replace("from_millis(100)", "from_millis(2_000)");
        let (_, note) = animate_for(&slower, budget, budget);
        assert_eq!(note, None, "eight seconds of tokio sleeps fit the budget");
    }

    #[test]
    fn budget_switches_once_the_view_changes() {
        // The view changes, so the spin runs to the animation budget.
        let (ran, note) = animate_for(TICK_THEN_SPIN, 1_000, 4_000);
        assert!(ran >= std::time::Duration::from_millis(4_000), "{ran:?}");
        let note = note.unwrap();
        assert!(
            note.starts_with("Rendering stopped after 4 s, so the animation shows its first "),
            "{note}"
        );
        // Without a longer budget, it stops at the still one.
        let (ran, note) = animate_for(TICK_THEN_SPIN, 1_000, 1_000);
        assert!(ran < std::time::Duration::from_millis(3_000), "{ran:?}");
        assert!(note.unwrap().starts_with("Rendering stopped after 1 s"));
        // A view that never changes keeps the still budget.
        let still = TICK_THEN_SPIN.replace("self.ticks > 1", "true");
        let (ran, _) = animate_for(&still, 1_000, 4_000);
        assert!(ran < std::time::Duration::from_millis(3_000), "{ran:?}");
    }

    #[test]
    fn run_plain_endless_loop_hits_the_time_limit() {
        let options = Options {
            limits: super::Limits {
                operations: u64::MAX,
                time: std::time::Duration::from_millis(100),
                ..super::Limits::default()
            },
            ..Options::default()
        };
        let source =
            "fn main() {\n    println!(\"start\");\n    let mut n = 0;\n    loop { n += 1; }\n}\n";
        let super::Outcome::Text(output) = super::run(source, &options).unwrap() else {
            panic!("a plain program runs as text");
        };
        assert_eq!((output.status, output.limit), (1, true));
        assert_eq!(output.text(), "start\ntime limit exceeded\n");
    }

    #[test]
    fn run_plain_output_is_cut_and_the_program_stopped() {
        let options = Options {
            limits: super::Limits {
                operations: u64::MAX,
                time: std::time::Duration::from_secs(20),
                ..super::Limits::default()
            },
            ..Options::default()
        };
        let source = "fn main() {\n    loop { println!(\"0123456789abcdef\"); }\n}\n";
        let started = std::time::Instant::now();
        let super::Outcome::Text(output) = super::run(source, &options).unwrap() else {
            panic!("a plain program runs as text");
        };
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "stopped once full"
        );
        assert!(output.truncated && output.limit && output.status == 1);
        let text = output.text();
        assert!(
            text.len() <= super::plain::MAX_OUTPUT + 64,
            "{}",
            text.len()
        );
        assert!(
            text.ends_with("… output truncated\n"),
            "{}",
            &text[text.len() - 40..]
        );
    }

    #[test]
    fn run_plain_compile_error_is_a_diagnostic() {
        let error = super::run(
            "fn main() {\n    let x: i64 = \"s\";\n}\n",
            &Options::default(),
        )
        .unwrap_err();
        let PlayError::Compile(text) = error else {
            panic!("expected a compile error, got {error:?}");
        };
        assert!(text.contains("play.rs:2:"), "{text}");
    }

    /// A still of an app is the frame after it settles, as a GIF's last
    /// frame is: a boot Task that sleeps on the simulated clock has run.
    #[test]
    fn a_still_app_waits_for_its_timers() {
        const APP: &str = "use iced::Task;
use iced::widget::text;
use std::time::Duration;

pub fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view).run()
}

struct App { done: bool }

#[derive(Debug, Clone)]
enum Message { Done }

impl App {
    fn new() -> (Self, Task<Message>) {
        (App { done: false }, Task::perform(tokio::time::sleep(Duration::from_millis(300)), |_| Message::Done))
    }
    fn update(&mut self, _message: Message) { self.done = true; }
    fn view(&self) -> impl iced::Widget<Message> { text(if self.done { \"done\" } else { \"waiting\" }) }
}
";
        let still = |source: &str| {
            let options = Options {
                still: true,
                ..Options::default()
            };
            match super::animate(source, &options).unwrap().picture {
                super::Picture::Still(png) => png.bytes,
                super::Picture::Animated(_) => panic!("a still was animated"),
            }
        };
        let waited = still(APP);
        let done = still(&APP.replace("App { done: false }", "App { done: true }"));
        let waiting = still(&APP.replace("self.done = true;", ""));
        assert!(waited == done, "the still shows the settled frame");
        assert!(waited != waiting, "the view's text shows in the still");
    }

    #[test]
    fn run_iced_snippets_still_render() {
        // Named `iced`, or only names of rawr-iced's prelude.
        assert!(is_png(&image("column![text(\"hi\")]").bytes));
        assert!(is_png(
            &image("fn view() -> impl Widget<()> {\n    text(\"x\")\n}\n").bytes
        ));
        assert!(is_png(
            &image(include_str!("../tests/fixtures/counter.rs")).bytes
        ));
    }

    #[test]
    fn run_plain_sandbox_refuses_escapes() {
        for body in ["eval(\"40 + 2\")", "import \"x\" as m; x", "sleep(100); x"] {
            let source = format!("fn leak(x) {{ {body} }}\nfn main() {{ leak(1); }}\n");
            let refused = match super::run(&source, &Options::default()) {
                Err(_) => true,
                Ok(super::Outcome::Text(output)) => output.status != 0,
                Ok(super::Outcome::Image(_)) => false,
            };
            assert!(refused, "{body}");
        }
    }

    #[test]
    fn run_plain_program_ignores_messages_with_a_note() {
        let options = Options {
            messages: vec!["Message::Increment".into(), "Message::Increment".into()],
            ..Options::default()
        };
        let super::Outcome::Text(output) = super::run("println!(\"Foo\")", &options).unwrap()
        else {
            panic!("a plain program runs as text");
        };
        assert_eq!((output.text().as_str(), output.status), ("Foo\n", 0));
        assert_eq!(output.note.as_deref(), Some(super::plain::MESSAGES_IGNORED));
        assert_eq!(
            super::Output::from_json(output.to_json().as_bytes()).unwrap(),
            output
        );
    }

    #[test]
    fn run_decides_by_the_claims() {
        // A fake claimant: a crate `gui` with the prelude name `label`.
        let claims = rawr::layer::Claims {
            crates: vec!["gui".into()],
            sections: Vec::new(),
            prelude: vec!["gui::label".into()],
        };
        let options = Options::default();
        // rawr-iced's prelude name, unclaimed here: plain, so `text` is an
        // unknown function, reported at its name before the program runs.
        let unknown = super::run_claimed("text(\"x\")", &options, &claims).unwrap_err();
        let PlayError::Compile(unknown) = unknown else {
            panic!("an unknown function is a compile error: {unknown:?}");
        };
        assert!(unknown.contains("unknown function `text`"), "{unknown}");
        assert!(unknown.contains("play.rs:1:1-1:5"), "{unknown}");
        let plain = super::run_claimed("1 + 2", &options, &claims).unwrap();
        assert!(matches!(plain, super::Outcome::Text(ref output) if output.text() == "3\n"));
        // Claimed by the fake prelude, so handed to the renderer, which
        // knows no `label`: a compile error from the iced side.
        let claimed = super::run_claimed("label(\"x\")", &options, &claims).unwrap_err();
        assert!(matches!(claimed, PlayError::Compile(_)), "{claimed:?}");
        // The real claims: `text` is rawr-iced's.
        assert!(super::iced_claims()
            .prelude
            .iter()
            .any(|path| path.ends_with("::text")));
    }

    #[test]
    fn unknown_function_in_a_snippet_is_an_error_at_its_name() {
        let error = super::run("nonexistent(1)", &Options::default()).unwrap_err();
        let PlayError::Compile(error) = error else {
            panic!("an unknown function is a compile error: {error:?}");
        };
        assert!(error.contains("unknown function `nonexistent`"), "{error}");
        assert!(error.contains("play.rs:1:1-1:12"), "{error}");
    }

    /// The image and targets [`super::animate`] gives `source`.
    fn clicked(source: &str, options: &Options) -> super::Image {
        super::animate(source, options).unwrap_or_else(|error| panic!("{error}"))
    }

    /// The labels of `image`'s targets.
    fn labels(image: &super::Image) -> Vec<&str> {
        image
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect()
    }

    /// Clicks on targets `targets`, in order.
    fn clicks(targets: &[usize]) -> Vec<super::Step> {
        targets.iter().copied().map(super::Step::click).collect()
    }

    #[test]
    fn clicks_on_the_counter_match_its_messages() {
        let source = include_str!("../tests/fixtures/counter.rs");
        let steps = Options {
            steps: clicks(&[0, 0, 1]),
            ..Options::default()
        };
        let image = clicked(source, &steps);
        assert_eq!(labels(&image), ["Increment", "Decrement"]);
        assert_eq!(image.clicked, ["Increment", "Increment", "Decrement"]);
        // Only the last click's frames are recorded, and it settles at once.
        let super::Picture::Still(png) = image.picture else {
            panic!("a settled click is a still");
        };
        let messages = Options {
            messages: vec![
                "Message::Increment".into(),
                "Message::Increment".into(),
                "Message::Decrement".into(),
            ],
            ..Options::default()
        };
        let expected = render(source, &messages).unwrap();
        assert!(
            png.bytes == expected.bytes,
            "+ + - shows what the messages do"
        );
        let unclicked = render(source, &Options::default()).unwrap();
        assert!(png.bytes != unclicked.bytes, "the clicks change the view");
    }

    #[test]
    fn targets_are_labelled_in_reading_order() {
        let source = r#"
use iced::widget::{Widget, button, column, row, text};

#[derive(Default)]
struct State { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Add(i64), Reset }

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Add(by) => self.value += by,
            Message::Reset => self.value = 0,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            row![
                button("+").on_press(Message::Add(1)),
                button("-").on_press(Message::Add(-1)),
                button("+").on_press(Message::Add(10)),
            ]
            .spacing(8),
            text(self.value),
            button("Reset").on_press(Message::Reset),
            button("Off"),
        ]
        .spacing(8)
    }
}
"#;
        let image = clicked(source, &Options::default());
        assert_eq!(
            labels(&image),
            ["+ (1)", "-", "+ (2)", "Reset"],
            "Off is disabled"
        );
        // The second `+` adds 10; the first then adds 1.
        let steps = |steps: Vec<usize>| Options {
            steps: clicks(&steps),
            still: true,
            ..Options::default()
        };
        let messages = |messages: &[&str]| Options {
            messages: messages.iter().map(|m| (*m).to_owned()).collect(),
            still: true,
            ..Options::default()
        };
        let png = |options: &Options| render(source, options).unwrap().bytes;
        assert!(
            png(&steps(vec![2, 0])) == png(&messages(&["Message::Add(10)", "Message::Add(1)"])),
            "steps index the targets in reading order"
        );
        let image = clicked(source, &steps(vec![2, 0, 3]));
        assert_eq!(image.clicked, ["+ (2)", "+ (1)", "Reset"]);
        let error = render(source, &steps(vec![4])).unwrap_err();
        assert!(
            error
                .text()
                .contains("step 1 clicks target 4, but the app shows 4 targets"),
            "{error}"
        );
        let plain = render("column![text(\"hi\")]", &steps(vec![0])).unwrap_err();
        assert!(plain.text().contains("need a full app"), "{plain}");
    }

    #[test]
    fn togglers_radios_and_checkboxes_are_targets_by_their_labels() {
        let source = r#"
use iced::widget::{Widget, checkbox, column, radio, row, toggler};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size { Small, Large }

#[derive(Default)]
struct State { dark: bool, size: Option<Size>, done: bool, bare: bool }

#[derive(Debug, Clone, Copy)]
enum Message { Dark(bool), Size(Size), Done(bool), Bare(bool) }

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Dark(on) => self.dark = on,
            Message::Size(size) => self.size = Some(size),
            Message::Done(on) => self.done = on,
            Message::Bare(on) => self.bare = on,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            toggler(self.dark).label("Dark mode").on_toggle(Message::Dark),
            toggler(self.dark).label("Locked"),
            row![
                radio("Small", Size::Small, self.size, Message::Size),
                radio("Large", Size::Large, self.size, Message::Size),
            ]
            .spacing(20),
            checkbox(self.done).label("Done").on_toggle(Message::Done),
            checkbox(self.bare).on_toggle(Message::Bare),
        ]
        .spacing(12)
    }
}
"#;
        let image = clicked(source, &Options::default());
        assert_eq!(
            labels(&image),
            ["Dark mode", "Small", "Large", "Done", "#5"],
            "a disabled toggler is no target, a bare checkbox is numbered"
        );
        let steps = |steps: Vec<usize>| Options {
            steps: clicks(&steps),
            still: true,
            ..Options::default()
        };
        let messages = |messages: &[&str]| Options {
            messages: messages.iter().map(|m| (*m).to_owned()).collect(),
            still: true,
            ..Options::default()
        };
        let png = |options: &Options| render(source, options).unwrap().bytes;
        assert!(
            png(&steps(vec![0, 2, 4]))
                == png(&messages(&[
                    "Message::Dark(true)",
                    "Message::Size(Size::Large)",
                    "Message::Bare(true)",
                ])),
            "a click toggles a toggler, picks a radio and checks a checkbox"
        );
        let image = clicked(source, &steps(vec![0, 0, 1]));
        assert_eq!(image.clicked, ["Dark mode", "Dark mode", "Small"]);
    }

    #[test]
    fn a_slider_steps_down_and_up_by_its_keys() {
        let source = r#"
use iced::widget::{Widget, column, slider, text};

struct State { value: i64 }

impl Default for State {
    fn default() -> Self { State { value: 5 } }
}

#[derive(Debug, Clone, Copy)]
enum Message { Changed(i64) }

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Changed(value) => self.value = value,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![text(self.value), slider(0..=10, self.value, Message::Changed)]
            .spacing(12)
            .width(200)
    }
}
"#;
        let image = clicked(source, &Options::default());
        assert_eq!(labels(&image), ["Slider −", "Slider +"]);
        let still = |options: Options| Options {
            still: true,
            ..options
        };
        let png = |options: &Options| render(source, options).unwrap().bytes;
        assert!(
            png(&still(Options {
                steps: clicks(&[1, 1, 0]),
                ..Options::default()
            })) == png(&still(Options {
                messages: vec!["Message::Changed(6)".into()],
                ..Options::default()
            })),
            "up, up and down from 5 is 6"
        );
        let image = clicked(
            source,
            &Options {
                steps: clicks(&[0; 6]),
                ..Options::default()
            },
        );
        assert_eq!(image.clicked.len(), 6);
        assert!(
            png(&still(Options {
                steps: clicks(&[0; 6]),
                ..Options::default()
            })) == png(&still(Options {
                messages: vec!["Message::Changed(0)".into()],
                ..Options::default()
            })),
            "a slider stops at its start"
        );
    }

    #[test]
    fn a_step_types_into_a_text_input_and_submits() {
        let source = r#"
use iced::widget::{Widget, button, column, text, text_input};

#[derive(Default)]
struct State { name: String, sent: Vec<String>, secret: String }

#[derive(Debug, Clone)]
enum Message { Name(String), Send, Secret(String), Clear }

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Name(name) => self.name = name,
            Message::Send => self.sent.push(self.name.clone()),
            Message::Secret(secret) => self.secret = secret,
            Message::Clear => self.name.clear(),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            text_input("Name", &self.name)
                .on_input(Message::Name)
                .on_submit(Message::Send),
            text_input("Password", &self.secret)
                .on_input(Message::Secret)
                .secure(true),
            text_input("Locked", ""),
            button("Clear").on_press(Message::Clear),
            text(self.sent.join(", ")),
        ]
        .spacing(8)
        .width(240)
    }
}
"#;
        let image = clicked(source, &Options::default());
        let inputs: Vec<(&str, bool)> = image
            .actions
            .iter()
            .map(|action| (action.label.as_str(), action.input))
            .collect();
        assert_eq!(
            inputs,
            [("Name", true), ("Password", true), ("Clear", false)],
            "a disabled input is no target"
        );
        let still = |steps: Vec<super::Step>| Options {
            steps,
            still: true,
            ..Options::default()
        };
        // The messages, then a click that focuses the input, as typing
        // leaves it.
        let messages = |messages: &[&str]| Options {
            messages: messages.iter().map(|m| (*m).to_owned()).collect(),
            steps: clicks(&[0]),
            still: true,
            ..Options::default()
        };
        let png = |options: &Options| render(source, options).unwrap().bytes;
        let typed = |target, text: &str| super::Step::typed(target, text);
        assert!(
            png(&still(vec![typed(0, "Ada")]))
                == png(&messages(&[
                    "Message::Name(\"Ada\".to_string())",
                    "Message::Send"
                ])),
            "the text is typed, then ⏎ submits it"
        );
        assert!(
            png(&still(vec![typed(0, "Ada"), typed(0, "Bo")]))
                == png(&messages(&[
                    "Message::Name(\"Ada\".to_string())",
                    "Message::Send",
                    "Message::Name(\"Bo\".to_string())",
                    "Message::Send"
                ])),
            "a second text replaces the first"
        );
        let image = clicked(source, &still(vec![typed(1, "hunter2"), typed(0, "Ada")]));
        assert_eq!(
            image.clicked,
            ["Password: \"•••••••\"", "Name: \"Ada\""],
            "a secure input's text is hidden"
        );
        assert_eq!(
            image.actions[1].label, "Password",
            "a label is the placeholder, never the value"
        );
        let error = render(source, &still(vec![typed(2, "x")])).unwrap_err();
        assert!(
            error
                .text()
                .contains("step 1 types text into target 2, but that target is not a text input"),
            "{error}"
        );
    }

    #[test]
    fn render_iced_counter_example_verbatim() {
        let source = include_str!("../tests/fixtures/counter.rs");
        assert_eq!(shape(&super::without_tests(source)), Shape::App);
        let png = render(source, &Options::default()).unwrap();
        assert!(is_png(&png.bytes));
        assert!(
            png.width < 600 && png.height < 600,
            "{}x{}",
            png.width,
            png.height
        );
    }

    // Discord feedback: snippets as users posted them, with no `fn main`.

    #[test]
    fn play_final_iterator_is_not_printed_and_runs_no_stage() {
        for source in [
            r#"[0, 1, 2, 3].iter().map(|idx| println!("It's me! {idx}"))"#,
            r#"[0, 1, 2, 3].into_iter().map(|idx| println!("It's me! {idx}"))"#,
        ] {
            let output = text(source);
            assert_eq!(output.text(), "", "{source}: {output:?}");
            assert_eq!(output.status, 0);
            assert_eq!(output.note.as_deref(), Some(super::plain::UNUSED_ITERATOR));
        }
    }

    #[test]
    fn play_final_value_without_debug_is_not_printed() {
        let output = text("let f = |x: i64| x + 1; f");
        assert_eq!(
            (output.text().as_str(), output.status, output.note),
            ("", 0, None)
        );
    }

    #[test]
    fn play_programs_see_their_name_and_no_files() {
        let output = text(
            r#"fn main() {
    let args: Vec<String> = std::env::args().collect();
    println!("{args:?}");
    match std::fs::read_to_string("/etc/hosts") {
        Ok(text) => println!("read {}", text.len()),
        Err(error) => println!("{error}"),
    }
}"#,
        );
        assert_eq!(
            (output.text().as_str(), output.status),
            ("[\"play\"]\n?play has no files\n", 0)
        );
    }

    #[test]
    fn play_endless_loop_runs_to_a_limit() {
        let source = r#"let mut iteration = 0; loop { iteration += 1; println!("Iteration No{iteration}"); }"#;
        let output = text(source);
        assert!(output.limit && output.truncated, "{output:?}");
        assert!(output.text().starts_with("Iteration No1\nIteration No2\n"));
        assert!(output
            .text()
            .ends_with(&format!("\n{}\n", super::plain::TRUNCATED)));
        assert!(!output.text().contains("Debug"));
    }

    #[test]
    fn play_errors_never_show_the_wrapper() {
        for source in [
            "let x = 1; x +",
            "let x = (1 + 2;",
            "[1, 2].iter().map(|x| x.nope())",
        ] {
            let error = super::run(source, &Options::default()).unwrap_err();
            let text = error.to_string();
            for wrapper in ["__v", "__d", "fn main", "play.rawr"] {
                assert!(!text.contains(wrapper), "{source}: {text}");
            }
            assert!(text.contains(" --> play.rs:1:"), "{source}: {text}");
            assert!(
                text.contains(&format!(" 1 | {source}\n")),
                "{source}: {text}"
            );
        }
    }

    #[test]
    fn play_missing_image_and_svg_render() {
        let missing = image(r#"image("67.png")"#);
        assert!(is_png(&missing.bytes));
        let svg = image(r#"svg("Ivan the Terrible.svg")"#);
        assert!(is_png(&svg.bytes));
    }

    #[test]
    fn play_includes_no_file() {
        for source in [
            r#"text(include_str!("Cargo.toml"))"#,
            r#"text(include_bytes!("Cargo.toml").len())"#,
            r#"svg(concat!(env!("CARGO_MANIFEST_DIR"), "/tiger.svg"))"#,
            r#"text(env!("HOME"))"#,
        ] {
            let Err(PlayError::Compile(error)) = render(source, &Options::default()) else {
                panic!("{source} reads no file");
            };
            assert!(error.contains("?play has no files"), "{error}");
        }
    }

    #[test]
    fn play_skips_an_apps_font_settings_unread() {
        // iced's checkbox example loads its icon font in `main`. The window
        // settings are skipped, so the `include_bytes!` there is never
        // compiled, and the app draws as it would without them.
        let plain = include_str!("../tests/fixtures/checkbox.rs");
        let with_fonts = plain.replace(
            ".run()",
            r#".fonts([include_bytes!("../fonts/icons.ttf").as_slice()]).window_size((640, 480)).run()"#,
        );
        assert_ne!(plain, with_fonts);
        assert_eq!(image(&with_fonts).bytes, image(plain).bytes);
    }

    #[test]
    fn an_app_with_an_inline_module_renders() {
        let source = include_str!("../tests/fixtures/counter.rs")
            .replace("text(self.value)", "text(label::of(self.value))");
        let source = format!("mod label {{\n    pub fn of(value: i64) -> String {{\n        format!(\"count {{value}}\")\n    }}\n}}\n{source}");
        let with_module = image(&source);
        let inline = image(&include_str!("../tests/fixtures/counter.rs").replace(
            "text(self.value)",
            "text(format!(\"count {}\", self.value))",
        ));
        assert!(is_png(&with_module.bytes));
        assert_eq!(with_module.bytes, inline.bytes);
    }

    /// A plain program, a preview and an `async fn main` with inline
    /// modules run as the same program without them does. rustc prints
    /// `6 2.25` for the plain one.
    #[test]
    fn plain_preview_and_async_snippets_take_inline_modules() {
        let plain = text(
            "mod geometry {\n    pub(crate) fn area(w: f64, h: f64) -> f64 {\n        w * h\n    }\n\n    \
             pub mod shapes {\n        pub struct Square(pub f64);\n\n        impl Square {\n            \
             pub fn area(&self) -> f64 {\n                super::area(self.0, self.0)\n            }\n        \
             }\n    }\n}\n\nuse geometry::shapes::Square;\n\nfn main() {\n    let a = geometry::area(2.0, 3.0);\n    \
             println!(\"{a} {}\", Square(1.5).area());\n}\n",
        );
        assert_eq!(plain.text(), "6 2.25\n");
        assert_eq!(plain.status, 0);
        // A file module is still refused: a snippet has no other file.
        let refused = super::run("mod util;\nfn main() {}\n", &Options::default()).unwrap_err();
        assert!(
            matches!(&refused, PlayError::Compile(text) if text.contains("file modules require")),
            "{refused:?}"
        );

        let with_module = image(
            "use iced::widget::text;\nmod labels {\n    pub fn title() -> String {\n        \
             String::from(\"hi\")\n    }\n}\n\nfn view() -> impl iced::widget::Widget<()> {\n    \
             text(labels::title())\n}\n",
        );
        let without = image(
            "use iced::widget::text;\n\nfn view() -> impl iced::widget::Widget<()> {\n    \
             text(String::from(\"hi\"))\n}\n",
        );
        assert!(is_png(&with_module.bytes));
        assert_eq!(with_module.bytes, without.bytes);

        let tasks = text(
            "mod clock {\n    pub fn label(n: u64) -> String {\n        format!(\"tick {n}\")\n    }\n}\n\n\
             #[tokio::main]\nasync fn main() {\n    tokio::time::sleep(std::time::Duration::from_millis(10)).await;\n    \
             println!(\"{}\", clock::label(3));\n}\n",
        );
        assert_eq!(tasks.text(), "tick 3\n");
    }

    #[test]
    fn play_reads_no_file() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../site/public/img/counter.png")
            .canonicalize()
            .unwrap();
        assert!(file.is_absolute() && file.is_file());
        let path = file.display().to_string().replace('\\', "/");
        let missing = image(r#"image("67.png")"#);
        for source in [
            format!("image({path:?})"),
            format!("image(image::Handle::from_path({path:?}))"),
        ] {
            assert_eq!(image(&source).bytes, missing.bytes, "{source} draws blank");
        }
        let svg = image(r#"svg("Ivan the Terrible.svg")"#);
        let favicon = file.parent().unwrap().with_file_name("favicon.svg");
        assert!(favicon.is_file());
        let favicon = favicon.display().to_string();
        assert_eq!(image(&format!("svg({favicon:?})")).bytes, svg.bytes);
    }
}
