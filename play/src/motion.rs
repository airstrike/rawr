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
const CONFIRM: usize = 8;

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
}

/// `time` in seconds, to a tenth, without a trailing `.0`.
#[must_use]
pub fn seconds(time: Duration) -> String {
    let tenths = (time.as_millis() + 50) / 100;
    if tenths % 10 == 0 {
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
/// once its view changed, or the time it ran when it was cut.
pub fn picture(recording: Recording, loops: &Loops, budget: Duration) -> Result<Image, PlayError> {
    let Recording {
        frames,
        background,
        ending,
    } = recording;
    let last = frames.last().ok_or_else(|| PlayError::Limit(crate::TIME_LIMIT.into()))?;
    let still = |note: Option<&str>| {
        Ok(Image {
            picture: Picture::Still(png(last)?),
            note: note.map(str::to_owned),
        })
    };
    if ending == Ending::Messages {
        return still(Some(TOO_LONG));
    }
    let hashes: Vec<u64> = frames.iter().map(hash).collect();
    if hashes.iter().all(|hash| *hash == hashes[0]) {
        return still(None);
    }
    let final_phase = last.phase;
    let first = frames.iter().position(|frame| frame.phase == final_phase).unwrap_or(0);
    let (shown, looping, note) = match (ending, loops.found) {
        (Ending::Stopped, Some((j, k))) => {
            let shown: Vec<(&Frame, Duration)> = (first + j..first + k)
                .map(|at| (&frames[at], frames[at + 1].time - frames[at].time))
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
                    (frame, delay)
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
    let gif = gif(&shown, background, looping)?;
    if gif.bytes.len() > MAX_GIF {
        return still(Some(TOO_LARGE));
    }
    Ok(Image {
        picture: Picture::Animated(gif),
        note,
    })
}

/// A frame as a PNG.
fn png(frame: &Frame) -> Result<Png, PlayError> {
    let failed = |error: &dyn std::fmt::Display| PlayError::Render(format!("cannot encode: {error}"));
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| failed(&error))?;
    writer.write_image_data(&frame.rgba).map_err(|error| failed(&error))?;
    writer.finish().map_err(|error| failed(&error))?;
    Ok(Png {
        width: frame.width,
        height: frame.height,
        bytes,
    })
}

/// Encode `frames`, each shown for its delay, on a canvas the size of the
/// largest, padded with `background`. Equal neighbors merge, and each frame
/// after the first stores only the rectangle that changed.
fn gif(frames: &[(&Frame, Duration)], background: [u8; 4], looping: bool) -> Result<Gif, PlayError> {
    let failed = |error: &dyn std::fmt::Display| PlayError::Render(format!("cannot encode: {error}"));
    let width = frames.iter().map(|(frame, _)| frame.width).max().unwrap_or(1);
    let height = frames.iter().map(|(frame, _)| frame.height).max().unwrap_or(1);
    let side = |side: u32| u16::try_from(side).map_err(|error| failed(&error));
    let (columns, rows) = (width as usize, height as usize);
    // Each frame on the full canvas, equal neighbors merged.
    let mut canvases: Vec<(Vec<u8>, Duration)> = Vec::new();
    for (frame, delay) in frames {
        let mut canvas = background.repeat(columns * rows);
        let stride = frame.width as usize * 4;
        for (row, line) in frame.rgba.chunks_exact(stride).enumerate() {
            let at = row * columns * 4;
            canvas[at..at + stride].copy_from_slice(line);
        }
        match canvases.last_mut() {
            Some((previous, shown)) if *previous == canvas => *shown += *delay,
            _ => canvases.push((canvas, *delay)),
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder =
            gif::Encoder::new(&mut bytes, side(width)?, side(height)?, &[]).map_err(|error| failed(&error))?;
        if looping {
            encoder
                .set_repeat(gif::Repeat::Infinite)
                .map_err(|error| failed(&error))?;
        }
        let mut previous: Option<&Vec<u8>> = None;
        for (canvas, delay) in &canvases {
            let (left, top, right, bottom) = match previous {
                Some(previous) => changed(previous, canvas, columns, rows),
                None => (0, 0, columns, rows),
            };
            let mut pixels = Vec::with_capacity((right - left) * (bottom - top) * 4);
            for row in top..bottom {
                let at = (row * columns + left) * 4;
                pixels.extend_from_slice(&canvas[at..at + (right - left) * 4]);
            }
            let mut frame = gif::Frame::from_rgba_speed(
                side((right - left) as u32)?,
                side((bottom - top) as u32)?,
                &mut pixels,
                10,
            );
            frame.left = side(left as u32)?;
            frame.top = side(top as u32)?;
            frame.delay = centiseconds(*delay);
            frame.dispose = gif::DisposalMethod::Keep;
            encoder.write_frame(&frame).map_err(|error| failed(&error))?;
            previous = Some(canvas);
        }
    }
    Ok(Gif {
        width,
        height,
        bytes,
        frames: canvases.len(),
        looping,
    })
}

/// A delay in GIF's hundredths of a second, at least 2: viewers slow a
/// shorter one down.
fn centiseconds(delay: Duration) -> u16 {
    let hundredths = (delay.as_millis() + 5) / 10;
    u16::try_from(hundredths).unwrap_or(u16::MAX).max(2)
}

/// The rectangle `(left, top, right, bottom)` where `next` differs from
/// `previous`; one pixel when nothing does.
fn changed(previous: &[u8], next: &[u8], columns: usize, rows: usize) -> (usize, usize, usize, usize) {
    let (mut left, mut top, mut right, mut bottom) = (columns, rows, 0, 0);
    for row in 0..rows {
        let line = row * columns * 4..(row + 1) * columns * 4;
        let (a, b) = (&previous[line.clone()], &next[line]);
        if a == b {
            continue;
        }
        top = top.min(row);
        bottom = row + 1;
        let first = a.chunks_exact(4).zip(b.chunks_exact(4)).position(|(a, b)| a != b).unwrap_or(0);
        let last = a.chunks_exact(4).zip(b.chunks_exact(4)).rposition(|(a, b)| a != b).unwrap_or(0);
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
    use super::{cut_note, cycle, stopped_note};
    use std::time::Duration;

    fn times(count: usize) -> Vec<Duration> {
        (0..count).map(|at| Duration::from_millis(50 * at as u64)).collect()
    }

    #[test]
    fn a_still_stretch_is_no_loop() {
        // A stopwatch: each value shows for a while and never comes back.
        let hashes: Vec<u64> = (0..60).map(|at| at / 20).collect();
        assert_eq!(cycle(&hashes, &times(60)), None);
    }

    #[test]
    fn a_repeat_after_an_intro_loops_from_the_repeat() {
        let hashes: Vec<u64> = [9, 8].into_iter().chain((0..40).map(|at| at % 3)).collect();
        assert_eq!(cycle(&hashes, &times(hashes.len())), Some((2, 5)));
    }

    #[test]
    fn notes_round_to_tenths() {
        assert_eq!(cut_note(Duration::from_secs(5)), "Animation shown for the first 5 s.");
        assert_eq!(cut_note(Duration::from_millis(1250)), "Animation shown for the first 1.3 s.");
    }

    #[test]
    fn a_clock_cut_says_rendering_stopped() {
        assert_eq!(
            stopped_note(Duration::from_secs(15), Duration::from_millis(2300)),
            "Rendering stopped after 15 s, so the animation shows its first 2.3 s."
        );
    }
}
