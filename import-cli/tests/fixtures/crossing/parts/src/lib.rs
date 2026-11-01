//! Types `events` names only through its aliases, as iced names
//! iced_core's `Element` and iced_graphics's `geometry::Cache`: `events`
//! depends on this crate and re-exports none of it.

use std::cell::RefCell;
use std::marker::PhantomData;

pub mod geometry {
    use super::{PhantomData, RefCell};

    /// What a renderer draws, as iced_graphics's `geometry::Renderer`.
    pub trait Renderer {
        /// What it draws into.
        type Geometry;

        /// `label`, drawn.
        fn draw(label: &str) -> Self::Geometry;
    }

    /// The labels a renderer drew, kept until cleared, as iced_graphics's
    /// `geometry::Cache`.
    pub struct Cache<R: Renderer> {
        drawn: RefCell<Vec<String>>,
        renderer: PhantomData<R>,
    }

    impl<R: Renderer> Cache<R> {
        /// An empty cache.
        pub fn new() -> Self {
            Cache {
                drawn: RefCell::new(Vec::new()),
                renderer: PhantomData,
            }
        }

        /// Draws `label` into the cache.
        pub fn draw(&self, label: &str) {
            let _ = R::draw(label);
            self.drawn.borrow_mut().push(label.to_owned());
        }

        /// Draws `label` into the cache and gives what the renderer drew,
        /// as iced_graphics's `Cache::draw` gives `Renderer::Geometry`.
        pub fn sketch(&self, label: &str) -> R::Geometry {
            self.drawn.borrow_mut().push(label.to_owned());
            R::draw(label)
        }

        /// What the cache holds, in order.
        pub fn drawn(&self) -> String {
            self.drawn.borrow().join(",")
        }

        /// Empties the cache.
        pub fn clear(&self) {
            self.drawn.borrow_mut().clear();
        }
    }

    impl<R: Renderer> Default for Cache<R> {
        fn default() -> Self {
            Self::new()
        }
    }

    pub use path::Path;

    /// What a program draws on, as iced_graphics's `geometry::Frame`.
    pub struct Frame {
        steps: Vec<String>,
        depth: usize,
    }

    impl Frame {
        /// An empty frame.
        pub fn new() -> Self {
            Frame {
                steps: Vec::new(),
                depth: 0,
            }
        }

        /// Fills `shape` at the frame's current depth.
        pub fn fill(&mut self, shape: &str) {
            self.steps.push(format!("{shape}@{}", self.depth));
        }

        /// Runs `f` on the frame with its state saved, and restores it
        /// after, as iced_graphics's `Frame::with_save(&mut self, f: impl
        /// FnOnce(&mut Frame) -> R) -> R`.
        pub fn with_save<R>(&mut self, f: impl FnOnce(&mut Frame) -> R) -> R {
            self.depth += 1;
            let result = f(self);
            self.depth -= 1;
            result
        }

        /// What the frame drew, in order.
        pub fn steps(&self) -> String {
            self.steps.join(",")
        }
    }

    pub use stroke::{LineDash, Stroke};

    /// How a path is outlined, as iced_graphics's `geometry::stroke`.
    pub mod stroke {
        /// The dash pattern of a stroke, as iced_graphics's `LineDash<'a>`:
        /// public fields, one of them a borrowed slice.
        #[derive(Debug, Clone, Copy, Default)]
        pub struct LineDash<'a> {
            /// The lengths of the dashes and the gaps between them.
            pub segments: &'a [f32],
            /// Where in the pattern the stroke starts.
            pub offset: usize,
        }

        /// A stroke, as iced_graphics's `Stroke<'a>`.
        #[derive(Debug, Clone, Copy)]
        pub struct Stroke<'a> {
            /// Its width.
            pub width: f32,
            /// Its dash pattern.
            pub line_dash: LineDash<'a>,
        }

        impl Default for Stroke<'_> {
            fn default() -> Self {
                Stroke {
                    width: 1.0,
                    line_dash: LineDash::default(),
                }
            }
        }

        impl Stroke<'_> {
            /// What it draws, as a frame's `stroke` would.
            pub fn describe(&self) -> String {
                format!(
                    "stroke {} dash {:?}+{}",
                    self.width, self.line_dash.segments, self.line_dash.offset
                )
            }
        }
    }

    /// A traced shape and what traces it, as iced_graphics's
    /// `geometry::path`.
    pub mod path {
        /// The points a builder traced, as iced_graphics's `Path`.
        pub struct Path {
            points: Vec<i64>,
        }

        impl Path {
            /// The path `f` traces on a builder it borrows, as
            /// iced_graphics's `Path::new(f: impl FnOnce(&mut Builder))`.
            pub fn new(f: impl FnOnce(&mut Builder)) -> Self {
                let mut builder = Builder { points: Vec::new() };
                f(&mut builder);
                Path {
                    points: builder.points,
                }
            }

            /// The traced points, in order.
            pub fn points(&self) -> String {
                self.points
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
        }

        /// What traces a path, as iced_graphics's `path::Builder`.
        pub struct Builder {
            points: Vec<i64>,
        }

        impl Builder {
            /// Traces a line to `point`.
            pub fn line_to(&mut self, point: i64) {
                self.points.push(point);
            }
        }
    }
}

/// Text at a theme, as iced_core's `widget::Text<'a, Theme>`.
pub struct Note<'a, Theme> {
    text: String,
    look: PhantomData<&'a Theme>,
}

impl<Theme> Note<'_, Theme> {
    /// A note showing `text`.
    pub fn new(text: impl ToString) -> Self {
        Note {
            text: text.to_string(),
            look: PhantomData,
        }
    }

    /// What it shows.
    pub fn text(&self) -> String {
        self.text.clone()
    }
}
