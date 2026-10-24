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

pub use motion::{Gif, Image, Picture};
use rawr_iced::snapshot::Deadline;
pub use plain::Output;

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
            _ => Err(format!("unknown format `{value}`; expected png, jpg or webp")),
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
    reader.next_frame(&mut rgba).map_err(|error| failed(&error))?;
    let (width, height) = (png.width, png.height);
    let mut bytes = Vec::new();
    match format {
        Format::Png => unreachable!(),
        Format::Jpeg => {
            let side = |side: u32| u16::try_from(side).map_err(|error| failed(&error));
            jpeg_encoder::Encoder::new(&mut bytes, 90)
                .encode(&rgba, side(width)?, side(height)?, jpeg_encoder::ColorType::Rgba)
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
            // A 128 by 128 RGBA pixel buffer.
            array_size: 64 * 1024,
            map_size: 10_000,
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

/// Identity used for the wrapped source in diagnostics.
pub const SOURCE_NAME: &str = "play.rs";

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
    rawr::resolve::deny_files(NO_FILES);
    let deadline = Instant::now() + options.limits.time;
    let parsed = plain::Parsed::new(source, &options.limits, deadline)?;
    if parsed.claimed(claims) {
        drop(parsed);
        return animate(source, options).map(Outcome::Image);
    }
    // Messages drive an iced app; a plain program runs without them.
    let mut output = parsed.run(source, &options.limits)?;
    if !options.messages.is_empty() {
        let note = output.note.take().map(|note| format!("{note}\n")).unwrap_or_default();
        output.note = Some(format!("{note}{}", plain::MESSAGES_IGNORED));
    }
    Ok(Outcome::Text(output))
}

/// Check, evaluate, and render one iced snippet: a full app on a simulated
/// clock, as [`motion`] decides, and anything else (or any snippet when
/// [`Options::still`] is set) as a still PNG.
pub fn animate(source: &str, options: &Options) -> Result<Image, PlayError> {
    if options.still || shape(source) != Shape::App {
        return render(source, options).map(|png| Image {
            picture: Picture::Still(png),
            note: None,
        });
    }
    validate(options)?;
    rawr::resolve::deny_files(NO_FILES);
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
    };
    let mut loops = motion::Loops::default();
    let mut encoder = motion::Encoder::new(motion::SCALE);
    let recording = rawr_iced::snapshot::record_application(
        &mut adapter,
        messages,
        canvas,
        bound,
        theme,
        motion,
        &mut |frames| loops.check(frames),
        &mut |frames, background| encoder.push(frames, background),
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
/// its view settles after its boot and messages.
pub fn render(source: &str, options: &Options) -> Result<Png, PlayError> {
    validate(options)?;
    rawr::resolve::deny_files(NO_FILES);
    let program = wrap(source, options)?;
    let deadline = Instant::now() + options.limits.time;
    let canvas = canvas(options);
    let rendered = if shape(source) == Shape::App {
        render_application(&program, source, options, canvas, Deadline::new(deadline))
    } else {
        let sandbox = sandbox(&options.limits, Deadline::new(deadline));
        let compiler = rawr_iced::preview::Compiler::configured(sandbox)
            .map_err(|error| PlayError::Render(error.to_string()))?;
        let checked = compiler
            .check_named_source(SOURCE_NAME, &program)
            .map_err(|error| {
                PlayError::Compile(report(&error.report(diagnostic(&program)), &program, source))
            })?;
        checked
            .render_png_with(canvas)
            .map_err(|error| classify(&error.to_string(), &options.limits))
    };
    if Instant::now() >= deadline {
        return Err(PlayError::Limit(TIME_LIMIT.into()));
    }
    rendered
}

/// A full app runs as `rawr run` would show it, headless: booted, given
/// the options' messages, and drawn once its view settles.
fn render_application(
    program: &str,
    source: &str,
    options: &Options,
    canvas: rawr_iced::snapshot::Canvas,
    deadline: Deadline,
) -> Result<Png, PlayError> {
    let (mut adapter, messages) = application(program, source, options, deadline)?;
    // The app's own `.theme(f)` wins over the built-in theme.
    let theme = app_theme(source).is_none().then(|| options.theme.variant());
    let bound = options.size.unwrap_or((MAX_SIDE, MAX_SIDE));
    rawr_iced::snapshot::render_application(&mut adapter, messages, canvas, bound, theme)
        .map_err(|error| classify(&error.to_string(), &options.limits))
}

/// A full app checked and booted, and the options' messages for it.
fn application(
    program: &str,
    source: &str,
    options: &Options,
    deadline: Deadline,
) -> Result<(rawr_iced::program::Adapter, Vec<rawr_iced::runtime::Message>), PlayError> {
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
        .and_then(rawr_iced::application::Checked::prepare)
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
        let refuse = || Err::<(), Box<rawr::EvalAltResult>>("sleep is not available in ?play".into());
        engine
            .register_fn("sleep", move |_: rawr::INT| refuse())
            .register_fn("sleep", move |_: rawr::FLOAT| refuse());
    }
}

/// The function a full app's `index`th message is wrapped as.
const MESSAGE: &str = "rawr_play_message_";

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

/// A report as the user's own source shows it: what [`wrap`] added to a
/// line (the imports and `#[preview]` on the first) is cut from the quoted
/// line, its marker and its columns. A position on a line the wrapping
/// added after the source moves to the end of the source's last line.
fn report(report: &rawr::diagnostic::Report, program: &str, source: &str) -> String {
    let text = rawr::diagnostic::render(report);
    let source = without_tests(source);
    let own: Vec<&str> = source.split('\n').collect();
    // The last line with text: a source ending in a newline ends on it.
    let last = own.iter().rposition(|line| !line.trim().is_empty()).map_or(1, |at| at + 1);
    let end = own[last - 1].chars().count();
    let added = program
        .split('\n')
        .zip(source.split('\n'))
        .enumerate()
        .filter_map(|(index, (wrapped, own))| {
            added_span(wrapped, own).map(|span| (index + 1, span))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut out = String::with_capacity(text.len());
    // The quoted line's added span, and whether it was moved to `last`.
    let mut quoted = None;
    let mut moved = false;
    for line in text.lines() {
        let rewritten = if let Some(location) = line
            .strip_prefix(" --> ")
            .or_else(|| line.strip_prefix(" ::: "))
            .and_then(|rest| rest.strip_prefix(SOURCE_NAME))
            .and_then(|rest| rest.strip_prefix(':'))
        {
            quoted = None;
            moved = false;
            let arrow = &line[..5];
            let location = shift_location(location, &added, last, end);
            format!("{arrow}{SOURCE_NAME}:{location}")
        } else if let Some((number, rest)) = quoted_line(line) {
            let gutter = &line[..line.len() - rest.len()];
            moved = number > last;
            quoted = if moved { None } else { added.get(&number).copied() };
            if moved {
                let width = gutter.len() - 3;
                format!("{last:>width$} | {}", own[last - 1])
            } else {
                match quoted {
                    Some(span) => format!("{gutter}{}", cut(rest, span)),
                    None => line.to_owned(),
                }
            }
        } else if let Some(at) = line.find('|')
            && line[..at].trim().is_empty()
            && (moved || quoted.is_some())
        {
            let marker = line.get(at + 2..).unwrap_or_default();
            let rest = if std::mem::take(&mut moved) {
                format!("{}^", " ".repeat(end))
            } else {
                cut(marker, quoted.take().expect("a quoted span"))
            };
            format!("{}{rest}", &line[..at + 2])
        } else {
            without_positions(line)
        };
        out.push_str(&rewritten);
        out.push('\n');
    }
    out
}

/// `line` without a runtime error's ` (line 2, position 1)`: it counts in
/// the wrapped program, and the ` --> ` line shows where it is.
fn without_positions(line: &str) -> String {
    let mut out = line.to_owned();
    while let Some(at) = out.find(" (line ") {
        let rest = &out[at + " (line ".len()..];
        let digits = |text: &str| text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let line_digits = digits(rest);
        let Some(after) = rest[line_digits..].strip_prefix(", position ") else {
            break;
        };
        let position_digits = digits(after);
        if line_digits == 0 || position_digits == 0 || !after[position_digits..].starts_with(')') {
            break;
        }
        let end = out.len() - after.len() + position_digits + 1;
        out.replace_range(at..end, "");
    }
    out
}

/// Where `wrapped` adds text to `own`: the char column it starts at and its
/// length in chars.
fn added_span(wrapped: &str, own: &str) -> Option<(usize, usize)> {
    let wrapped: Vec<char> = wrapped.chars().collect();
    let own: Vec<char> = own.chars().collect();
    if wrapped.len() <= own.len() {
        return None;
    }
    let start = wrapped.iter().zip(&own).take_while(|(a, b)| a == b).count();
    let length = wrapped.len() - own.len();
    (wrapped[start + length..] == own[start..]).then_some((start, length))
}

/// ` 12 | text`: the quoted line's number and text.
fn quoted_line(line: &str) -> Option<(usize, &str)> {
    let (gutter, rest) = line.split_once(" | ")?;
    let number = gutter.trim().parse().ok()?;
    Some((number, rest))
}

fn cut(text: &str, (start, length): (usize, usize)) -> String {
    text.chars()
        .enumerate()
        .filter(|(index, _)| *index < start || *index >= start + length)
        .map(|(_, c)| c)
        .collect()
}

/// `1:310-1:324` with the added columns taken out, and a line past `last`
/// moved to its end, column `end + 1`.
fn shift_location(
    location: &str,
    added: &std::collections::BTreeMap<usize, (usize, usize)>,
    last: usize,
    end: usize,
) -> String {
    location
        .split('-')
        .map(|point| {
            let Some((line, column)) = point.split_once(':') else {
                return point.to_owned();
            };
            let (Ok(number), Ok(column)) = (line.parse::<usize>(), column.parse::<usize>()) else {
                return point.to_owned();
            };
            if number > last {
                return format!("{last}:{}", end + 1);
            }
            let column = match added.get(&number) {
                Some(&(start, length)) if column > start + length => column - length,
                Some(&(start, _)) if column > start => start + 1,
                _ => column,
            };
            format!("{number}:{column}")
        })
        .collect::<Vec<_>>()
        .join("-")
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
        let what = text.rsplit(": ").next().unwrap_or(text);
        format!(
            "size limit exceeded: {what} (strings up to {} bytes, arrays and maps up to {} items)",
            limits.string_size, limits.array_size
        )
    } else {
        return PlayError::Render(text.to_owned());
    };
    PlayError::Limit(reason)
}

/// The snippet shape [`wrap`] recognized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    /// The source declares its own `#[preview]`; refused.
    Preview,
    /// A type with `fn view(&self)`.
    App,
    /// A stateless `fn view()`.
    View,
    /// A bare expression.
    Expression,
}

/// Classify a snippet without wrapping it.
#[must_use]
pub fn shape(source: &str) -> Shape {
    if source.contains("#[preview") {
        Shape::Preview
    } else if runs_itself(source) {
        // `iced::run(|state: &mut u32, message: Message| .., |state: &u32| ..)`
        // has no `fn view` of its own.
        Shape::App
    } else if let Some(at) = find_view(source) {
        let parameters = source[at..]
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map_or("", |(parameters, _)| parameters.trim());
        if !parameters.is_empty() {
            Shape::App
        } else {
            Shape::View
        }
    } else {
        Shape::Expression
    }
}

/// Wrap a snippet into a program: a view or an expression gets one
/// `#[preview]`; a full app runs as itself, given a `main` when it has none
/// and each message as a function returning its Message.
///
/// User lines keep their line numbers: added imports and attributes share
/// the first line or the `fn view` line, and helpers go after the source.
pub fn wrap(source: &str, options: &Options) -> Result<String, PlayError> {
    let (width, height) = options.size.unwrap_or((MAX_SIDE, MAX_SIDE));
    let size = format!("size = ({width}, {height})");
    let source = &without_tests(source);
    let source = source.as_str();
    let theme_fn = format!(
        "\nfn rawr_play_theme() -> iced::Theme {{ iced::Theme::{} }}\n",
        options.theme.variant()
    );
    let themes = "themes = [rawr_play_theme]";
    let shape = shape(source);
    if !options.messages.is_empty() && shape != Shape::App {
        return Err(PlayError::Options(
            "messages need a full app with `fn view(&self)` and `fn update(&mut self, ..)`".into(),
        ));
    }
    // rawr-iced's prelude, for `fn view()` and expression snippets that
    // import nothing themselves. A `use` inside a function body imports
    // only there.
    let prelude = if has_top_level_use(source) {
        String::new()
    } else {
        iced_claims().prelude_imports()
    };
    Ok(match shape {
        // A hand-written `#[preview]` could pick any canvas size.
        Shape::Preview => {
            return Err(PlayError::Options(
                "?play adds `#[preview]` itself; remove it and use size=WxH".into(),
            ));
        }
        Shape::Expression => format!(
            "{prelude}#[preview({size}, {themes})] fn view() -> impl iced::widget::Widget<()> {{ {source}\n}}{theme_fn}"
        ),
        Shape::View => {
            let at = find_view(source).expect("view shape has a view fn");
            format!(
                "{prelude}{}#[preview({size}, {themes})] {}{theme_fn}",
                &source[..at],
                &source[at..]
            )
        }
        // A full app runs as itself: given a `main` when it has none, and
        // each message as a function of its Message type.
        Shape::App => {
            let mut program = source.to_owned();
            if !runs_itself(source) {
                let at = find_view(source).expect("app shape has a view fn");
                let state = app_state(source, at).ok_or_else(|| {
                    PlayError::Options("cannot find the State type that owns `fn view`".into())
                })?;
                let callbacks = if view_signature(&source[at..]).contains("self") {
                    format!("{state}::update, {state}::view")
                } else {
                    "update, view".into()
                };
                program.push_str(&format!(
                    "\npub fn main() -> iced::Result {{ iced::run({callbacks}) }}\n"
                ));
            }
            if !options.messages.is_empty() {
                let message = message_type(source).ok_or_else(|| {
                    PlayError::Options("cannot find the Message type `fn update` takes".into())
                })?;
                for (index, expression) in options.messages.iter().enumerate() {
                    program.push_str(&format!(
                        "\nfn {MESSAGE}{index}() -> {message} {{ {expression} }}\n"
                    ));
                }
            }
            program
        }
    })
}

/// Whether `source` starts its own app with `iced::run` or
/// `iced::application`.
fn runs_itself(source: &str) -> bool {
    source.contains("iced::run(") || source.contains("iced::application(")
}

/// Whether a line of `source` outside every brace declares an inline
/// module: `mod name {` or `pub mod name {`.
fn has_top_level_mod(source: &str) -> bool {
    let mut depth = 0usize;
    for line in source.lines() {
        let item = line.trim_start();
        let item = item.strip_prefix("pub ").unwrap_or(item);
        if depth == 0 && item.starts_with("mod ") && item.contains('{') {
            return true;
        }
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

/// Whether a line of `source` outside every brace starts with `use `.
fn has_top_level_use(source: &str) -> bool {
    let mut depth = 0usize;
    for line in source.lines() {
        if depth == 0 && line.trim_start().starts_with("use ") {
            return true;
        }
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

/// The parameters of the `fn view` at the start of `view`.
fn view_signature(view: &str) -> &str {
    view.split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map_or("", |(parameters, _)| parameters)
}

/// The type of `fn update`'s last parameter, or of the update closure's
/// that `iced::run` takes: the app's Message.
fn message_type(source: &str) -> Option<String> {
    let Some((_, rest)) = source.split_once("fn update(") else {
        return closure_message_type(source);
    };
    let mut depth = 0usize;
    let end = rest.char_indices().find_map(|(at, c)| match c {
        '(' | '<' | '[' => {
            depth += 1;
            None
        }
        ')' if depth == 0 => Some(at),
        ')' | '>' | ']' => {
            depth = depth.saturating_sub(1);
            None
        }
        _ => None,
    })?;
    last_parameter_type(&rest[..end])
}

/// The last parameter type of the first closure after `iced::run(` or
/// `iced::application(` that takes `&mut` state: its update's Message.
fn closure_message_type(source: &str) -> Option<String> {
    let start = source
        .find("iced::run(")
        .or_else(|| source.find("iced::application("))?;
    let rest = &source[start..];
    let mut from = 0;
    while let Some(open) = rest[from..].find('|') {
        let open = from + open + 1;
        let close = open + rest[open..].find('|')?;
        let parameters = &rest[open..close];
        if parameters.contains("&mut") {
            return last_parameter_type(parameters);
        }
        from = close + 1;
    }
    None
}

/// The type of the last of `parameters`, as `name: Type` or `_: Type`.
fn last_parameter_type(parameters: &str) -> Option<String> {
    let mut depth = 0usize;
    let last = parameters
        .char_indices()
        .filter(|(_, c)| {
            match c {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            *c == ',' && depth == 0
        })
        .last()
        .map_or(parameters, |(at, _)| &parameters[at + 1..]);
    let (_, ty) = last.split_once(':')?;
    let ty = ty.trim();
    (!ty.is_empty()).then(|| ty.to_owned())
}

/// `source` with each `#[cfg(test)]` item (such as `mod tests { .. }`)
/// blanked, keeping line numbers: a pasted example often ends with tests.
fn without_tests(source: &str) -> String {
    const ATTRIBUTE: &str = "#[cfg(test)]";
    let mut out = source.to_owned();
    let mut from = 0;
    while let Some(found) = out[from..].find(ATTRIBUTE) {
        let start = from + found;
        let mut depth = 0usize;
        let mut end = out.len();
        for (offset, c) in out[start + ATTRIBUTE.len()..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + ATTRIBUTE.len() + offset + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = start + ATTRIBUTE.len() + offset + 1;
                    break;
                }
                _ => {}
            }
        }
        let blank: String = out[start..end]
            .chars()
            .map(|c| if c == '\n' { c } else { ' ' })
            .collect();
        out.replace_range(start..end, &blank);
        from = start + blank.len();
    }
    out
}

/// Byte offset of `fn view` (or `pub fn view`) followed by `(`.
fn find_view(source: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = source[from..].find("fn view") {
        let at = from + found;
        let after = source[at + "fn view".len()..].trim_start();
        let boundary = at == 0
            || !source[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if boundary && after.starts_with('(') {
            let before = source[..at].trim_end();
            return Some(match before.strip_suffix("pub") {
                Some(rest) => rest.len(),
                None => at,
            });
        }
        from = at + 1;
    }
    None
}

/// The State type for a view at `at`: the nearest preceding `impl Name`, or
/// the `&Name` parameter of a free `fn view(state: &Name)`.
/// The path an app passes to `.theme(..)`, such as `App::theme` or `ink`.
fn app_theme(source: &str) -> Option<&str> {
    let (_, rest) = source.split_once(".theme(")?;
    let path = rest[..rest.find(')')?].trim();
    let valid = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':');
    valid.then_some(path)
}

fn app_state(source: &str, at: usize) -> Option<String> {
    let ident = |text: &str| -> String {
        text.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    };
    let view = view_signature(&source[at..]);
    if !view.contains("self") {
        let (_, rest) = view.split_once('&')?;
        let name = ident(rest.trim_start());
        return (!name.is_empty()).then_some(name);
    }
    let impl_at = source[..at].rfind("impl ")?;
    let name = ident(source[impl_at + "impl ".len()..].trim_start());
    (!name.is_empty()).then_some(name)
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
        let source = "fn view() -> impl Widget<()> {\n    row![\"left\", text(\"right\")].spacing(8)\n}\n";
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
        assert_eq!(&rgba[center..center + 3], &[0xff, 0x2e, 0x93], "the circle is drawn");
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
    fn render_size_and_depth_limits() {
        let huge = "fn view() -> impl Widget<()> {\n    let mut s = String::new();\n    for i in 0..10000000 { s.push_str(\"xxxxxxxxxxxxxxxxxxxx\"); }\n    text(s)\n}\n";
        let error = render(huge, &Options::default()).unwrap_err();
        assert!(matches!(error, PlayError::Limit(_)) && error.text().contains("size limit"), "{error:?}");
        let deep = "fn f(n: i64) -> i64 { f(n + 1) + 1 }\nfn view() -> impl Widget<()> { text(f(0)) }\n";
        let error = render(deep, &Options::default()).unwrap_err();
        assert!(matches!(error, PlayError::Limit(_)) && error.text().contains("call depth"), "{error:?}");
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
        let preview = "#[preview(size = (60000, 60000))]\nfn view() -> impl Widget<()> { text(\"x\") }\n";
        let error = render(preview, &Options::default()).unwrap_err();
        assert!(matches!(error, PlayError::Options(_)), "{error:?}");
    }

    #[test]
    fn render_encodes_jpeg_and_webp() {
        let options = Options { size: Some((64, 32)), ..Options::default() };
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
        assert!(super::has_top_level_use("use iced::widget::text;\nfn view() {}"));
        assert!(!super::has_top_level_use("fn view() {\n    use iced::widget::text;\n}"));
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
        assert!(text.contains(" --> play.rs:1:12\n"), "{text}");
        assert!(text.contains(" 1 | text(\"hi\").bogus(1)\n"), "{text}");
        assert!(text.contains("   |            ^\n"), "{text}");
        assert!(!text.contains("use iced"), "{text}");
    }

    #[test]
    fn render_fits_the_content() {
        let small = render("text(\"hi\")", &Options::default()).unwrap();
        let tall = render("column![text(\"a\"), text(\"b\"), text(\"c\")]", &Options::default()).unwrap();
        // A margin on each side at 2x, plus the text, well under the bound.
        assert!(small.width > 4 * super::MARGIN && small.width < 200, "{}x{}", small.width, small.height);
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
        let source = include_str!("../tests/fixtures/component.rs");
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
        let cart = text(include_str!("../../examples/scripts/cart.rawr"));
        assert_eq!(cart.text(), "total: 27\n");
        assert_eq!((cart.status, cart.truncated, cart.limit), (0, false, false));
    }

    #[test]
    fn run_plain_program_interleaves_streams_in_order() {
        use super::plain::Stream::{Stderr, Stdout};
        let output = text("fn main() {\n    println!(\"a\");\n    eprintln!(\"b\");\n    print!(\"c\");\n    println!(\"d\");\n}\n");
        assert_eq!(
            output.chunks,
            [(Stdout, "a\n".into()), (Stderr, "b\n".into()), (Stdout, "cd\n".into())]
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
        assert!(text.starts_with("before\n"), "{text}");
        assert!(text.contains("assertion `left == right` failed\n  left: 2\n right: 3"), "{text}");
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
    fn animate_for(source: &str, time: u64, animation: u64) -> (std::time::Duration, Option<String>) {
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

    #[test]
    fn budget_switches_once_the_view_changes() {
        // The view changes, so the spin runs to the animation budget.
        let (ran, note) = animate_for(TICK_THEN_SPIN, 1_000, 4_000);
        assert!(ran >= std::time::Duration::from_millis(4_000), "{ran:?}");
        let note = note.unwrap();
        assert!(note.starts_with("Rendering stopped after 4 s, so the animation shows its first "), "{note}");
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
        let source = "fn main() {\n    println!(\"start\");\n    let mut n = 0;\n    loop { n += 1; }\n}\n";
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
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "stopped once full");
        assert!(output.truncated && output.limit && output.status == 1);
        let text = output.text();
        assert!(text.len() <= super::plain::MAX_OUTPUT + 64, "{}", text.len());
        assert!(text.ends_with("… output truncated\n"), "{}", &text[text.len() - 40..]);
    }

    #[test]
    fn run_plain_compile_error_is_a_diagnostic() {
        let error = super::run("fn main() {\n    let x: i64 = \"s\";\n}\n", &Options::default()).unwrap_err();
        let PlayError::Compile(text) = error else {
            panic!("expected a compile error, got {error:?}");
        };
        assert!(text.contains("play.rs:2:"), "{text}");
    }

    #[test]
    fn run_iced_snippets_still_render() {
        // Named `iced`, or only names of rawr-iced's prelude.
        assert!(is_png(&image("column![text(\"hi\")]").bytes));
        assert!(is_png(&image("fn view() -> impl Widget<()> {\n    text(\"x\")\n}\n").bytes));
        assert!(is_png(&image(include_str!("../tests/fixtures/counter.rs")).bytes));
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
        let super::Outcome::Text(output) = super::run("println!(\"Foo\")", &options).unwrap() else {
            panic!("a plain program runs as text");
        };
        assert_eq!((output.text().as_str(), output.status), ("Foo\n", 0));
        assert_eq!(output.note.as_deref(), Some(super::plain::MESSAGES_IGNORED));
        assert_eq!(super::Output::from_json(output.to_json().as_bytes()).unwrap(), output);
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
        assert!(super::iced_claims().prelude.iter().any(|path| path.ends_with("::text")));
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

    #[test]
    fn render_iced_counter_example_verbatim() {
        let source = include_str!("../tests/fixtures/counter.rs");
        assert_eq!(shape(&super::without_tests(source)), Shape::App);
        let png = render(source, &Options::default()).unwrap();
        assert!(is_png(&png.bytes));
        assert!(png.width < 600 && png.height < 600, "{}x{}", png.width, png.height);
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
        assert_eq!((output.text().as_str(), output.status, output.note), ("", 0, None));
    }

    #[test]
    fn play_endless_loop_runs_to_a_limit() {
        let source = r#"let mut iteration = 0; loop { iteration += 1; println!("Iteration No{iteration}"); }"#;
        let output = text(source);
        assert!(output.limit && output.truncated, "{output:?}");
        assert!(output.text().starts_with("Iteration No1\nIteration No2\n"));
        assert!(output.text().ends_with(&format!("\n{}\n", super::plain::TRUNCATED)));
        assert!(!output.text().contains("Debug"));
    }

    #[test]
    fn play_errors_never_show_the_wrapper() {
        for source in ["let x = 1; x +", "let x = (1 + 2;", "[1, 2].iter().map(|x| x.nope())"] {
            let error = super::run(source, &Options::default()).unwrap_err();
            let text = error.to_string();
            for wrapper in ["__v", "__d", "fn main", "play.rawr"] {
                assert!(!text.contains(wrapper), "{source}: {text}");
            }
            assert!(text.contains(" --> play.rs:1:"), "{source}: {text}");
            assert!(text.contains(&format!(" 1 | {source}\n")), "{source}: {text}");
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
    fn an_app_with_an_inline_module_renders() {
        let source = include_str!("../tests/fixtures/counter.rs")
            .replace("text(self.value)", "text(label::of(self.value))");
        let source = format!("mod label {{\n    pub fn of(value: i64) -> String {{\n        format!(\"count {{value}}\")\n    }}\n}}\n{source}");
        let with_module = image(&source);
        let inline = image(
            &include_str!("../tests/fixtures/counter.rs")
                .replace("text(self.value)", "text(format!(\"count {}\", self.value))"),
        );
        assert!(is_png(&with_module.bytes));
        assert_eq!(with_module.bytes, inline.bytes);
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
