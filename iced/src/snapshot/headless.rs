//! Pinned software rendering for deterministic artifacts.

use iced::advanced::renderer::{self, Headless};
use iced::widget::Widget as _;
use iced_runtime::core::renderer::Style;
use iced_runtime::core::theme::Base;
use iced_runtime::core::widget::operation::Text;
use iced_runtime::core::widget::{Id, Operation};
use iced_runtime::core::{keyboard, mouse, shell, time, window, Event, Rectangle, Size, Vector};
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
        super::fonts::embed_fira_sans();
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
            canvas
                .scale
                .min(canvas.max_pixels as f32 / largest)
                .max(1.0)
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
        view: &mut dyn FnMut(
            Vec<runtime::Message>,
            bool,
        ) -> iced::Element<'static, runtime::Message>,
        trees: &mut Trees,
        declared: Size,
        now: time::Instant,
        theme: &iced::Theme,
        scale: &mut Option<f32>,
    ) -> Result<(image::Image, window::RedrawRequest, Size), Error> {
        let (request, size) = self.settle_frame(view, trees, declared, now);
        let scale = *scale.get_or_insert_with(|| self.scale(size));
        let interface = trees.drawn.as_mut().expect("the frame settled");
        let image = self.draw(interface, theme, size, scale)?;
        Ok((image, request, size))
    }

    /// One frame as [`Self::frame`] runs it, settled but not drawn: the
    /// widget trees move on as they would, and nothing is rasterized.
    /// Gives when the drawn interface asked to be redrawn, and the logical
    /// size.
    pub(super) fn settle_frame(
        &mut self,
        view: &mut dyn FnMut(
            Vec<runtime::Message>,
            bool,
        ) -> iced::Element<'static, runtime::Message>,
        trees: &mut Trees,
        declared: Size,
        now: time::Instant,
    ) -> (window::RedrawRequest, Size) {
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
        let (interface, request) = self.settle_at(&mut drawn, size, cache, now);
        trees.drawn = Some(interface);
        trees.size = size;
        (request, size)
    }

    /// The places a click lands in the interface the last frame settled,
    /// in reading order (see [`arrange`]). Each text and container the
    /// interface reports, and each control that reports itself (a toggler,
    /// a radio, a slider, a checkbox without a label; see `widget::reported`),
    /// overlays included, is a candidate; one is kept
    /// when the interface, asked with the cursor at its centre and no
    /// event, shows a pointer, a text cursor or a grab. Asking runs no
    /// widget's `update`, so nothing is published and no State changes.
    /// At most [`PROBED`] candidates are asked, and none once `stop` says so.
    pub(super) fn targets(
        &mut self,
        trees: &mut Trees,
        stop: &dyn Fn() -> bool,
    ) -> Vec<super::Target> {
        let Some(interface) = trees.drawn.as_mut() else {
            return Vec::new();
        };
        let mut found = Candidates::default();
        interface.operate(&self.raw, &mut found);
        let mut candidates = found.found;
        let window = Rectangle::with_size(trees.size);
        for (at, candidate) in candidates.iter_mut().enumerate() {
            if at >= PROBED || stop() {
                break;
            }
            let centre = candidate.bounds.center();
            if !window.contains(centre) {
                continue;
            }
            let mut messages = shell::Bus::new();
            let (state, _) = interface.update(
                &window::Headless,
                &shell::Waker::noop(),
                &[],
                mouse::Cursor::Available(centre),
                &mut self.raw,
                &mut messages,
            );
            if let user_interface::State::Updated {
                mouse_interaction, ..
            } = state
            {
                candidate.clickable = matches!(
                    mouse_interaction,
                    mouse::Interaction::Pointer
                        | mouse::Interaction::Text
                        | mouse::Interaction::Grab
                );
                candidate.pointer = mouse_interaction == mouse::Interaction::Pointer;
            }
        }
        arrange(candidates)
    }

    /// Act on `target` in the interface the last frame settled, as a
    /// person would and as `iced_test`'s `Simulator` does: the cursor
    /// moves to its centre, then for a click the left button is pressed
    /// and released there, and for a slider's step ↓ or ↑ is tapped. Gives
    /// the messages its widgets published.
    pub(super) fn act(
        &mut self,
        trees: &mut Trees,
        target: &super::Target,
    ) -> Vec<runtime::Message> {
        let interface = trees.drawn.as_mut().expect("a frame settled before a step");
        let at = target.bounds.center();
        let mut events = vec![Event::Mouse(mouse::Event::CursorMoved { position: at })];
        match target.action {
            super::Action::Click => events.extend([
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ]),
            super::Action::Decrease => events.extend(tap(keyboard::key::Named::ArrowDown)),
            super::Action::Increase => events.extend(tap(keyboard::key::Named::ArrowUp)),
        }
        let mut messages = shell::Bus::new();
        let _ = interface.update(
            &window::Headless,
            &shell::Waker::noop(),
            &events,
            mouse::Cursor::Available(at),
            &mut self.raw,
            &mut messages,
        );
        messages.drain().map(|(message, _)| message).collect()
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

/// The events of tapping `key`, pressed and released with no modifiers,
/// as `iced_test`'s `tap_key` sends them.
fn tap(key: keyboard::key::Named) -> [Event; 2] {
    let key = keyboard::Key::Named(key);
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    [
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        }),
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
        }),
    ]
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
    /// The logical size `drawn` is laid out at.
    size: Size,
}

/// The most candidates [`Renderer::targets`] asks about.
const PROBED: usize = 512;

/// Something an interface reports that a click might land on: a text, a
/// container or a control that reports itself, at its bounds on screen.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Candidate {
    bounds: Rectangle,
    /// A text's content or a control's label (empty for none); `None` for
    /// a container.
    text: Option<String>,
    /// Whether an overlay reported it, over the interface's root.
    overlay: bool,
    /// Whether the cursor at its centre shows that a click does something:
    /// a pointer, a text cursor or a grab.
    clickable: bool,
    /// Whether that is a pointer.
    pointer: bool,
    /// Whether it is a slider, which keys step: two targets, ↓ and ↑.
    slider: bool,
}

/// The texts, containers and controls an interface reports, in the order it reports
/// them, at their place on screen: a scrollable's content moves by its
/// offset. What is reported outside the first traversal of the root is an
/// overlay's.
#[derive(Default)]
struct Candidates {
    found: Vec<Candidate>,
    depth: usize,
    /// Whether the root's traversal is over, so overlays report now.
    rooted: bool,
    translation: Vector,
    /// A scrollable's offset, for the content it traverses next.
    scrolled: Vector,
}

impl Candidates {
    fn push(&mut self, bounds: Rectangle, text: Option<String>) -> Option<&mut Candidate> {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        let shift = self.translation + self.scrolled;
        self.found.push(Candidate {
            bounds: bounds - shift,
            text,
            overlay: self.rooted || self.depth == 0,
            clickable: false,
            pointer: false,
            slider: false,
        });
        self.found.last_mut()
    }
}

impl Operation for Candidates {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let translation = self.translation;
        self.translation += std::mem::take(&mut self.scrolled);
        self.depth += 1;
        operate(self);
        self.depth -= 1;
        self.translation = translation;
        if self.depth == 0 {
            self.rooted = true;
        }
    }

    fn container(&mut self, _id: Option<&Id>, bounds: Rectangle, _viewport: &Rectangle) {
        let _ = self.push(bounds, None);
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content: Size,
        translation: Vector,
        _state: &mut dyn iced_runtime::core::widget::operation::Scrollable,
    ) {
        self.scrolled = translation;
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Text) {
        let _ = self.push(bounds, Some(collapsed(&state.text())));
    }

    // A toggler, a radio, a label-less checkbox or a slider reports itself
    // through `widget::reported`, labelled by its own label (empty for
    // none).
    fn custom(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        use crate::widget::reported::{Kind, Report};
        if let Some(report) = state.downcast_ref::<Report>()
            && let Some(candidate) = self.push(bounds, Some(collapsed(&report.label)))
        {
            candidate.slider = report.kind == Kind::Slider;
        }
    }
}

/// `text` with each run of whitespace one space, and none at its ends.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The targets among `candidates` (as [`Candidates`] reports them, each
/// probed), in reading order:
///
/// - What an overlay covers is hidden, so a candidate under one is dropped.
/// - Of the clickable candidates, the innermost wins: one holding another
///   in the same layer is one clickable region with it (a button and its
///   label), and the inner one is kept. Of two with the same bounds, the
///   later (the inner) is kept.
/// - A container is a target only when it holds no text and shows a
///   pointer: an icon's button. A layout holding texts whose centre lands
///   on a slider or a text input is not one.
/// - A text is labelled by its content and a control by its label. A
///   container, or a control without a label, is labelled by its number.
/// - A slider is two targets, one step down and one up, labelled `−` and
///   `+` after its label, or after `Slider` when it has none.
/// - Reading order is by lines, top to bottom, each left to right. A
///   target whose centre is above the bottom of a line's first target is
///   on that line.
/// - A label that repeats gets its ordinal: `+ (1)`, `+ (2)`.
pub(super) fn arrange(candidates: Vec<Candidate>) -> Vec<super::Target> {
    let covered = |candidate: &Candidate| {
        !candidate.overlay
            && candidates
                .iter()
                .any(|other| other.overlay && other.bounds.contains(candidate.bounds.center()))
    };
    let shown: Vec<&Candidate> = candidates.iter().filter(|c| !covered(c)).collect();
    let within = |inner: &Rectangle, outer: &Rectangle| {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.width <= outer.x + outer.width
            && inner.y + inner.height <= outer.y + outer.height
    };
    // Whether a candidate other than `at`, in its layer and inside it,
    // passes `test`.
    let inside = |at: usize, test: fn(&Candidate) -> bool| {
        let candidate = shown[at];
        shown.iter().enumerate().any(|(other_at, other)| {
            other_at != at
                && other.overlay == candidate.overlay
                && within(&other.bounds, &candidate.bounds)
                && (other.bounds != candidate.bounds || other_at > at)
                && test(other)
        })
    };
    let mut kept: Vec<super::Target> = shown
        .iter()
        .enumerate()
        .filter(|&(at, candidate)| {
            candidate.clickable
                && !inside(at, |other| other.clickable)
                && (candidate.text.is_some()
                    || candidate.pointer && !inside(at, |other| other.text.is_some()))
        })
        .flat_map(|(_, candidate)| {
            let label = candidate.text.clone().unwrap_or_default();
            let target = |label, action| super::Target {
                label,
                bounds: candidate.bounds,
                action,
            };
            if candidate.slider {
                let name = if label.is_empty() { "Slider" } else { &label };
                vec![
                    target(format!("{name} −"), super::Action::Decrease),
                    target(format!("{name} +"), super::Action::Increase),
                ]
            } else {
                vec![target(label, super::Action::Click)]
            }
        })
        .collect();
    kept.sort_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
    let mut ordered = Vec::with_capacity(kept.len());
    let mut rest = kept.into_iter().peekable();
    while let Some(first) = rest.next() {
        let bottom = first.bounds.y + first.bounds.height;
        let mut line = vec![first];
        while let Some(next) = rest.next_if(|next| next.bounds.center().y < bottom) {
            line.push(next);
        }
        line.sort_by(|a, b| a.bounds.x.total_cmp(&b.bounds.x));
        ordered.extend(line);
    }
    for (at, target) in ordered.iter_mut().enumerate() {
        if target.label.is_empty() {
            target.label = format!("#{}", at + 1);
        }
    }
    let labels: Vec<String> = ordered.iter().map(|target| target.label.clone()).collect();
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (target, label) in ordered.iter_mut().zip(&labels) {
        if labels.iter().filter(|other| *other == label).count() > 1 {
            let ordinal = seen.entry(label).or_insert(0);
            *ordinal += 1;
            target.label = format!("{label} ({ordinal})");
        }
    }
    ordered
}

#[cfg(test)]
mod target_tests {
    use super::{arrange, Candidate};
    use iced_runtime::core::Rectangle;

    fn at(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text: Option<&str>,
        clickable: bool,
    ) -> Candidate {
        Candidate {
            bounds: Rectangle {
                x,
                y,
                width,
                height,
            },
            text: text.map(str::to_owned),
            overlay: false,
            clickable,
            pointer: clickable,
            slider: false,
        }
    }

    fn labels(candidates: Vec<Candidate>) -> Vec<String> {
        arrange(candidates)
            .into_iter()
            .map(|target| target.label)
            .collect()
    }

    #[test]
    fn a_buttons_label_is_its_target_and_text_alone_is_not() {
        // A column holding a button "Increment", a value, a button
        // "Decrement": the column's centre lands on the value.
        let candidates = vec![
            at(0.0, 0.0, 100.0, 120.0, None, false),
            at(10.0, 0.0, 80.0, 30.0, None, true),
            at(15.0, 5.0, 70.0, 20.0, Some("Increment"), true),
            at(40.0, 40.0, 20.0, 40.0, Some("0"), false),
            at(10.0, 90.0, 80.0, 30.0, None, true),
            at(15.0, 95.0, 70.0, 20.0, Some("Decrement"), true),
        ];
        let targets = arrange(candidates);
        let labels: Vec<&str> = targets.iter().map(|target| target.label.as_str()).collect();
        assert_eq!(labels, ["Increment", "Decrement"]);
        assert_eq!(targets[0].bounds.center().y, 15.0, "the label's centre");
    }

    #[test]
    fn targets_read_in_lines_left_to_right() {
        // Two lines of buttons of different heights, reported column by
        // column.
        let candidates = vec![
            at(100.0, 2.0, 40.0, 20.0, Some("b"), true),
            at(100.0, 40.0, 40.0, 20.0, Some("d"), true),
            at(0.0, 0.0, 40.0, 24.0, Some("a"), true),
            at(0.0, 42.0, 40.0, 16.0, Some("c"), true),
        ];
        assert_eq!(labels(candidates), ["a", "b", "c", "d"]);
    }

    #[test]
    fn repeated_labels_get_ordinals_and_a_bare_target_its_number() {
        let candidates = vec![
            at(0.0, 0.0, 20.0, 20.0, Some("+"), true),
            at(30.0, 0.0, 20.0, 20.0, Some("-"), true),
            at(60.0, 0.0, 20.0, 20.0, Some("+"), true),
            // A clickable container with no text inside: an icon button.
            at(90.0, 0.0, 20.0, 20.0, None, true),
        ];
        assert_eq!(labels(candidates), ["+ (1)", "-", "+ (2)", "#4"]);
    }

    #[test]
    fn a_layout_whose_centre_lands_on_a_slider_is_no_target() {
        // A column of labels and sliders: its centre shows a slider's grab.
        let mut column = at(0.0, 0.0, 100.0, 200.0, None, true);
        column.pointer = false;
        let candidates = vec![
            column,
            at(0.0, 0.0, 100.0, 20.0, Some("Width:"), false),
            at(0.0, 100.0, 100.0, 20.0, Some("Margin:"), false),
        ];
        assert!(labels(candidates).is_empty());
        // A slider alone in a container: a grab, not a pointer.
        let mut alone = at(0.0, 0.0, 100.0, 20.0, None, true);
        alone.pointer = false;
        assert!(labels(vec![alone]).is_empty());
    }

    #[test]
    fn a_slider_is_a_step_down_and_a_step_up() {
        use crate::snapshot::Action;
        // A column whose centre lands on a slider, the slider reporting
        // itself inside it, and a button beside it.
        let mut column = at(0.0, 0.0, 100.0, 60.0, None, true);
        column.pointer = false;
        let mut slider = at(0.0, 20.0, 100.0, 20.0, Some(""), true);
        slider.pointer = false;
        slider.slider = true;
        let candidates = vec![
            column,
            at(0.0, 0.0, 100.0, 20.0, Some("Width:"), false),
            slider.clone(),
            at(120.0, 20.0, 40.0, 20.0, Some("Reset"), true),
        ];
        let targets = arrange(candidates);
        let found: Vec<(&str, Action)> = targets
            .iter()
            .map(|target| (target.label.as_str(), target.action))
            .collect();
        assert_eq!(
            found,
            [
                ("Slider −", Action::Decrease),
                ("Slider +", Action::Increase),
                ("Reset", Action::Click),
            ]
        );
        assert_eq!(targets[0].bounds, targets[1].bounds);
        // Two sliders: each pair gets its ordinal.
        let mut lower = slider.clone();
        lower.bounds.y = 60.0;
        assert_eq!(
            labels(vec![slider, lower]),
            [
                "Slider − (1)",
                "Slider + (1)",
                "Slider − (2)",
                "Slider + (2)"
            ]
        );
    }

    #[test]
    fn an_overlay_hides_what_it_covers() {
        let mut option = at(0.0, 20.0, 60.0, 20.0, Some("Apple"), true);
        option.overlay = true;
        let mut menu = at(0.0, 20.0, 60.0, 40.0, None, true);
        menu.overlay = true;
        let candidates = vec![
            at(0.0, 0.0, 60.0, 20.0, Some("Pick"), true),
            // Under the menu: its centre shows the menu's pointer.
            at(0.0, 30.0, 60.0, 20.0, Some("Hidden"), true),
            menu,
            option,
        ];
        assert_eq!(labels(candidates), ["Pick", "Apple"]);
    }
}

#[cfg(test)]
mod report_tests {
    use iced::widget::Widget as _;
    use iced_runtime::core::Size;
    use iced_runtime::user_interface::{self, UserInterface};

    use crate::widget::reported::{reported, Kind};

    #[test]
    fn a_control_that_reports_itself_is_a_candidate_by_its_label() {
        let mut renderer = super::Renderer::new().unwrap();
        let toggler = iced::widget::toggler(false).label("Dark  mode").boxed();
        let element = iced::widget::column![
            iced::widget::text("Title"),
            reported(toggler, Some("Dark  mode"), Kind::Toggle),
        ]
        .boxed();
        let mut interface = UserInterface::build(
            element,
            Size::new(200.0, 100.0),
            user_interface::Cache::default(),
            &mut renderer.raw,
        );
        let mut found = super::Candidates::default();
        interface.operate(&renderer.raw, &mut found);
        let texts: Vec<&str> = found
            .found
            .iter()
            .filter_map(|candidate| candidate.text.as_deref())
            .collect();
        assert_eq!(texts, ["Title", "Dark mode"]);
        let control = &found.found[found.found.len() - 1];
        assert!(control.bounds.y > 0.0, "below the title");
        assert!(control.bounds.width > 0.0 && control.bounds.height > 0.0);
    }
}

/// A view to render: given the messages the last frame published, the
/// element of the next.
pub(super) type View<'a> =
    dyn FnMut(Vec<runtime::Message>) -> iced::Element<'static, runtime::Message> + 'a;

/// The most frames a render runs before it draws the last.
pub(super) const FRAMES: usize = 8;
