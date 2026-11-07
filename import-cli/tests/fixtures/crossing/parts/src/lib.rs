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

/// iced_graphics's modules that iced re-exports as `advanced::graphics`.
pub mod graphics {
    pub mod color {
        //! Colors packed for shaders, as iced_graphics's `color`.

        /// A color, as iced_core's `Color`.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Color {
            /// Red.
            pub r: f32,
            /// Green.
            pub g: f32,
            /// Blue.
            pub b: f32,
            /// Alpha.
            pub a: f32,
        }

        impl From<[f32; 4]> for Color {
            fn from([r, g, b, a]: [f32; 4]) -> Self {
                Color { r, g, b, a }
            }
        }

        /// A color packed as four floats, as iced_graphics's `Packed`: its
        /// only field is private, so only `pack` makes one.
        #[derive(Debug, Clone, Copy, PartialEq)]
        #[repr(C)]
        pub struct Packed([f32; 4]);

        /// `color`, packed.
        pub fn pack(color: impl Into<Color>) -> Packed {
            let Color { r, g, b, a } = color.into();
            Packed([r, g, b, a])
        }
    }

    pub mod mesh {
        //! Triangles, as iced_graphics's `mesh`.

        /// A vertex with its own color, as iced_graphics's `SolidVertex2D`.
        #[derive(Debug, Clone, Copy, PartialEq)]
        #[repr(C)]
        pub struct SolidVertex2D {
            /// Where it is.
            pub position: [f32; 2],
            /// Its color.
            pub color: super::color::Packed,
        }

        /// Vertices and the indices of their triangles, as iced_graphics's
        /// `mesh::Indexed`.
        #[derive(Debug, Clone, PartialEq)]
        pub struct Indexed<T> {
            /// The vertices.
            pub vertices: Vec<T>,
            /// Three indices per triangle.
            pub indices: Vec<u32>,
        }

        /// A mesh, as iced_graphics's `Mesh`.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Mesh {
            /// Vertices of one color each.
            Solid {
                /// Its vertices and triangles.
                buffers: Indexed<SolidVertex2D>,
                /// How far it is moved.
                offset: [f32; 2],
            },
        }

        /// A vertex whose shade a function gives, which rawr can't hold.
        pub struct Shaded {
            /// Where it is.
            pub position: [f32; 2],
            /// Its shade at a point.
            pub shade: fn(f32) -> f32,
        }

        /// What `mesh` draws, as a renderer's `draw_mesh` would.
        pub fn describe(mesh: &Mesh) -> String {
            let Mesh::Solid { buffers, offset } = mesh;
            format!(
                "{} vertices, last {:?}, {} triangles at {offset:?}",
                buffers.vertices.len(),
                buffers.vertices.last(),
                buffers.indices.len() / 3,
            )
        }

        /// Where `shaded` is, and its shade there.
        pub fn shade(shaded: &Shaded) -> f32 {
            (shaded.shade)(shaded.position[0])
        }

        /// What draws meshes, as iced_graphics's `mesh::Renderer`.
        pub trait Renderer {
            /// Draws `mesh`.
            fn draw_mesh(&mut self, mesh: Mesh);
        }
    }
}

/// What draws, as iced_core's `renderer`.
/// Renderers that draw images, as iced_core's `image::Renderer`: each names
/// the handle it draws from.
pub mod imaging {
    pub trait Renderer {
        /// What the renderer draws an image from.
        type Handle;
    }
}

pub mod renderer {
    use crate::graphics::mesh::{self, Mesh};

    /// How far something moves, as iced_core's `Vector`.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Vector {
        /// Across.
        pub x: f32,
        /// Down.
        pub y: f32,
    }

    impl Vector {
        /// The vector `(x, y)`.
        pub const fn new(x: f32, y: f32) -> Self {
            Vector { x, y }
        }
    }

    /// What draws, as iced_core's `Renderer`, whose provided
    /// `with_translation` lends the renderer itself, as `&mut Self`, to a
    /// closure.
    pub trait Renderer: 'static {
        /// Starts moving what is drawn by `offset`.
        fn start_transformation(&mut self, offset: Vector);

        /// Stops moving what is drawn by the last offset.
        fn end_transformation(&mut self);

        /// Draws what `f` draws moved by `offset`.
        fn with_transformation(&mut self, offset: Vector, f: impl FnOnce(&mut Self)) {
            self.start_transformation(offset);
            f(self);
            self.end_transformation();
        }

        /// Draws what `f` draws moved by `translation`.
        fn with_translation(&mut self, translation: Vector, f: impl FnOnce(&mut Self)) {
            self.with_transformation(translation, f);
        }
    }

    /// A renderer that records what it draws, as iced's `Renderer`.
    pub struct Recorder {
        offset: Vector,
        steps: Vec<String>,
    }

    impl Recorder {
        /// A renderer that drew nothing.
        pub fn new() -> Self {
            Recorder {
                offset: Vector::new(0.0, 0.0),
                steps: Vec::new(),
            }
        }

        /// What it drew, in order.
        pub fn steps(&self) -> String {
            self.steps.join(",")
        }
    }

    impl Renderer for Recorder {
        fn start_transformation(&mut self, offset: Vector) {
            self.offset = Vector::new(self.offset.x + offset.x, self.offset.y + offset.y);
        }

        fn end_transformation(&mut self) {
            self.offset = Vector::new(0.0, 0.0);
        }
    }

    impl mesh::Renderer for Recorder {
        fn draw_mesh(&mut self, mesh: Mesh) {
            let Mesh::Solid { buffers, .. } = mesh;
            self.steps.push(format!(
                "{} vertices at {:?}",
                buffers.vertices.len(),
                [self.offset.x, self.offset.y]
            ));
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

pub mod widget {
    use super::PhantomData;

    /// A widget whose messages `F` turns, as iced_core's `widget::Map`,
    /// which iced's `Widget::map` gives and iced doesn't re-export: a
    /// program uses it without naming it.
    pub struct Map<W, F, A> {
        widget: W,
        mapper: F,
        input: PhantomData<A>,
    }

    impl<W, F, A> Map<W, F, A> {
        /// `widget`, its messages turned by `mapper`.
        pub fn new(widget: W, mapper: F) -> Self {
            Map {
                widget,
                mapper,
                input: PhantomData,
            }
        }

        /// The widget it wraps.
        pub fn widget(&self) -> &W {
            &self.widget
        }

        /// What turns its messages.
        pub fn mapper(&self) -> &F {
            &self.mapper
        }
    }
}

/// A point of the soft renderer's own, as tiny_skia's `Point`: a type of
/// this crate that `events` reaches only through a signature, beside its
/// own `Point` of the same name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Its x.
    pub x: f32,
    /// Its y.
    pub y: f32,
}
