//! Deterministic chrome-free snapshots of compiler-proven previews.

mod artifact;
pub mod clock;
mod commit;
mod encoding;
mod error;
pub mod fonts;
mod generation;
mod headless;
mod image;
mod inventory;
mod manifest;
mod store;
#[doc(hidden)]
pub mod wire;

pub use commit::{Commit, Notice};
pub use error::{Class, Error};
pub use generation::{Difference, Outcome};

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rawr::preview;

/// One headlessly rendered, PNG-encoded preview.
#[derive(Clone, Debug)]
pub struct Png {
    /// Physical width in pixels.
    pub width: u32,
    /// Physical height in pixels.
    pub height: u32,
    /// Encoded PNG bytes (RGBA8).
    pub bytes: Vec<u8>,
}

/// How a one-off render sizes and scales its image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    /// Size the image to the content laid out within the preview's declared
    /// size (the bound), instead of to the declared size itself.
    pub fit: bool,
    /// Logical size used on an axis the content fills (a `Fill` width).
    pub fallback: (u32, u32),
    /// Background margin around the content, in logical pixels.
    pub margin: u32,
    /// Pixels per logical pixel (2.0 for HiDPI).
    pub scale: f32,
    /// The scale is lowered so the larger side stays within this many pixels.
    pub max_pixels: u32,
}

/// Render the first preview of `catalog` with its first declared theme.
pub(crate) fn render_first(
    catalog: Arc<preview::BoundCatalog>,
    canvas: Option<Canvas>,
) -> Result<Png, Error> {
    let authority = crate::preview::catalog::Authority::static_(catalog);
    let definition = authority.previews().first().ok_or(Error::Empty)?;
    let theme = artifact::themes(definition)
        .into_iter()
        .find(|theme| matches!(theme, artifact::Theme::Explicit { .. }))
        .unwrap_or(artifact::Theme::Native);
    let mut renderer = match canvas {
        Some(canvas) => headless::Renderer::with_canvas(canvas)?,
        None => headless::Renderer::new()?,
    };
    let artifact = artifact::Artifact::render(&authority, definition, theme, &mut renderer)?;
    let image = artifact.image();
    Ok(Png {
        width: image.width(),
        height: image.height(),
        bytes: image.encode()?,
    })
}

/// Render a booted application as iced's runtime first shows it: its boot
/// Task and `messages` applied (`Adapter::deliver`), then its view drawn
/// once it settles, each message its widgets publish applied between
/// frames. `theme` names a built-in theme to draw with instead of the
/// application's own; `bound` is the canvas's declared size.
pub fn render_application(
    adapter: &mut crate::program::Adapter,
    messages: Vec<crate::runtime::Message>,
    canvas: Canvas,
    bound: (u32, u32),
    theme: Option<&str>,
) -> Result<Png, Error> {
    let mut renderer = headless::Renderer::with_canvas(canvas)?;
    adapter.deliver(messages);
    let theme = theme
        .and_then(|name| {
            iced::Theme::ALL
                .iter()
                .find(|theme| theme.to_string() == name)
                .cloned()
        })
        .unwrap_or_else(|| adapter.theme().native().clone());
    let frame = renderer.framing();
    // The size of the settled content, then the view with its diagnostic
    // overlays, which fill the window.
    let size = renderer.size(
        &mut |published| {
            adapter.deliver(published);
            frame(adapter.content())
        },
        iced::Size::new(bound.0 as f32, bound.1 as f32),
    );
    let mut view = |published: Vec<crate::runtime::Message>| {
        adapter.deliver(published);
        frame(adapter.view())
    };
    let image = renderer.render(&mut view, &theme, size)?;
    Ok(Png {
        width: image.width(),
        height: image.height(),
        bytes: image.encode()?,
    })
}

/// A wall-clock deadline that can move later, shared by its clones: a
/// recording moves it once the view changes, and an engine's progress
/// callback reads the same one.
#[derive(Clone, Debug)]
pub struct Deadline {
    base: std::time::Instant,
    /// Nanoseconds from `base`.
    after: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Deadline {
    /// A deadline at `at`.
    #[must_use]
    pub fn new(at: std::time::Instant) -> Self {
        let base = std::time::Instant::now();
        let deadline = Self {
            base,
            after: std::sync::Arc::default(),
        };
        deadline.extend(at);
        deadline
    }

    /// The instant it is at.
    #[must_use]
    pub fn at(&self) -> std::time::Instant {
        let after = self.after.load(std::sync::atomic::Ordering::Relaxed);
        self.base + std::time::Duration::from_nanos(after)
    }

    /// Whether the wall clock has reached it.
    #[must_use]
    pub fn past(&self) -> bool {
        std::time::Instant::now() >= self.at()
    }

    /// Move it to `at`, if that is later.
    pub fn extend(&self, at: std::time::Instant) {
        let after = at.saturating_duration_since(self.base).as_nanos();
        let after = u64::try_from(after).unwrap_or(u64::MAX);
        self.after
            .fetch_max(after, std::sync::atomic::Ordering::Relaxed);
    }
}

/// How [`record_application`] runs an application on its simulated clock.
#[derive(Clone, Debug)]
pub struct Motion {
    /// Simulated time between frames.
    pub step: std::time::Duration,
    /// Most frames recorded.
    pub frames: usize,
    /// Most frames after the boot or a message before the next message is
    /// applied, when they never settle.
    pub phase: usize,
    /// Longest simulated time recorded after the last message.
    pub span: std::time::Duration,
    /// The wall-clock instant recording stops at.
    pub deadline: Deadline,
    /// Where `deadline` moves once a frame differs from the first: an
    /// animation's longer budget. No later than `deadline` keeps it.
    pub extended: std::time::Instant,
    /// Set from outside to stop recording before the next frame, keeping
    /// the frames so far: someone else is waiting for the renderer.
    pub cut: Option<&'static std::sync::atomic::AtomicBool>,
    /// The scale frames are drawn at once a frame differs from the first,
    /// when lower than the canvas's: an animation's frames can be smaller
    /// than a still image.
    pub animation_scale: Option<f32>,
}

/// One frame of a [`Recording`].
#[derive(Clone, Debug)]
pub struct Frame {
    /// Physical width in pixels.
    pub width: u32,
    /// Physical height in pixels.
    pub height: u32,
    /// RGBA8 pixels, row by row.
    pub rgba: Vec<u8>,
    /// Simulated time since the recording began.
    pub time: std::time::Duration,
    /// What ran before it: 0 for the boot, `i` once the `i`th message is
    /// applied.
    pub phase: usize,
    /// Pixels per logical pixel it was drawn at.
    pub scale: f32,
    /// The background color of the theme it was drawn with, RGBA8.
    pub background: [u8; 4],
}

impl Frame {
    /// Whether `other` has the same size and pixels.
    #[must_use]
    pub fn same_image(&self, other: &Frame) -> bool {
        (self.width, self.height) == (other.width, other.height) && self.rgba == other.rgba
    }
}

/// Why a [`Recording`] ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ending {
    /// The last frame is settled: no stream the clock drives runs, and
    /// nothing asked to be redrawn.
    Settled,
    /// The `stop` predicate ended it.
    Stopped,
    /// It ran [`Motion::frames`] frames or [`Motion::span`] after the last
    /// message.
    Cap,
    /// The frames ran out before every message was applied. The last frame
    /// is drawn after all of them.
    Messages,
    /// The wall clock reached [`Motion::deadline`], moved to
    /// [`Motion::extended`] when the view changed.
    Deadline,
    /// [`Motion::cut`] was set.
    Cut,
}

/// The frames an application drew on a simulated clock.
#[derive(Clone, Debug)]
pub struct Recording {
    /// The frames, in order. Their sizes differ when the content's does.
    pub frames: Vec<Frame>,
    /// Why it ended.
    pub ending: Ending,
}

/// Record a booted application on a simulated clock: a frame every
/// [`Motion::step`], each drawn as [`render_application`] draws one.
///
/// The boot's Task runs first, then each of `messages` in turn. After each
/// of them the frames run until the view settles, so each message's
/// transition shows. `time::every` ticks and `window::frames` fire as the
/// clock moves, and `Instant::now()` reads it. Freeze the clock with
/// [`clock::freeze`] before the application boots so its boot reads the
/// same clock; otherwise it freezes here. After the last message, `stop`
/// sees the frames recorded since and ends the recording when it returns
/// `true`. `each` sees every frame recorded so far after each frame; the
/// time it takes counts against the deadline. Each frame is drawn with the
/// theme `theme` names, or else the application's theme as its State has
/// it then, so a message that changes the theme changes the colors.
pub fn record_application(
    adapter: &mut crate::program::Adapter,
    messages: Vec<crate::runtime::Message>,
    canvas: Canvas,
    bound: (u32, u32),
    theme: Option<&str>,
    motion: Motion,
    stop: &mut dyn FnMut(&[Frame]) -> bool,
    each: &mut dyn FnMut(&[Frame]),
) -> Result<Recording, Error> {
    use iced::time::Instant;
    use iced::window::RedrawRequest;

    let _frozen = clock::now().is_none().then(clock::freeze);
    let start = clock::now().expect("the clock is frozen");
    let mut renderer = headless::Renderer::with_canvas(canvas)?;
    adapter.deliver(Vec::new());
    let named = theme.and_then(|name| {
        iced::Theme::ALL
            .iter()
            .find(|theme| theme.to_string() == name)
            .cloned()
    });
    let framing = renderer.framing();
    let declared = iced::Size::new(bound.0 as f32, bound.1 as f32);
    let mut trees = headless::Trees::default();
    let mut scale = None;
    let mut streams = Streams::default();
    let mut frames: Vec<Frame> = Vec::new();
    let last = messages.len();
    let mut messages = messages.into_iter();
    let mut now: Instant = start;
    let mut changed = false;
    let ending = 'record: {
        for phase in 0..=last {
            if phase > 0 {
                adapter.deliver(messages.next());
            }
            let first = frames.len();
            let began = now;
            loop {
                if motion.deadline.past() {
                    break 'record Ending::Deadline;
                }
                if motion.cut.is_some_and(|cut| cut.load(std::sync::atomic::Ordering::Relaxed)) {
                    break 'record Ending::Cut;
                }
                let capped = frames.len() >= motion.frames;
                if capped && phase < last {
                    adapter.deliver(messages.by_ref());
                }
                if phase == last && (capped || now - began >= motion.span) {
                    break 'record Ending::Cap;
                }
                streams.sync(adapter);
                clock::set(now);
                let theme = named.clone().unwrap_or_else(|| adapter.theme().native().clone());
                // The content sizes the image; the view, with its
                // diagnostic overlays, fills it.
                let mut view = |published: Vec<crate::runtime::Message>, content: bool| {
                    adapter.deliver(published);
                    framing(if content { adapter.content() } else { adapter.view() })
                };
                let (image, request, size) =
                    renderer.frame(&mut view, &mut trees, declared, now, &theme, &mut scale)?;
                if motion.deadline.past() {
                    break 'record Ending::Deadline;
                }
                let mut frame = Frame {
                    width: image.width(),
                    height: image.height(),
                    rgba: image.into_bytes(),
                    time: now - start,
                    phase: if capped { last } else { phase },
                    scale: scale.unwrap_or(1.0),
                    background: iced::theme::Base::base(&theme).background_color.into_rgba8(),
                };
                // The view changed: an animation gets the longer budget,
                // and its frames from this one on are drawn at its scale.
                // This frame is drawn again, so it matches the later frames
                // that show the same picture.
                if !changed && frames.first().is_some_and(|first| !first.same_image(&frame)) {
                    changed = true;
                    motion.deadline.extend(motion.extended);
                    let lowered = motion
                        .animation_scale
                        .and_then(|lower| scale.map(|scale| scale.min(lower)));
                    if let Some(lowered) = lowered.filter(|lowered| Some(*lowered) != scale) {
                        scale = Some(lowered);
                        let image = renderer.redraw(&mut trees, size, &theme, lowered)?;
                        frame.width = image.width();
                        frame.height = image.height();
                        frame.rgba = image.into_bytes();
                        frame.scale = lowered;
                    }
                }
                frames.push(frame);
                each(&frames);
                if capped {
                    break 'record Ending::Messages;
                }
                let armed = clock::armed();
                if !armed && request == RedrawRequest::Wait {
                    if phase == last {
                        break 'record Ending::Settled;
                    }
                    break;
                }
                if phase == last && stop(&frames[first..]) {
                    break 'record Ending::Stopped;
                }
                if phase < last && frames.len() - first >= motion.phase {
                    break;
                }
                // Wait for a redraw asked for later, unless the clock
                // drives a stream.
                now = match request {
                    RedrawRequest::At(at) if !armed && at > now + motion.step => at,
                    _ => now + motion.step,
                };
                clock::set(now);
                let ticks = streams.tick(now);
                adapter.deliver(ticks);
            }
        }
        unreachable!("the last phase ends the recording")
    };
    Ok(Recording { frames, ending })
}

/// The streams of an application's subscription, run by hand on a
/// recording's clock.
#[derive(Default)]
struct Streams {
    running: Vec<Running>,
}

struct Running {
    id: u64,
    input: iced::futures::channel::mpsc::UnboundedSender<iced::advanced::subscription::Event>,
    output: iced::futures::stream::BoxStream<'static, crate::runtime::Message>,
}

impl Streams {
    /// Start each stream the subscription has that is not running, and stop
    /// each running one it no longer has.
    fn sync(&mut self, adapter: &crate::program::Adapter) {
        use iced::advanced::subscription::{into_recipes, Hasher};
        use iced::futures::StreamExt as _;
        use std::hash::Hasher as _;

        let mut next: Vec<Running> = Vec::new();
        for recipe in into_recipes(adapter.lower_subscription(std::convert::identity)) {
            let mut hasher = Hasher::default();
            recipe.hash(&mut hasher);
            let id = hasher.finish();
            if next.iter().any(|running| running.id == id) {
                continue;
            }
            if let Some(at) = self.running.iter().position(|running| running.id == id) {
                next.push(self.running.swap_remove(at));
                continue;
            }
            let (input, events) = iced::futures::channel::mpsc::unbounded();
            let output = recipe.stream(events.boxed());
            next.push(Running { id, input, output });
        }
        self.running = next;
    }

    /// Show each stream a frame at `now` and take every message ready.
    fn tick(&mut self, now: iced::time::Instant) -> Vec<crate::runtime::Message> {
        use iced::futures::{FutureExt as _, StreamExt as _};

        /// Most messages one frame takes.
        const MOST: usize = 4096;
        let window = iced::window::Id::unique();
        let mut ready = Vec::new();
        self.running.retain_mut(|running| {
            let _ = running
                .input
                .unbounded_send(iced::advanced::subscription::Event::Interaction {
                    window,
                    event: iced::Event::Window(iced::window::Event::RedrawRequested(now)),
                    status: iced::event::Status::Ignored,
                });
            while ready.len() < MOST {
                match running.output.next().now_or_never() {
                    Some(Some(message)) => ready.push(message),
                    Some(None) => return false,
                    None => break,
                }
            }
            true
        });
        ready
    }
}

/// Snapshot operation selected by the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Compare a fresh actual generation without changing baselines.
    Check,
    /// Transactionally promote a fresh generation as the new baseline.
    Update,
}

/// One exact bound Catalog paired with its validated artifact roots.
pub struct Suite {
    catalog: crate::preview::catalog::Authority,
    store: store::Store,
    renderer: Mutex<Option<headless::Renderer>>,
    selection: Option<Selection>,
}

/// The variable naming one preview to render alone: `name`, `group/name`
/// or `subject#name`, then optionally a tab and a theme (`native`, or a
/// declared theme by its path or last segment). `rawr mcp` sets it on its
/// `rawr snapshot --check` child; the generated runner receives it too.
pub const SELECTION: &str = "RAWR_SNAPSHOT_ONLY";

/// One preview (and optionally one theme) to render instead of the suite.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    name: String,
    theme: Option<String>,
}

impl Selection {
    /// Parse [`SELECTION`]'s value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let (name, theme) = value
            .split_once('\t')
            .map_or((value, None), |(name, theme)| {
                (
                    name,
                    Some(theme.trim().to_owned()).filter(|theme| !theme.is_empty()),
                )
            });
        let name = name.trim();
        (!name.is_empty()).then(|| Self {
            name: name.to_owned(),
            theme,
        })
    }

    fn from_env() -> Option<Self> {
        std::env::var(SELECTION)
            .ok()
            .as_deref()
            .and_then(Self::parse)
    }

    fn preview(&self, definition: &preview::Definition) -> bool {
        let name = self.name.as_str();
        name.eq_ignore_ascii_case(definition.name())
            || name.eq_ignore_ascii_case(&format!("{}/{}", definition.group(), definition.name()))
            || name == definition.key().to_string()
    }

    fn theme(&self, theme: &artifact::Theme) -> bool {
        self.theme.as_deref().is_none_or(|wanted| match theme {
            artifact::Theme::Native => wanted.eq_ignore_ascii_case("native"),
            artifact::Theme::Explicit { name, .. } => {
                wanted.eq_ignore_ascii_case(name)
                    || name.rsplit("::").next().is_some_and(|last| {
                        wanted
                            .rsplit("::")
                            .next()
                            .is_some_and(|tail| tail.eq_ignore_ascii_case(last))
                    })
            }
        })
    }
}

impl Suite {
    pub(crate) fn new(catalog: Arc<preview::BoundCatalog>, root: PathBuf) -> Self {
        Self {
            catalog: crate::preview::catalog::Authority::static_(catalog),
            store: store::Store::new(root),
            renderer: Mutex::new(None),
            selection: Selection::from_env(),
        }
    }

    pub(crate) fn from_installation(
        catalog: rawr::reload::installation::installed::Catalog,
        root: PathBuf,
    ) -> Self {
        Self {
            catalog: crate::preview::catalog::Authority::reload(catalog),
            store: store::Store::new(root),
            renderer: Mutex::new(None),
            selection: Selection::from_env(),
        }
    }

    /// Render and check or promote one complete deterministic generation.
    ///
    /// With a [`Selection`], only the selected preview renders, into the
    /// actual tree alone: nothing is compared and no baseline changes.
    pub fn execute(&self, mode: Mode) -> Result<Outcome, Error> {
        let mut renderer = self.renderer.lock().map_err(|_| Error::RendererLock)?;
        if renderer.is_none() {
            *renderer = Some(headless::Renderer::new()?);
        }
        let renderer = renderer.as_mut().ok_or(Error::Renderer)?;
        let generation =
            generation::Generation::render(&self.catalog, renderer, self.selection.as_ref())?;
        match (mode, &self.selection) {
            (_, Some(_)) => self.store.actual_only(&generation),
            (Mode::Check, None) => self.store.check(&generation),
            (Mode::Update, None) => self.store.update(&generation),
        }
    }

    /// Return the committed baseline root.
    #[must_use]
    pub fn baseline(&self) -> &std::path::Path {
        self.store.baseline()
    }

    /// Return the disposable actual-artifact root.
    #[must_use]
    pub fn actual(&self) -> &std::path::Path {
        self.store.actual()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Class, Mode};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn complete_generation_is_ordered_pinned_and_baseline_immutable_on_check() {
        let project = Project::new(source());
        let suite = suite(&project);
        let updated = suite.execute(Mode::Update).unwrap();
        assert!(updated.updated());
        assert_eq!(updated.artifacts(), 2);

        let manifest = fs::read_to_string(suite.baseline().join("manifest.rawr-snapshot")).unwrap();
        assert!(manifest.contains(&format!("rawr\t{}", rawr::VERSION)));
        assert!(manifest.contains("renderer\ttiny-skia"));
        assert!(manifest.contains("font\t%46ira%20%53ans%20%52egular%20%28iced%20embedded%29"));
        assert!(manifest.contains("scale\t1.0"));
        let native = manifest.find("native.png").unwrap();
        let dark = manifest.find("dark.png").unwrap();
        assert!(native < dark, "native input must precede declared themes");
        assert!(!manifest.contains("%49gnored"));
        assert!(pngs(suite.baseline()).iter().all(|path| {
            path.strip_prefix(suite.baseline())
                .unwrap()
                .to_string_lossy()
                .starts_with("State-")
        }));

        let before = tree(suite.baseline());
        let checked = suite.execute(Mode::Check).unwrap();
        assert!(checked.is_successful());
        assert!(checked.differences().is_empty());
        assert_eq!(tree(suite.baseline()), before);
        assert_eq!(tree(suite.actual()), before);
        assert!(tree(suite.actual())
            .keys()
            .all(|path| !path.to_string_lossy().contains("rawr-preview-footer")));
        for path in pngs(suite.actual()) {
            let image = super::image::Image::read(&path).unwrap();
            assert_eq!((image.width(), image.height()), (96, 64));
            let row = usize::try_from(image.width()).unwrap() * 4;
            let bottom = &image.bytes()[image.bytes().len() - row..];
            assert!(
                bottom
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel == &bottom[..4]),
                "host footer or diagnostic chrome must not enter `{}`",
                path.display()
            );
        }
    }

    #[test]
    fn check_classifies_missing_pixel_size_and_stale_artifacts_and_ignores_build_facts() {
        let project = Project::new(source());
        let suite = suite(&project);
        suite.execute(Mode::Update).unwrap();
        let baseline_pngs = pngs(suite.baseline());
        let first = baseline_pngs.first().unwrap();

        fs::remove_file(first).unwrap();
        let missing = suite.execute(Mode::Check).unwrap();
        assert!(missing
            .differences()
            .iter()
            .any(|difference| difference.class() == Class::Missing));

        suite.execute(Mode::Update).unwrap();
        let first = pngs(suite.baseline()).remove(0);
        let image = super::image::Image::read(&first).unwrap();
        let mut bytes = image.bytes().to_vec();
        bytes[0] ^= 0xff;
        super::image::Image::new(image.width(), image.height(), bytes)
            .unwrap()
            .write(&first)
            .unwrap();
        let drift = suite.execute(Mode::Check).unwrap();
        assert!(drift
            .differences()
            .iter()
            .any(|difference| difference.class() == Class::Pixel));

        suite.execute(Mode::Update).unwrap();
        let first = pngs(suite.baseline()).remove(0);
        super::image::Image::new(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .write(&first)
            .unwrap();
        let size = suite.execute(Mode::Check).unwrap();
        assert!(size
            .differences()
            .iter()
            .any(|difference| difference.class() == Class::Size));

        suite.execute(Mode::Update).unwrap();
        let path = suite.baseline().join("stale.png");
        super::image::Image::new(1, 1, vec![0, 0, 0, 255])
            .unwrap()
            .write(&path)
            .unwrap();
        let stale = suite.execute(Mode::Check).unwrap();
        assert!(stale
            .differences()
            .iter()
            .any(|difference| difference.class() == Class::Stale));

        suite.execute(Mode::Update).unwrap();
        assert!(
            !path.exists(),
            "explicit update must remove stale artifacts"
        );
        // Build facts and preview proofs are recorded, never compared: equal
        // pixels pass whatever the manifest says about how they were made.
        let manifest = fs::read_to_string(suite.baseline().join("manifest.rawr-snapshot")).unwrap();
        let fingerprint = manifest
            .lines()
            .find_map(|line| line.strip_prefix("artifact\t"))
            .and_then(|row| row.split('\t').nth(1))
            .unwrap()
            .to_owned();
        let changed = manifest
            .replace("renderer\ttiny-skia", "renderer\tambient")
            .replace(&fingerprint, &"0".repeat(fingerprint.len()));
        fs::write(suite.baseline().join("manifest.rawr-snapshot"), changed).unwrap();
        let same = suite.execute(Mode::Check).unwrap();
        assert!(same.differences().is_empty(), "{:?}", same.differences());
    }

    #[test]
    fn corrupt_baseline_png_names_its_preview_theme_and_both_paths() {
        let project = Project::new(source());
        let suite = suite(&project);
        suite.execute(Mode::Update).unwrap();
        let expected = pngs(suite.baseline())
            .into_iter()
            .find(|path| path.file_name().is_some_and(|name| name == "native.png"))
            .unwrap();
        let relative = expected.strip_prefix(suite.baseline()).unwrap();
        let actual = suite.actual().join(relative);

        fs::write(&expected, b"not a PNG").unwrap();
        let error = suite.execute(Mode::Check).unwrap_err();
        assert_image_error(error, &expected, &actual, |source| {
            matches!(source, super::Error::Decode { .. })
        });

        suite.execute(Mode::Update).unwrap();
        let expected = pngs(suite.baseline())
            .into_iter()
            .find(|path| path.file_name().is_some_and(|name| name == "native.png"))
            .unwrap();
        grayscale(&expected, 96, 64);
        let relative = expected.strip_prefix(suite.baseline()).unwrap();
        let actual = suite.actual().join(relative);
        let error = suite.execute(Mode::Check).unwrap_err();
        assert_image_error(error, &expected, &actual, |source| {
            matches!(source, super::Error::Format { .. })
        });
    }

    #[test]
    fn a_selection_renders_one_preview_and_theme_and_the_parse_lists_them_all() {
        let project = Project::new(source());
        let target = rawr::project::Target::discover(&project.root).unwrap();
        let listed = rawr::package::Graph::load(&rawr::Engine::new(), &target)
            .unwrap()
            .previews();
        let names: Vec<(&str, &str, bool)> = listed
            .iter()
            .map(|preview| {
                (
                    preview.group.as_str(),
                    preview.name.as_str(),
                    preview.snapshot,
                )
            })
            .collect();
        assert_eq!(
            names,
            [("snapshot", "Default", true), ("snapshot", "Ignored", false)]
        );
        assert_eq!(listed[0].subject, "snapshot_fixture::State::view");
        assert_eq!(listed[0].themes, ["State::dark"]);
        assert_eq!(listed[0].size, (96, 64));

        let mut suite = suite(&project);
        suite.selection = super::Selection::parse("snapshot/default\tdark");
        let rendered = suite.execute(Mode::Check).unwrap();
        assert_eq!(rendered.artifacts(), 1);
        assert!(rendered.differences().is_empty());
        let actual = pngs(suite.actual());
        assert_eq!(actual.len(), 1);
        assert!(actual[0].ends_with("dark.png"), "{}", actual[0].display());
        assert!(!suite.baseline().exists(), "a selection writes no baseline");

        // A selection reaches a preview `rawr snapshot` skips.
        suite.selection = super::Selection::parse("ignored");
        assert_eq!(suite.execute(Mode::Check).unwrap().artifacts(), 1);
        suite.selection = super::Selection::parse("nope");
        assert!(suite.execute(Mode::Check).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn transactional_update_rejects_a_symlink_without_touching_its_generation() {
        use std::os::unix::fs::symlink;

        let project = Project::new(source());
        let suite = suite(&project);
        suite.execute(Mode::Update).unwrap();
        let retained = project.root.join("retained");
        fs::rename(suite.baseline(), &retained).unwrap();
        let before = tree(&retained);
        symlink(&retained, suite.baseline()).unwrap();

        assert!(suite.execute(Mode::Check).is_err());
        assert_eq!(tree(&retained), before);
        assert!(suite.execute(Mode::Update).is_err());
        assert_eq!(tree(&retained), before);
        assert!(fs::symlink_metadata(suite.baseline())
            .unwrap()
            .file_type()
            .is_symlink());
    }

    fn suite(project: &Project) -> super::Suite {
        let target = rawr::project::Target::discover(&project.root).unwrap();
        crate::preview::Compiler::new()
            .unwrap()
            .check(&target)
            .unwrap()
            .snapshots()
            .unwrap()
    }

    fn pngs(root: &Path) -> Vec<PathBuf> {
        tree(root)
            .into_keys()
            .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
            .map(|path| root.join(path))
            .collect()
    }

    fn grayscale(path: &Path, width: u32, height: u32) {
        let file = fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![0; (width * height) as usize])
            .unwrap();
    }

    fn assert_image_error(
        error: super::Error,
        expected_path: &Path,
        actual_path: &Path,
        source_is_expected: impl FnOnce(&super::Error) -> bool,
    ) {
        let super::Error::Image {
            identity,
            theme,
            expected,
            actual,
            source,
        } = error
        else {
            panic!("expected contextual image error, got {error}");
        };
        assert!(identity.contains("State::view#default"), "{identity}");
        assert_eq!(theme, "native");
        assert_eq!(expected, expected_path);
        assert_eq!(actual, actual_path);
        assert!(source_is_expected(&source), "unexpected source: {source}");
    }

    fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    visit(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }

        let mut files = BTreeMap::new();
        if root.exists() {
            visit(root, root, &mut files);
        }
        files
    }

    struct Project {
        root: PathBuf,
    }

    impl Project {
        fn new(source: &str) -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("rawr-snapshot-{}-{id}", std::process::id()));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"snapshot-fixture\"\n",
            )
            .unwrap();
            crate::write_marked(&root, "src/main.rs", source);
            Self { root }
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            if let Ok(metadata) = fs::symlink_metadata(self.root.join("snapshots"))
                && metadata.file_type().is_symlink()
            {
                fs::remove_file(self.root.join("snapshots")).unwrap();
            }
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    fn source() -> &'static str {
        r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;

#[derive(Default)]
struct State { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    fn dark() -> Theme { Theme::Dark }

    #[preview(name = "Default", group = "snapshot", themes = [State::dark], size = (96, 64))]
    #[preview(name = "Ignored", group = "snapshot", snapshot = false, size = (24, 24))]
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }
}
