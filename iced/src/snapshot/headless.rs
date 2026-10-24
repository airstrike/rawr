//! Pinned software rendering for deterministic artifacts.

use iced::advanced::renderer::{self, Headless};
use iced::widget::Widget as _;
use iced_runtime::core::renderer::Style;
use iced_runtime::core::theme::Base;
use iced_runtime::core::widget::{Id, Operation};
use iced_runtime::core::{mouse, shell, time, window, Event, Rectangle, Size};
use iced_runtime::user_interface::{self, UserInterface};

use crate::runtime;

use super::{image, Error};

pub(super) const BACKEND: &str = "tiny-skia";
pub(super) const FONT: &str = "Fira Sans Regular (iced embedded)";
pub(super) const ICED_REVISION: &str = "be88873d38544455e26c7a104eb757da2cbbac6b";
pub(super) const SCALE: f32 = 1.0;

pub(super) struct Renderer {
    raw: iced::Renderer,
    canvas: Option<super::Canvas>,
}

impl Renderer {
    pub(super) fn new() -> Result<Self, Error> {
        let raw = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings {
                font: iced::Font::DEFAULT,
                text_size: iced::Pixels(16.0),
                ..renderer::Settings::default()
            },
            Some(BACKEND),
        ))
        .ok_or(Error::Renderer)?;
        if raw.name() != BACKEND {
            return Err(Error::Renderer);
        }
        Ok(Self { raw, canvas: None })
    }

    /// A renderer for one-off images sized and scaled by `canvas`.
    pub(super) fn with_canvas(canvas: super::Canvas) -> Result<Self, Error> {
        Ok(Self {
            canvas: Some(canvas),
            ..Self::new()?
        })
    }

    /// A function framing content as the canvas does: with its margin, in
    /// the container a fitted size measures.
    pub(super) fn framing(
        &self,
    ) -> impl Fn(iced::Element<'static, runtime::Message>) -> iced::Element<'static, runtime::Message>
           + use<> {
        let margin = self.canvas.map(|canvas| canvas.margin as f32);
        move |content| match margin {
            Some(margin) => iced::widget::container(content).padding(margin).boxed(),
            None => content,
        }
    }

    /// The logical size to render framed `view` at: the declared size, or
    /// with a fitting canvas its settled content's own size, falling back
    /// per axis when the content fills it (a `Fill` width has no own size).
    pub(super) fn size(&mut self, view: &mut View<'_>, declared: Size) -> Size {
        let Some(canvas) = self.canvas.filter(|canvas| canvas.fit) else {
            return declared;
        };
        let interface = self.settle(view, declared);
        self.fit(interface, declared, canvas).1
    }

    /// The size `canvas` fits a settled `interface` to within `declared`,
    /// and the interface laid out at the width it measured with.
    fn fit(
        &mut self,
        mut interface: UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer>,
        declared: Size,
        canvas: super::Canvas,
    ) -> (
        UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer>,
        Size,
    ) {
        let fallback = Size::new(canvas.fallback.0 as f32, canvas.fallback.1 as f32);
        let mut size = self.measure(&mut interface);
        if size.width >= declared.width {
            let width = fallback.width.min(declared.width);
            interface = interface.relayout(Size::new(width, declared.height), &mut self.raw);
            size = self.measure(&mut interface);
            size.width = width;
        }
        if size.height >= declared.height {
            size.height = fallback.height.min(declared.height);
        }
        let size = Size::new(size.width.ceil().max(1.0), size.height.ceil().max(1.0));
        (interface, size)
    }

    /// The size of the root container `framing` adds.
    fn measure(
        &mut self,
        interface: &mut UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer>,
    ) -> Size {
        struct Root(Option<Rectangle>);
        impl Operation for Root {
            // Into the interface's root, and no deeper.
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                if self.0.is_none() {
                    operate(self);
                }
            }

            fn container(&mut self, _id: Option<&Id>, bounds: Rectangle, _viewport: &Rectangle) {
                self.0.get_or_insert(bounds);
            }
        }
        let mut root = Root(None);
        interface.operate(&self.raw, &mut root);
        root.0.map_or(Size::ZERO, |bounds| bounds.size())
    }

    /// `view` once it settles, as iced's runtime would show it. Each frame
    /// delivers one `RedrawRequested` at a virtual clock, which starts at one
    /// instant and moves only to a time the interface asked to be redrawn at
    /// (a sensor's `delay`), so every time is relative to the start. A frame
    /// that publishes a message or changes the interface (a component that
    /// took its own Event, a sensor that fired) is rebuilt from `view`,
    /// given that frame's messages, over the same widget tree; one that
    /// changes nothing settles, unless it asked for a redraw at a time, to
    /// which the clock jumps. A redraw on the next frame (an animation)
    /// settles. At most [`FRAMES`] frames run.
    fn settle(
        &mut self,
        view: &mut View<'_>,
        size: Size,
    ) -> UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer> {
        let mut now = time::Instant::now();
        let mut cache = user_interface::Cache::default();
        let mut published = Vec::new();
        for frame in 1.. {
            let element = view(std::mem::take(&mut published));
            let mut interface = UserInterface::build(element, size, cache, &mut self.raw);
            let mut messages = shell::Bus::new();
            let (state, _) = interface.update(
                &window::Headless,
                &shell::Waker::noop(),
                &[Event::Window(window::Event::RedrawRequested(now))],
                mouse::Cursor::Unavailable,
                &mut self.raw,
                &mut messages,
            );
            let later = match state {
                user_interface::State::Updated {
                    change: user_interface::Change::None,
                    redraw_request,
                    ..
                } if messages.is_empty() => match redraw_request {
                    window::RedrawRequest::At(at) if at > now => Some(at),
                    _ => return interface,
                },
                _ => None,
            };
            if frame == FRAMES {
                return interface;
            }
            if let Some(at) = later {
                now = at;
            }
            published = messages.drain().map(|(message, _)| message).collect();
            cache = interface.into_cache();
        }
        unreachable!("the frames end at the cap")
    }

    /// The pixels per logical pixel for an image of `size`: the canvas
    /// scale while the larger side stays within `max_pixels`, never below 1x.
    fn scale(&self, size: Size) -> f32 {
        self.canvas.map_or(SCALE, |canvas| {
            let largest = size.width.max(size.height);
            canvas.scale.min(canvas.max_pixels as f32 / largest).max(1.0)
        })
    }

    /// Draw `view` at `size` once it settles.
    pub(super) fn render(
        &mut self,
        view: &mut View<'_>,
        theme: &iced::Theme,
        size: Size,
    ) -> Result<image::Image, Error> {
        let scale = self.scale(size);
        let mut interface = self.settle(view, size);
        self.draw(&mut interface, theme, size, scale)
    }

    /// One frame of a recording at the clock's `now`, over the widget trees
    /// of the frame before. `view(published, true)` gives the content to
    /// measure and `view(published, false)` the view to draw, each settled
    /// within the frame (each message its widgets publish applied while the
    /// clock stands still). The image is sized as the canvas sizes the
    /// content and drawn at `scale` (the first frame's, once set). Gives
    /// when the drawn interface asked to be redrawn, and the logical size.
    pub(super) fn frame(
        &mut self,
        view: &mut dyn FnMut(Vec<runtime::Message>, bool) -> iced::Element<'static, runtime::Message>,
        trees: &mut Trees,
        declared: Size,
        now: time::Instant,
        theme: &iced::Theme,
        scale: &mut Option<f32>,
    ) -> Result<(image::Image, window::RedrawRequest, Size), Error> {
        let size = match self.canvas.filter(|canvas| canvas.fit) {
            Some(canvas) => {
                let mut content = |published| view(published, true);
                let cache = std::mem::take(&mut trees.content);
                let (interface, _) = self.settle_at(&mut content, declared, cache, now);
                let (interface, size) = self.fit(interface, declared, canvas);
                trees.content = interface.into_cache();
                size
            }
            None => declared,
        };
        let mut drawn = |published| view(published, false);
        let cache = trees
            .drawn
            .take()
            .map_or_else(user_interface::Cache::default, UserInterface::into_cache);
        let (mut interface, request) = self.settle_at(&mut drawn, size, cache, now);
        let scale = *scale.get_or_insert_with(|| self.scale(size));
        let image = self.draw(&mut interface, theme, size, scale)?;
        trees.drawn = Some(interface);
        Ok((image, request, size))
    }

    /// The interface the last [`Self::frame`] drew, drawn again at
    /// `scale`. It is the same interface, updated as that frame updated
    /// it, so its widgets keep their status (a button not yet updated
    /// draws as disabled), and the app's State and the clock are untouched.
    pub(super) fn redraw(
        &mut self,
        trees: &mut Trees,
        size: Size,
        theme: &iced::Theme,
        scale: f32,
    ) -> Result<image::Image, Error> {
        let interface = trees.drawn.as_mut().expect("a frame was drawn before");
        self.draw(interface, theme, size, scale)
    }

    /// `view` settled at `now` over `cache`, as [`Self::settle`] settles it
    /// but with the clock held: a frame that publishes a message or changes
    /// the interface is rebuilt, and the first that does neither ends it.
    /// Gives when the interface asked to be redrawn; one still changing
    /// after [`FRAMES`] asks for the next frame.
    fn settle_at(
        &mut self,
        view: &mut View<'_>,
        size: Size,
        mut cache: user_interface::Cache,
        now: time::Instant,
    ) -> (
        UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer>,
        window::RedrawRequest,
    ) {
        let mut published = Vec::new();
        for frame in 1.. {
            let element = view(std::mem::take(&mut published));
            let mut interface = UserInterface::build(element, size, cache, &mut self.raw);
            let mut messages = shell::Bus::new();
            let (state, _) = interface.update(
                &window::Headless,
                &shell::Waker::noop(),
                &[Event::Window(window::Event::RedrawRequested(now))],
                mouse::Cursor::Unavailable,
                &mut self.raw,
                &mut messages,
            );
            if let user_interface::State::Updated {
                change: user_interface::Change::None,
                redraw_request,
                ..
            } = state
                && messages.is_empty()
            {
                return (interface, redraw_request);
            }
            if frame == FRAMES {
                return (interface, window::RedrawRequest::NextFrame);
            }
            published = messages.drain().map(|(message, _)| message).collect();
            cache = interface.into_cache();
        }
        unreachable!("the frames end at the cap")
    }

    /// Draw a settled `interface` at `size` and `scale`.
    fn draw(
        &mut self,
        interface: &mut UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer>,
        theme: &iced::Theme,
        size: Size,
        scale: f32,
    ) -> Result<image::Image, Error> {
        let base = theme.base();
        interface.draw(
            &mut self.raw,
            theme,
            &Style {
                text_color: base.text_color,
            },
            mouse::Cursor::Unavailable,
        );
        let physical = Size::new(
            (size.width * scale).round() as u32,
            (size.height * scale).round() as u32,
        );
        let mut bytes = self.raw.screenshot(physical, scale, base.background_color);
        if let Some(canvas) = self.canvas.filter(|canvas| canvas.margin > 0) {
            let margin = (canvas.margin as f32 * scale).round() as u32;
            fill_margin(&mut bytes, physical.width, physical.height, margin);
        }
        image::Image::new(physical.width, physical.height, bytes)
    }
}

/// Paint the canvas's margin, `margin` pixels on each side of an image
/// `width` by `height`, with the content's own background: the color its
/// edge is all in, as a canvas filling the view draws. A content edge of
/// several colors keeps the theme's background there.
fn fill_margin(rgba: &mut [u8], width: u32, height: u32, margin: u32) {
    let (width, height, margin) = (width as usize, height as usize, margin as usize);
    if width <= 2 * margin || height <= 2 * margin {
        return;
    }
    let pixel = |rgba: &[u8], x: usize, y: usize| {
        let at = (y * width + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    };
    let (left, right) = (margin, width - margin - 1);
    let (top, bottom) = (margin, height - margin - 1);
    let edge = pixel(rgba, left, top);
    let uniform = (left..=right)
        .all(|x| pixel(rgba, x, top) == edge && pixel(rgba, x, bottom) == edge)
        && (top..=bottom).all(|y| pixel(rgba, left, y) == edge && pixel(rgba, right, y) == edge);
    if !uniform {
        return;
    }
    for y in 0..height {
        for x in 0..width {
            if (left..=right).contains(&x) && (top..=bottom).contains(&y) {
                continue;
            }
            let at = (y * width + x) * 4;
            rgba[at..at + 4].copy_from_slice(&edge);
        }
    }
}

#[cfg(test)]
mod margin_tests {
    #[test]
    fn a_uniform_content_edge_colors_the_margin() {
        let (side, margin) = (6_u32, 1_u32);
        let mut rgba = [9_u8, 9, 9, 255].repeat((side * side) as usize);
        for y in 1..5 {
            for x in 1..5 {
                let at = ((y * side + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[20, 18, 24, 255]);
            }
        }
        let center = ((2 * side + 2) * 4) as usize;
        rgba[center..center + 4].copy_from_slice(&[255, 0, 0, 255]);
        super::fill_margin(&mut rgba, side, side, margin);
        assert_eq!(&rgba[0..4], &[20, 18, 24, 255]);
        assert_eq!(&rgba[center..center + 4], &[255, 0, 0, 255]);

        // A content edge of two colors keeps the theme's margin.
        let mut mixed = [9_u8, 9, 9, 255].repeat((side * side) as usize);
        let edge = ((side + 1) * 4) as usize;
        mixed[edge..edge + 4].copy_from_slice(&[1, 2, 3, 255]);
        super::fill_margin(&mut mixed, side, side, margin);
        assert_eq!(&mixed[0..4], &[9, 9, 9, 255]);
    }
}

/// What a recording builds each frame over: the widget trees of the
/// content it measures, and the interface of the view it last drew.
#[derive(Default)]
pub(super) struct Trees {
    content: user_interface::Cache,
    drawn: Option<UserInterface<'static, runtime::Message, iced::Theme, iced::Renderer>>,
}

/// A view to render: given the messages the last frame published, the
/// element of the next.
pub(super) type View<'a> =
    dyn FnMut(Vec<runtime::Message>) -> iced::Element<'static, runtime::Message> + 'a;

/// The most frames a render runs before it draws the last.
pub(super) const FRAMES: usize = 8;
