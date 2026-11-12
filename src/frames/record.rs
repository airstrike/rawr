//! The frames one run of a program left, as its file under the project's
//! `target/rawr` holds them: for each window, the frame it first showed with
//! and the frame it was last in.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::str::FromStr;

/// The record's first line, which a change to the format changes.
const HEADER: &str = "rawr window frames 1";

/// A window's frame on screen, in the platform's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Frame {
    pub(super) const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(super) const fn x(self) -> f64 {
        self.x
    }

    pub(super) const fn y(self) -> f64 {
        self.y
    }

    pub(super) const fn width(self) -> f64 {
        self.width
    }

    pub(super) const fn height(self) -> f64 {
        self.height
    }

    /// Whether all of `self` lies on `screen`. A window manager that hides
    /// a window moves it almost wholly off the screen, as AeroSpace does to
    /// another workspace's windows, and a screen unplugged since leaves a
    /// frame on none.
    pub(super) fn lies_on(self, screen: Self) -> bool {
        self.x >= screen.x
            && self.y >= screen.y
            && self.x + self.width <= screen.x + screen.width
            && self.y + self.height <= screen.y + screen.height
    }
}

impl fmt::Display for Frame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} {} {}",
            self.x, self.y, self.width, self.height
        )
    }
}

/// How a window is told from the program's other windows across runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Key {
    /// The stable identifier the program gave the window.
    Named(String),
    /// The window's place among the unnamed windows its run showed, and how
    /// many of the run's windows were open when it showed: a run that shows
    /// its windows in another order or number matches no record.
    Order { index: usize, open: usize },
}

impl Key {
    /// Whether `self` and `other` stand for the same window, so a run's
    /// placement of it replaces the one an earlier run left.
    fn replaces(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Named(name), Self::Named(other)) => name == other,
            (Self::Order { index, .. }, Self::Order { index: other, .. }) => index == other,
            _ => false,
        }
    }
}

/// One window's placement in a run: the frame the program first showed it
/// with, and the frame it was last in.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Placement {
    key: Key,
    shown: Frame,
    last: Frame,
}

impl Placement {
    pub(super) const fn new(key: Key, shown: Frame) -> Self {
        Self {
            key,
            shown,
            last: shown,
        }
    }

    pub(super) const fn key(&self) -> &Key {
        &self.key
    }

    pub(super) fn set_last(&mut self, last: Frame) {
        self.last = last;
    }
}

/// The placements a run left, which the next run starts from.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Record {
    placements: Vec<Placement>,
}

impl Record {
    /// The frame to give a window `key` names that first showed with
    /// `shown`: the frame the person left it in, when the program asked for
    /// the same frame in the run that left it and the person moved or
    /// resized it since. A window the program now asks to show elsewhere or
    /// at another size shows as the program asks.
    pub(super) fn restored(&self, key: &Key, shown: Frame) -> Option<Frame> {
        self.placements
            .iter()
            .find(|placement| placement.key == *key && placement.shown == shown)
            .map(|placement| placement.last)
            .filter(|last| *last != shown)
    }

    /// The record a run leaves: its own placements, then each of `self`'s
    /// windows the run hasn't shown, so a window that shows later still
    /// finds its frame.
    pub(super) fn updated(&self, run: &[Placement]) -> Self {
        let kept = self.placements.iter().filter(|kept| {
            !run.iter()
                .any(|placement| placement.key.replaces(&kept.key))
        });
        Self {
            placements: run.iter().chain(kept).cloned().collect(),
        }
    }

    /// The record at `path`: an absent or unreadable file leaves none.
    pub(super) fn read(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or_default()
    }

    /// Write the record to `path` whole, so a run that starts while this
    /// one writes reads the record before or the record after.
    pub(super) fn write(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let written = path.with_extension("new");
        fs::write(&written, self.to_string())?;
        fs::rename(&written, path)
    }
}

impl fmt::Display for Record {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{HEADER}")?;
        for Placement { key, shown, last } in &self.placements {
            match key {
                Key::Order { index, open } => {
                    writeln!(formatter, "order {index} {open} {shown} {last}")?;
                }
                Key::Named(name) => writeln!(formatter, "named {shown} {last} {name}")?,
            }
        }
        Ok(())
    }
}

/// A record file that isn't one this format writes.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Unreadable;

impl FromStr for Record {
    type Err = Unreadable;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(Unreadable);
        }
        let placements = lines
            .map(placement)
            .collect::<Option<_>>()
            .ok_or(Unreadable)?;
        Ok(Self { placements })
    }
}

/// One line of a record.
fn placement(line: &str) -> Option<Placement> {
    let (kind, rest) = line.split_once(' ')?;
    let (order, rest) = match kind {
        "order" => {
            let mut words = rest.splitn(3, ' ');
            let index = words.next()?.parse().ok()?;
            let open = words.next()?.parse().ok()?;
            (Some(Key::Order { index, open }), words.next()?)
        }
        "named" => (None, rest),
        _ => return None,
    };
    let mut words = rest.splitn(9, ' ');
    let shown = frame(&mut words)?;
    let last = frame(&mut words)?;
    let key = match order {
        Some(order) => words.next().is_none().then_some(order)?,
        None => Key::Named(words.next().filter(|name| !name.is_empty())?.to_owned()),
    };
    Some(Placement { key, shown, last })
}

/// The frame the next four words of a line hold.
fn frame<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Frame> {
    let mut number = || {
        words
            .next()?
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
    };
    Some(Frame::new(number()?, number()?, number()?, number()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: Frame = Frame::new(100.0, 200.0, 1024.0, 768.0);
    const MOVED: Frame = Frame::new(340.5, 80.0, 1200.0, 700.0);

    fn first() -> Key {
        Key::Order { index: 0, open: 0 }
    }

    fn left(key: Key, shown: Frame, last: Frame) -> Placement {
        let mut placement = Placement::new(key, shown);
        placement.set_last(last);
        placement
    }

    #[test]
    fn a_window_shown_as_before_gets_the_frame_the_person_left_it_in() {
        let record = Record::default().updated(&[left(first(), SHOWN, MOVED)]);
        assert_eq!(record.restored(&first(), SHOWN), Some(MOVED));
    }

    #[test]
    fn a_window_the_program_asks_for_differently_shows_as_asked() {
        let record = Record::default().updated(&[left(first(), SHOWN, MOVED)]);
        let resized = Frame::new(100.0, 200.0, 800.0, 768.0);
        assert_eq!(record.restored(&first(), resized), None);
    }

    #[test]
    fn a_window_left_where_it_showed_needs_nothing() {
        let record = Record::default().updated(&[Placement::new(first(), SHOWN)]);
        assert_eq!(record.restored(&first(), SHOWN), None);
    }

    #[test]
    fn a_window_shown_beside_another_number_of_open_windows_matches_nothing() {
        let record = Record::default().updated(&[left(first(), SHOWN, MOVED)]);
        assert_eq!(
            record.restored(&Key::Order { index: 0, open: 1 }, SHOWN),
            None
        );
        assert_eq!(
            record.restored(&Key::Order { index: 1, open: 0 }, SHOWN),
            None
        );
        assert_eq!(record.restored(&Key::Named("main".to_owned()), SHOWN), None);
    }

    #[test]
    fn a_run_keeps_the_windows_it_has_not_shown_yet() {
        let second = Key::Order { index: 1, open: 1 };
        let earlier = Record::default().updated(&[
            left(first(), SHOWN, SHOWN),
            left(second.clone(), SHOWN, MOVED),
        ]);
        let record = earlier.updated(&[left(first(), SHOWN, MOVED)]);
        assert_eq!(record.restored(&first(), SHOWN), Some(MOVED));
        assert_eq!(record.restored(&second, SHOWN), Some(MOVED));
    }

    #[test]
    fn a_run_replaces_the_placement_of_a_window_it_showed() {
        let earlier = Record::default().updated(&[left(first(), SHOWN, MOVED)]);
        let reopened = Key::Order { index: 0, open: 1 };
        let record = earlier.updated(&[Placement::new(reopened.clone(), SHOWN)]);
        assert_eq!(record.restored(&first(), SHOWN), None);
        assert_eq!(record.placements.len(), 1);
    }

    #[test]
    fn a_record_reads_back_as_it_was_written() {
        let record = Record::default().updated(&[
            left(first(), SHOWN, MOVED),
            left(Key::Named("inspector window".to_owned()), MOVED, SHOWN),
        ]);
        assert_eq!(record.to_string().parse(), Ok(record));
    }

    #[test]
    fn a_file_of_another_format_is_unreadable() {
        assert_eq!("".parse::<Record>(), Err(Unreadable));
        assert_eq!("rawr window frames 0\n".parse::<Record>(), Err(Unreadable));
        let text = format!("{HEADER}\norder 0 0 1 2 3\n");
        assert_eq!(text.parse::<Record>(), Err(Unreadable));
        let text = format!("{HEADER}\norder 0 0 1 2 3 4 5 6 7 NaN\n");
        assert_eq!(text.parse::<Record>(), Err(Unreadable));
    }

    #[test]
    fn only_a_frame_wholly_on_a_screen_lies_on_it() {
        let screen = Frame::new(0.0, 0.0, 1600.0, 900.0);
        assert!(MOVED.lies_on(screen));
        assert!(screen.lies_on(screen));
        assert!(!Frame::new(1400.0, -768.0, 1024.0, 800.0).lies_on(screen));
        assert!(!Frame::new(-2000.0, 100.0, 400.0, 300.0).lies_on(screen));
    }
}
