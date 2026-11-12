//! The windows one run shows, followed from their first show to their
//! close, against the record the run started from.

use super::record::{Frame, Key, Placement, Record};

/// A window as the platform hands it over: an address that stands for the
/// window only while it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Window(usize);

impl Window {
    pub(super) const fn new(address: usize) -> Self {
        Self(address)
    }
}

/// The run in progress: the record it started from and the windows it has
/// shown, in the order they first showed.
#[derive(Debug, Default)]
pub(super) struct Session {
    previous: Record,
    placements: Vec<Placement>,
    /// Each placement's window while it is open.
    windows: Vec<Option<Window>>,
}

impl Session {
    pub(super) fn new(previous: Record) -> Self {
        Self {
            previous,
            ..Self::default()
        }
    }

    /// Whether `window` has shown in this run and is open.
    pub(super) fn knows(&self, window: Window) -> bool {
        self.windows.contains(&Some(window))
    }

    /// `window` shows for the first time, in `shown`, the frame the program
    /// gave it: the frame to give it in its place, if any. `name` is the
    /// stable identifier the program gave it; an unnamed window is known by
    /// its order. A frame that doesn't lie wholly on one of `screens` is
    /// never given.
    pub(super) fn show(
        &mut self,
        window: Window,
        name: Option<&str>,
        shown: Frame,
        screens: &[Frame],
    ) -> Option<Frame> {
        let key = match name.filter(|name| !name.is_empty() && !name.contains('\n')) {
            Some(name) => Key::Named(name.to_owned()),
            None => Key::Order {
                index: self
                    .placements
                    .iter()
                    .filter(|placement| matches!(placement.key(), Key::Order { .. }))
                    .count(),
                open: self.windows.iter().flatten().count(),
            },
        };
        let restored = self
            .previous
            .restored(&key, shown)
            .filter(|last| screens.iter().any(|screen| last.lies_on(*screen)));
        let mut placement = Placement::new(key, shown);
        if let Some(restored) = restored {
            placement.set_last(restored);
        }
        self.placements.push(placement);
        self.windows.push(Some(window));
        restored
    }

    /// `window` is now in `frame`: whether it is one this run follows.
    pub(super) fn moved(&mut self, window: Window, frame: Frame) -> bool {
        let Some(index) = self.index(window) else {
            return false;
        };
        self.placements[index].set_last(frame);
        true
    }

    /// `window` closes; its placement stays in the run's record.
    pub(super) fn closed(&mut self, window: Window) {
        if let Some(index) = self.index(window) {
            self.windows[index] = None;
        }
    }

    /// The record this run leaves for the next.
    pub(super) fn record(&self) -> Record {
        self.previous.updated(&self.placements)
    }

    fn index(&self, window: Window) -> Option<usize> {
        self.windows.iter().position(|open| *open == Some(window))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Frame = Frame::new(0.0, 0.0, 1440.0, 900.0);
    const SHOWN: Frame = Frame::new(208.0, 66.0, 1024.0, 768.0);
    const MOVED: Frame = Frame::new(400.0, 100.0, 1024.0, 768.0);

    /// The record a run leaves that showed one window in `SHOWN` and left
    /// it in `last`.
    fn run(previous: Record, last: Frame) -> Record {
        let mut session = Session::new(previous);
        session.show(Window::new(1), None, SHOWN, &[SCREEN]);
        session.moved(Window::new(1), last);
        session.record()
    }

    #[test]
    fn a_restarted_window_shows_where_the_person_left_it() {
        let record = run(Record::default(), MOVED);
        let mut session = Session::new(record);
        assert_eq!(
            session.show(Window::new(2), None, SHOWN, &[SCREEN]),
            Some(MOVED)
        );
    }

    #[test]
    fn a_window_the_program_now_sizes_differently_shows_as_cargo_shows_it() {
        let record = run(Record::default(), MOVED);
        let mut session = Session::new(record);
        let resized = Frame::new(208.0, 66.0, 800.0, 600.0);
        assert_eq!(session.show(Window::new(2), None, resized, &[SCREEN]), None);
    }

    #[test]
    fn a_restored_window_keeps_the_program_frame_for_the_next_run() {
        let record = run(Record::default(), MOVED);
        // The restored window isn't moved again; the run after still
        // compares against the frame the program asked for.
        let mut session = Session::new(record);
        session.show(Window::new(2), None, SHOWN, &[SCREEN]);
        let mut next = Session::new(session.record());
        assert_eq!(
            next.show(Window::new(3), None, SHOWN, &[SCREEN]),
            Some(MOVED)
        );
    }

    #[test]
    fn a_frame_off_the_screens_is_never_given() {
        // As AeroSpace leaves a window it hides on another workspace.
        let hidden = Frame::new(1400.0, -768.0, 1024.0, 800.0);
        for last in [Frame::new(2000.0, 100.0, 1024.0, 768.0), hidden] {
            let mut session = Session::new(run(Record::default(), last));
            assert_eq!(session.show(Window::new(2), None, SHOWN, &[SCREEN]), None);
        }
    }

    #[test]
    fn unnamed_windows_match_by_order_and_the_count_open() {
        let mut first = Session::new(Record::default());
        first.show(Window::new(1), None, SHOWN, &[SCREEN]);
        first.show(Window::new(2), None, SHOWN, &[SCREEN]);
        first.moved(Window::new(2), MOVED);
        let record = first.record();

        let mut same = Session::new(record.clone());
        assert_eq!(same.show(Window::new(3), None, SHOWN, &[SCREEN]), None);
        assert_eq!(
            same.show(Window::new(4), None, SHOWN, &[SCREEN]),
            Some(MOVED)
        );

        // The first window closed before the second showed: one window is
        // open where two were, so the second shows as the program asks.
        let mut fewer = Session::new(record);
        fewer.show(Window::new(3), None, SHOWN, &[SCREEN]);
        fewer.closed(Window::new(3));
        assert_eq!(fewer.show(Window::new(4), None, SHOWN, &[SCREEN]), None);
    }

    #[test]
    fn a_named_window_matches_by_name_in_any_order() {
        let mut first = Session::new(Record::default());
        first.show(Window::new(1), Some("editor"), SHOWN, &[SCREEN]);
        first.moved(Window::new(1), MOVED);
        let mut later = Session::new(first.record());
        later.show(Window::new(2), None, SHOWN, &[SCREEN]);
        assert_eq!(
            later.show(Window::new(3), Some("editor"), SHOWN, &[SCREEN]),
            Some(MOVED)
        );
    }

    #[test]
    fn a_closed_window_is_no_longer_followed() {
        let mut session = Session::new(Record::default());
        session.show(Window::new(1), None, SHOWN, &[SCREEN]);
        session.closed(Window::new(1));
        assert!(!session.knows(Window::new(1)));
        assert!(!session.moved(Window::new(1), MOVED));
    }
}
