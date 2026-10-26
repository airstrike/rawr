//! A small copy of iced's `pane_grid` state API: a generic `State<T>` of
//! the app's own pane values, keyed by `Pane`, with the shapes iced's
//! example uses.

pub mod grid {
    use std::collections::BTreeMap;

    /// A pane's key.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Pane(usize);

    /// A split between two panes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Split(usize);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Axis {
        Horizontal,
        Vertical,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct ResizeEvent {
        pub split: Split,
        pub ratio: f32,
    }

    #[derive(Debug, Clone, Copy)]
    pub enum Target {
        Edge,
        Pane(Pane),
    }

    #[derive(Debug, Clone, Copy)]
    pub enum DragEvent {
        Picked { pane: Pane },
        Dropped { pane: Pane, target: Target },
        Canceled { pane: Pane },
    }

    /// The panes and the values the app keeps for each.
    #[derive(Debug, Clone)]
    pub struct State<T> {
        panes: BTreeMap<Pane, T>,
        ratios: Vec<(Split, Axis, f32)>,
        next: usize,
        maximized: Option<Pane>,
    }

    impl<T> State<T> {
        pub fn new(first: T) -> (Self, Pane) {
            let mut panes = BTreeMap::new();
            panes.insert(Pane(0), first);
            (
                State {
                    panes,
                    ratios: Vec::new(),
                    next: 1,
                    maximized: None,
                },
                Pane(0),
            )
        }

        pub fn len(&self) -> usize {
            self.panes.len()
        }

        pub fn get(&self, pane: Pane) -> Option<&T> {
            self.panes.get(&pane)
        }

        pub fn get_mut(&mut self, pane: Pane) -> Option<&mut T> {
            self.panes.get_mut(&pane)
        }

        pub fn split(&mut self, axis: Axis, pane: Pane, state: T) -> Option<(Pane, Split)> {
            if !self.panes.contains_key(&pane) {
                return None;
            }
            let new = Pane(self.next);
            let split = Split(self.next);
            self.next += 1;
            self.panes.insert(new, state);
            self.ratios.push((split, axis, 0.5));
            Some((new, split))
        }

        pub fn resize(&mut self, split: Split, ratio: f32) {
            for (candidate, _, value) in &mut self.ratios {
                if *candidate == split {
                    *value = ratio;
                }
            }
        }

        pub fn ratio(&self, split: Split) -> Option<f32> {
            self.ratios
                .iter()
                .find(|(candidate, ..)| *candidate == split)
                .map(|(.., ratio)| *ratio)
        }

        pub fn close(&mut self, pane: Pane) -> Option<(T, Pane)> {
            if self.panes.len() < 2 {
                return None;
            }
            let value = self.panes.remove(&pane)?;
            let sibling = *self.panes.keys().next()?;
            Some((value, sibling))
        }

        pub fn maximize(&mut self, pane: Pane) {
            self.maximized = Some(pane);
        }

        pub fn maximized(&self) -> Option<Pane> {
            self.maximized
        }
    }
}
