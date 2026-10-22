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
        let fallback = Size::new(canvas.fallback.0 as f32, canvas.fallback.1 as f32);
        let mut interface = self.settle(view, declared);
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
        Size::new(size.width.ceil().max(1.0), size.height.ceil().max(1.0))
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

    /// Draw `view` at `size` once it settles.
    pub(super) fn render(
        &mut self,
        view: &mut View<'_>,
        theme: &iced::Theme,
        size: Size,
    ) -> Result<image::Image, Error> {
        // Rasterize at the canvas scale while the larger side stays within
        // `max_pixels`, never below 1x.
        let scale = self.canvas.map_or(SCALE, |canvas| {
            let largest = size.width.max(size.height);
            canvas.scale.min(canvas.max_pixels as f32 / largest).max(1.0)
        });
        let mut interface = self.settle(view, size);
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
        let bytes = self.raw.screenshot(physical, scale, base.background_color);
        drop(interface);
        image::Image::new(physical.width, physical.height, bytes)
    }
}

/// A view to render: given the messages the last frame published, the
/// element of the next.
pub(super) type View<'a> =
    dyn FnMut(Vec<runtime::Message>) -> iced::Element<'static, runtime::Message> + 'a;

/// The most frames a render runs before it draws the last.
pub(super) const FRAMES: usize = 8;
