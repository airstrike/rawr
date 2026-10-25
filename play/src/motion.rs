//! Animated renders: a full app recorded on a simulated clock, and answered
//! as a GIF when its view changes.
//!
//! - A view that never changes stays a PNG.
//! - A view that changes and then settles plays once. Its last frame is held
//!   for [`HOLD`], and the GIF has no loop extension.
//! - A view that never settles loops when its frames repeat: frame `k`
//!   equals an earlier frame `j`, and the frames after them match too. The
//!   GIF is frames `j..k` and loops with no hold.
//! - A view that never settles and never repeats within [`SPAN`] plays once
//!   with a note.
//! - A GIF over [`MAX_GIF`], or messages that run past [`FRAMES`], give a PNG
//!   of the last frame with a note.
//!
//! Once the view changes, frames are drawn at [`SCALE`] and the [`Encoder`]
//! encodes each as it is recorded, so the GIF's encoding shares the
//! render's deadline. A still image keeps the canvas's scale.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use rawr_iced::snapshot::{Ending, Frame, Recording};

use crate::{PlayError, Png};

/// Simulated time between frames.
pub const STEP: Duration = Duration::from_millis(50);
/// Most frames recorded.
pub const FRAMES: usize = 100;
/// Longest simulated time recorded after the last message.
pub const SPAN: Duration = Duration::from_secs(5);
/// Most frames after the boot or a message before the next message is
/// applied, when they never settle.
pub const PHASE: usize = 20;
/// How long a play-once GIF shows its last frame.
pub const HOLD: Duration = Duration::from_secs(3);
/// How long a settled frame shows before the next message's frames.
pub const PAUSE: Duration = Duration::from_secs(1);
/// Largest GIF answered: Discord's upload limit.
pub const MAX_GIF: usize = 8 * 1024 * 1024;
/// Most frames after a repeat that must match before it counts as a loop.
/// The clock is simulated and the State drives the view, so two matching
/// frames, with matching gaps, are enough, and a 40-frame loop is found
/// after 43 frames.
const CONFIRM: usize = 2;

/// The note on a GIF over [`MAX_GIF`].
pub const TOO_LARGE: &str = "The animation is over 8 MB. This is its last frame.";
/// The note when messages run past [`FRAMES`].
pub const TOO_LONG: &str = "The animation is over 100 frames. This is its last frame.";

/// An encoded GIF.
#[derive(Clone, Debug)]
pub struct Gif {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The GIF's bytes.
    pub bytes: Vec<u8>,
    /// Frames in the GIF, after equal neighbors are merged.
    pub frames: usize,
    /// Whether it loops forever. One that does not plays once.
    pub looping: bool,
}

/// A rendered snippet: a still image or an animation.
#[derive(Clone, Debug)]
pub enum Picture {
    /// A PNG.
    Still(Png),
    /// A GIF.
    Animated(Gif),
}

/// What an iced snippet rendered, with a note to show beside it.
#[derive(Clone, Debug)]
pub struct Image {
    /// The image.
    pub picture: Picture,
    /// A note for the reader, such as why an animation was cut.
    pub note: Option<String>,
    /// A full app's targets in its last frame, by label in reading order:
    /// what [`crate::Options::steps`] index.
    pub actions: Vec<String>,
    /// The label of the target each step clicked, in order.
    pub clicked: Vec<String>,
}

/// `time` in seconds, to a tenth, without a trailing `.0`.
#[must_use]
pub fn seconds(time: Duration) -> String {
    let tenths = (time.as_millis() + 50) / 100;
    if tenths.is_multiple_of(10) {
        format!("{}", tenths / 10)
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// The note on an animation cut by [`FRAMES`] or [`SPAN`] after `shown` of
/// simulated time.
#[must_use]
pub fn cut_note(shown: Duration) -> String {
    format!("Animation shown for the first {} s.", seconds(shown))
}

/// How a note on an animation the wall clock cut starts.
pub const STOPPED: &str = "Rendering stopped after ";

/// The note on an animation the wall clock cut after `budget`, when it had
/// shown `shown` of simulated time.
#[must_use]
pub fn stopped_note(budget: Duration, shown: Duration) -> String {
    format!(
        "{STOPPED}{} s, so the animation shows its first {} s.",
        seconds(budget),
        seconds(shown)
    )
}

/// The sentence added to [`stopped_note`] when the bot cut the render
/// because someone else was waiting.
pub const WAITING: &str = "It was cut short because someone else was waiting.";

/// Where a recording's frames repeat, as `stop` sees them: the first `k`
/// whose frame equals an earlier frame `j` that is not part of a still
/// stretch, with the next frames and their timing matching too.
#[derive(Default)]
pub struct Loops {
    hashes: Vec<u64>,
    found: Option<(usize, usize)>,
}

impl Loops {
    /// Whether `frames` (the recording since its last message) repeat.
    pub fn check(&mut self, frames: &[Frame]) -> bool {
        for frame in &frames[self.hashes.len()..] {
            self.hashes.push(hash(frame));
        }
        let times: Vec<Duration> = frames.iter().map(|frame| frame.time).collect();
        self.found = cycle(&self.hashes, &times);
        self.found.is_some()
    }
}

fn hash(frame: &Frame) -> u64 {
    let mut hasher = DefaultHasher::new();
    (frame.width, frame.height).hash(&mut hasher);
    frame.rgba.hash(&mut hasher);
    hasher.finish()
}

/// The first repeat `(j, k)` in `hashes` taken at `times`: frame `k` equals
/// frame `j`, frames `j..k` are not all the same, and the frames after `k`
/// and the time between them match those after `j`.
fn cycle(hashes: &[u64], times: &[Duration]) -> Option<(usize, usize)> {
    let gap = |at: usize| times[at + 1] - times[at];
    for k in 1..hashes.len() {
        for j in 0..k {
            if hashes[j] != hashes[k] || hashes[j..k].iter().all(|hash| *hash == hashes[j]) {
                continue;
            }
            let confirm = (k - j).min(CONFIRM);
            if k + confirm >= hashes.len() {
                continue;
            }
            if (0..confirm).all(|i| hashes[j + i] == hashes[k + i] && gap(j + i) == gap(k + i)) {
                return Some((j, k));
            }
        }
    }
    None
}

/// The image a recording gives. `budget` is the wall-clock time it had
/// once its view changed, or the time it ran when it was cut. `encoder`
/// has seen every frame.
pub fn picture(
    recording: Recording,
    loops: &Loops,
    budget: Duration,
    encoder: Encoder,
) -> Result<Image, PlayError> {
    let Recording {
        frames,
        ending,
        targets,
        clicked,
    } = recording;
    let actions: Vec<String> = targets.into_iter().map(|target| target.label).collect();
    let last = frames
        .last()
        .ok_or_else(|| PlayError::Limit(crate::TIME_LIMIT.into()))?;
    let still = |note: Option<&str>| {
        Ok(Image {
            picture: Picture::Still(png(last)?),
            note: note.map(str::to_owned),
            actions: actions.clone(),
            clicked: clicked.clone(),
        })
    };
    if ending == Ending::Messages {
        return still(Some(TOO_LONG));
    }
    if frames.iter().all(|frame| frame.same_image(&frames[0])) {
        return still(None);
    }
    let final_phase = last.phase;
    let first = frames
        .iter()
        .position(|frame| frame.phase == final_phase)
        .unwrap_or(0);
    let (shown, looping, note) = match (ending, loops.found) {
        (Ending::Stopped, Some((j, k))) => {
            // Start the loop where its picture changes, so a frame held
            // across the seam is not split. Frame `k` shows frame `j`'s
            // picture as a change from the frame before the seam.
            let (j, k) = (first + j, first + k);
            let count = k - j;
            let before = |at: usize| if at == j { k - 1 } else { at - 1 };
            let start = (j..k)
                .find(|&at| !frames[at].same_image(&frames[before(at)]))
                .unwrap_or(j);
            let shown: Vec<(usize, Duration)> = (0..count)
                .map(|step| {
                    let at = j + (start - j + step) % count;
                    let delay = frames[at + 1].time - frames[at].time;
                    (if at == j && step > 0 { k } else { at }, delay)
                })
                .collect();
            (shown, true, None)
        }
        _ => {
            let shown = frames
                .iter()
                .enumerate()
                .map(|(at, frame)| {
                    let delay = match frames.get(at + 1) {
                        None => HOLD,
                        Some(next) if next.phase != frame.phase => PAUSE,
                        Some(next) => next.time - frame.time,
                    };
                    (at, delay)
                })
                .collect();
            let played = last.time + STEP - frames[first].time;
            let note = match ending {
                Ending::Settled => None,
                Ending::Deadline => Some(stopped_note(budget, played)),
                Ending::Cut => Some(format!("{} {WAITING}", stopped_note(budget, played))),
                _ => Some(cut_note(played)),
            };
            (shown, false, note)
        }
    };
    let gif = encoder.finish(&frames, &shown, looping)?;
    if gif.bytes.len() > MAX_GIF {
        return still(Some(TOO_LARGE));
    }
    Ok(Image {
        picture: Picture::Animated(gif),
        note,
        actions,
        clicked,
    })
}

/// A frame as a PNG.
pub(crate) fn png(frame: &Frame) -> Result<Png, PlayError> {
    let failed =
        |error: &dyn std::fmt::Display| PlayError::Render(format!("cannot encode: {error}"));
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| failed(&error))?;
    writer
        .write_image_data(&frame.rgba)
        .map_err(|error| failed(&error))?;
    writer.finish().map_err(|error| failed(&error))?;
    Ok(Png {
        width: frame.width,
        height: frame.height,
        bytes,
    })
}

/// The scale an animation's frames are drawn and encoded at. Discord
/// shows a GIF small, so it needs no more than one pixel per logical pixel.
pub const SCALE: f32 = 1.0;
/// NeuQuant's sampling factor for a frame with more than 255 colors: 1 is
/// slowest, 30 fastest.
const SPEED: i32 = 20;

/// An animation's GIF frames, encoded as they are recorded, so encoding
/// shares the recording's deadline. Nothing is encoded until the view
/// changes, so a still image costs nothing here. Each frame stores only
/// the rectangle that changed, with its unchanged pixels transparent.
pub struct Encoder {
    /// The scale frames are encoded at.
    scale: f32,
    /// Frames encoded so far.
    seen: usize,
    /// The canvas in pixels, once the view changed.
    size: Option<(usize, usize)>,
    /// The last frame on the canvas.
    previous: Vec<u8>,
    /// Each frame from the first: its change from the one before, or
    /// `None` when it shows the same.
    encoded: Vec<Option<gif::Frame<'static>>>,
    error: Option<PlayError>,
}

impl Encoder {
    /// An encoder for frames at `scale`.
    #[must_use]
    pub fn new(scale: f32) -> Self {
        Self {
            scale,
            seen: 0,
            size: None,
            previous: Vec::new(),
            encoded: Vec::new(),
            error: None,
        }
    }

    /// Encode the frames of `frames` not yet seen, each on a canvas of its
    /// own background.
    pub fn push(&mut self, frames: &[Frame]) {
        if self.error.is_some() {
            return;
        }
        if self.size.is_none() && frames.last().is_none_or(|last| last.same_image(&frames[0])) {
            return;
        }
        // A frame larger than the canvas grows it, and starts over.
        let size = frames[self.seen..]
            .iter()
            .map(|frame| self.size_of(frame))
            .fold(self.size.unwrap_or((0, 0)), |(w, h), (fw, fh)| {
                (w.max(fw), h.max(fh))
            });
        if self.size != Some(size) {
            self.size = Some(size);
            self.seen = 0;
            self.encoded.clear();
            self.previous.clear();
        }
        for frame in &frames[self.seen..] {
            let canvas = self.canvas(frame);
            let previous = (!self.previous.is_empty()).then_some(self.previous.as_slice());
            match encode(&canvas, previous, size) {
                Ok(encoded) => self.encoded.push(encoded),
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
            self.previous = canvas;
        }
        self.seen = frames.len();
    }

    /// `frame`'s size in pixels at the encoder's scale.
    fn size_of(&self, frame: &Frame) -> (usize, usize) {
        let factor = (frame.scale / self.scale).max(1.0);
        let side = |side: u32| ((side as f32 / factor).round() as usize).max(1);
        (side(frame.width), side(frame.height))
    }

    /// `frame` at the encoder's scale, on the canvas, padded with its
    /// background.
    fn canvas(&self, frame: &Frame) -> Vec<u8> {
        let (columns, rows) = self.size.unwrap_or((1, 1));
        let (width, height) = self.size_of(frame);
        let pixels = resample(frame, width, height);
        let mut canvas = frame.background.repeat(columns * rows);
        let stride = width * 4;
        for (row, line) in pixels.chunks_exact(stride).enumerate() {
            let at = row * columns * 4;
            canvas[at..at + stride].copy_from_slice(line);
        }
        canvas
    }

    /// The GIF of `shown`: indices into `frames` with each one's delay.
    fn finish(
        mut self,
        frames: &[Frame],
        shown: &[(usize, Duration)],
        looping: bool,
    ) -> Result<Gif, PlayError> {
        let failed =
            |error: &dyn std::fmt::Display| PlayError::Render(format!("cannot encode: {error}"));
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        let Some((columns, rows)) = self.size else {
            return Err(PlayError::Render(
                "cannot encode: the view never changed".into(),
            ));
        };
        let mut out: Vec<gif::Frame<'static>> = Vec::new();
        for &(at, delay) in shown {
            let frame = if out.is_empty() {
                // The first frame is whole; a loop's first was stored as
                // a change from the frame before it.
                match self
                    .encoded
                    .get_mut(at)
                    .and_then(Option::take)
                    .filter(|_| at == 0)
                {
                    Some(frame) => Some(frame),
                    None => encode(&self.canvas(&frames[at]), None, (columns, rows))?,
                }
            } else {
                self.encoded.get_mut(at).and_then(Option::take)
            };
            match (frame, out.last_mut()) {
                (Some(mut frame), _) => {
                    frame.delay = centiseconds(delay);
                    out.push(frame);
                }
                // The same picture: the frame before shows longer.
                (None, Some(previous)) => {
                    let shown = Duration::from_millis(u64::from(previous.delay) * 10) + delay;
                    previous.delay = centiseconds(shown);
                }
                (None, None) => {}
            }
        }
        let side = |side: usize| u16::try_from(side).map_err(|error| failed(&error));
        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut bytes, side(columns)?, side(rows)?, &[])
                .map_err(|error| failed(&error))?;
            if looping {
                encoder
                    .set_repeat(gif::Repeat::Infinite)
                    .map_err(|error| failed(&error))?;
            }
            for frame in &out {
                encoder
                    .write_lzw_pre_encoded_frame(frame)
                    .map_err(|error| failed(&error))?;
            }
        }
        Ok(Gif {
            width: columns as u32,
            height: rows as u32,
            bytes,
            frames: out.len(),
            looping,
        })
    }
}

/// `frame`'s pixels at `width` by `height`: each pixel the average of the
/// pixels it covers when the factor is whole, else the nearest one.
fn resample(frame: &Frame, width: usize, height: usize) -> Vec<u8> {
    let (from_width, from_height) = (frame.width as usize, frame.height as usize);
    if (width, height) == (from_width, from_height) {
        return frame.rgba.clone();
    }
    let factor = from_width / width.max(1);
    let whole = factor > 1 && from_width == width * factor && from_height == height * factor;
    let mut pixels = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        for column in 0..width {
            if whole {
                let mut sum = [0u32; 4];
                for y in row * factor..(row + 1) * factor {
                    let at = (y * from_width + column * factor) * 4;
                    for pixel in frame.rgba[at..at + factor * 4].as_chunks::<4>().0 {
                        for (sum, value) in sum.iter_mut().zip(pixel) {
                            *sum += u32::from(*value);
                        }
                    }
                }
                let count = (factor * factor) as u32;
                pixels.extend(sum.map(|sum| ((sum + count / 2) / count) as u8));
            } else {
                let y = (row * from_height / height).min(from_height - 1);
                let x = (column * from_width / width).min(from_width - 1);
                let at = (y * from_width + x) * 4;
                pixels.extend_from_slice(&frame.rgba[at..at + 4]);
            }
        }
    }
    pixels
}

/// `canvas` (`size` in pixels) as a GIF frame: whole, or the rectangle
/// where it differs from `previous` with its unchanged pixels transparent.
/// `None` when it equals `previous`.
fn encode(
    canvas: &[u8],
    previous: Option<&[u8]>,
    (columns, rows): (usize, usize),
) -> Result<Option<gif::Frame<'static>>, PlayError> {
    let failed =
        |error: &dyn std::fmt::Display| PlayError::Render(format!("cannot encode: {error}"));
    let side = |side: usize| u16::try_from(side).map_err(|error| failed(&error));
    if previous == Some(canvas) {
        return Ok(None);
    }
    let (left, top, right, bottom) = match previous {
        Some(previous) => changed(previous, canvas, columns, rows),
        None => (0, 0, columns, rows),
    };
    // Each pixel of the rectangle: its color, or `None` when unchanged.
    let mut pixels: Vec<Option<[u8; 3]>> = Vec::with_capacity((right - left) * (bottom - top));
    for row in top..bottom {
        for column in left..right {
            let at = (row * columns + column) * 4;
            let pixel = &canvas[at..at + 4];
            let same = previous.is_some_and(|previous| &previous[at..at + 4] == pixel);
            pixels.push((!same).then(|| [pixel[0], pixel[1], pixel[2]]));
        }
    }
    // An exact palette when the colors fit, else NeuQuant's 255; the last
    // index is transparent.
    let mut exact: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
    let fits = pixels.iter().flatten().all(|color| {
        let next = exact.len();
        if next < 255 {
            exact.entry(*color).or_insert(next as u8);
            true
        } else {
            exact.contains_key(color)
        }
    });
    let (mut palette, indices): (Vec<u8>, Vec<u8>) = if fits {
        let mut palette = vec![0; exact.len() * 3];
        for (color, index) in &exact {
            palette[*index as usize * 3..][..3].copy_from_slice(color);
        }
        let indices = pixels
            .iter()
            .map(|pixel| pixel.map_or(255, |color| exact[&color]))
            .collect();
        (palette, indices)
    } else {
        let rgba: Vec<u8> = pixels
            .iter()
            .flatten()
            .flat_map(|[r, g, b]| [*r, *g, *b, 255])
            .collect();
        let quant = color_quant::NeuQuant::new(SPEED, 255, &rgba);
        let mut cache: std::collections::HashMap<[u8; 3], u8> = std::collections::HashMap::new();
        let indices = pixels
            .iter()
            .map(|pixel| match pixel {
                None => 255,
                Some(color) => *cache
                    .entry(*color)
                    .or_insert_with(|| quant.index_of(&[color[0], color[1], color[2], 255]) as u8),
            })
            .collect();
        (quant.color_map_rgb(), indices)
    };
    palette.resize(256 * 3, 0);
    let transparent = pixels.iter().any(Option::is_none).then_some(255);
    let mut frame = gif::Frame::from_palette_pixels(
        side(right - left)?,
        side(bottom - top)?,
        indices,
        palette,
        transparent,
    );
    frame.left = side(left)?;
    frame.top = side(top)?;
    frame.dispose = gif::DisposalMethod::Keep;
    frame.make_lzw_pre_encoded();
    Ok(Some(frame))
}

/// A delay in GIF's hundredths of a second, at least 2: viewers slow a
/// shorter one down.
fn centiseconds(delay: Duration) -> u16 {
    let hundredths = (delay.as_millis() + 5) / 10;
    u16::try_from(hundredths).unwrap_or(u16::MAX).max(2)
}

/// The rectangle `(left, top, right, bottom)` where `next` differs from
/// `previous`; one pixel when nothing does.
fn changed(
    previous: &[u8],
    next: &[u8],
    columns: usize,
    rows: usize,
) -> (usize, usize, usize, usize) {
    let (mut left, mut top, mut right, mut bottom) = (columns, rows, 0, 0);
    for row in 0..rows {
        let line = row * columns * 4..(row + 1) * columns * 4;
        let (a, b) = (&previous[line.clone()], &next[line]);
        if a == b {
            continue;
        }
        top = top.min(row);
        bottom = row + 1;
        let first = a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let last = a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .rposition(|(a, b)| a != b)
            .unwrap_or(0);
        left = left.min(first);
        right = right.max(last + 1);
    }
    if bottom == 0 {
        return (0, 0, 1, 1);
    }
    (left, top, right, bottom)
}

#[cfg(test)]
mod tests {
    use super::{cut_note, cycle, resample, stopped_note, Encoder};
    use rawr_iced::snapshot::{Ending, Frame, Recording};
    use std::time::Duration;

    const BACKGROUND: [u8; 4] = [10, 20, 30, 255];

    /// A `side` by `side` frame at `scale` of one color with a lit pixel
    /// at `lit`, `at` steps in.
    fn frame(side: u32, scale: f32, lit: Option<(u32, u32)>, at: u64) -> Frame {
        let mut rgba = BACKGROUND.repeat((side * side) as usize);
        if let Some((x, y)) = lit {
            let at = ((y * side + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        Frame {
            width: side,
            height: side,
            rgba,
            time: Duration::from_millis(50 * at),
            phase: 0,
            scale,
            background: BACKGROUND,
        }
    }

    fn times(count: usize) -> Vec<Duration> {
        (0..count)
            .map(|at| Duration::from_millis(50 * at as u64))
            .collect()
    }

    #[test]
    fn a_still_stretch_is_no_loop() {
        // A stopwatch: each value shows for a while and never comes back.
        let hashes: Vec<u64> = (0..60).map(|at| at / 20).collect();
        assert_eq!(cycle(&hashes, &times(60)), None);
    }

    #[test]
    fn a_long_loop_is_found_two_frames_after_it_repeats() {
        let hashes: Vec<u64> = (0..43).map(|at| at % 40).collect();
        assert_eq!(cycle(&hashes, &times(hashes.len())), Some((0, 40)));
        assert_eq!(cycle(&hashes[..42], &times(42)), None);
    }

    #[test]
    fn a_repeat_after_an_intro_loops_from_the_repeat() {
        let hashes: Vec<u64> = [9, 8].into_iter().chain((0..40).map(|at| at % 3)).collect();
        assert_eq!(cycle(&hashes, &times(hashes.len())), Some((2, 5)));
    }

    #[test]
    fn notes_round_to_tenths() {
        assert_eq!(
            cut_note(Duration::from_secs(5)),
            "Animation shown for the first 5 s."
        );
        assert_eq!(
            cut_note(Duration::from_millis(1250)),
            "Animation shown for the first 1.3 s."
        );
    }

    #[test]
    fn a_clock_cut_says_rendering_stopped() {
        assert_eq!(
            stopped_note(Duration::from_secs(15), Duration::from_millis(2300)),
            "Rendering stopped after 15 s, so the animation shows its first 2.3 s."
        );
    }

    #[test]
    fn a_still_view_encodes_nothing() {
        let mut encoder = Encoder::new(1.0);
        let frames: Vec<Frame> = (0..10).map(|at| frame(8, 2.0, None, at)).collect();
        for count in 1..=frames.len() {
            encoder.push(&frames[..count]);
        }
        assert!(encoder.size.is_none() && encoder.encoded.is_empty());
    }

    #[test]
    fn a_double_scale_frame_is_averaged_down() {
        // A 2x2 block of one lit pixel and three background ones.
        let big = frame(4, 2.0, Some((0, 0)), 0);
        let small = resample(&big, 2, 2);
        assert_eq!(&small[..4], &[71, 15, 23, 255]);
        assert_eq!(&small[4..8], &BACKGROUND);
    }

    #[test]
    fn frames_store_their_change_with_the_rest_transparent() {
        // A 2x frame, the change at 1x, the same again, then a move.
        let frames = vec![
            frame(16, 2.0, None, 0),
            frame(8, 1.0, Some((2, 3)), 1),
            frame(8, 1.0, Some((2, 3)), 2),
            frame(8, 1.0, Some((5, 6)), 3),
        ];
        let mut encoder = Encoder::new(1.0);
        for count in 1..=frames.len() {
            encoder.push(&frames[..count]);
        }
        assert_eq!(encoder.size, Some((8, 8)));
        let first = encoder.encoded[0].as_ref().unwrap();
        assert_eq!((first.width, first.height, first.transparent), (8, 8, None));
        let lit = encoder.encoded[1].as_ref().unwrap();
        assert_eq!((lit.left, lit.top, lit.width, lit.height), (2, 3, 1, 1));
        assert!(
            encoder.encoded[2].is_none(),
            "the same picture stores nothing"
        );
        // The move changes two pixels; the ones between keep the last frame.
        let moved = encoder.encoded[3].as_ref().unwrap();
        assert_eq!(
            (moved.left, moved.top, moved.width, moved.height),
            (2, 3, 4, 4)
        );
        assert_eq!(moved.transparent, Some(255));
        let recording = Recording {
            frames,
            ending: Ending::Settled,
            targets: Vec::new(),
            clicked: Vec::new(),
        };
        let image =
            super::picture(recording, &super::Loops::default(), Duration::ZERO, encoder).unwrap();
        let super::Picture::Animated(gif) = image.picture else {
            panic!("an animation gives a GIF");
        };
        assert_eq!(
            (gif.width, gif.height, gif.frames, gif.looping),
            (8, 8, 3, false)
        );
        // It decodes: the last frame shows the moved pixel only.
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(gif.bytes.as_slice()).unwrap();
        let mut screen = vec![0u8; 8 * 8 * 4];
        let mut delays = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            delays.push(frame.delay);
            for row in 0..frame.height as usize {
                for column in 0..frame.width as usize {
                    let from = (row * frame.width as usize + column) * 4;
                    if frame.buffer[from + 3] == 0 {
                        continue;
                    }
                    let to = ((row + frame.top as usize) * 8 + column + frame.left as usize) * 4;
                    screen[to..to + 4].copy_from_slice(&frame.buffer[from..from + 4]);
                }
            }
        }
        // The repeated frame shows twice as long, and the last is held.
        assert_eq!(delays, vec![5, 10, 300]);
        let pixel = |x: usize, y: usize| &screen[(y * 8 + x) * 4..][..4];
        assert_eq!(pixel(5, 6), &[255, 0, 0, 255]);
        assert_eq!(pixel(2, 3), &BACKGROUND);
        assert_eq!(pixel(0, 0), &BACKGROUND);
    }
}
