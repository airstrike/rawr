//! iced's `advanced::Widget`, implemented by a script type.
//!
//! A script type that implements the trait converts into an [`Instance`]:
//! its value and its impl's methods. Lowered, it is [`Custom`], a native
//! iced widget that calls those methods. Each call borrows the call's Tree,
//! renderer and Shell, and the wrapped child widgets, through a
//! [`lend::Loan`] for the length of the call. `layout` changes the Tree in
//! place, and a child's `layout` has finished when the script reads its
//! size. `draw` records its drawing and Custom replays it after the call
//! returns, with each child's own drawing in its place, inside the layers
//! and transformations around it (D82). `update`'s Shell requests are kept
//! and applied after it returns.
//!
//! A method the impl does not write is iced's default, run natively.

pub(crate) mod lend;

use std::cell::RefCell;
use std::sync::Arc;

use iced::advanced::layout::{Layout, Limits};
use iced::advanced::widget::{tree, Tree as NativeTree};
use iced::advanced::{renderer as native_renderer, Shell as NativeShell, Widget};
use iced::{mouse, Length, Rectangle, Size};
use rawr::{host, item, Dynamic, EvalAltResult};

use super::builtin::{arity, runtime_error, take, witness_error, Witnessed};
use crate::renderer::Command;
use crate::runtime;
use lend::{Children, Frame, Lent, Loan};

/// iced's `advanced::Widget` trait, which script types implement.
pub(crate) struct WidgetTrait;
/// iced's `advanced::widget::Meta`.
pub(crate) struct MetaTrait;
/// iced's `advanced::Renderer`.
pub(crate) struct RendererTrait;
/// iced's `advanced::graphics::mesh::Renderer`.
pub(crate) struct MeshRendererTrait;

/// The trait's methods in declaration order, as `Instance::new` takes them.
pub(crate) const SIZE: usize = 0;
pub(crate) const TAG: usize = 1;
pub(crate) const STATE: usize = 2;
pub(crate) const DIFF: usize = 3;
pub(crate) const LAYOUT: usize = 4;
pub(crate) const DRAW: usize = 5;
pub(crate) const UPDATE: usize = 6;
pub(crate) const INTERACTION: usize = 7;
pub(crate) const OPERATE: usize = 8;
pub(crate) const OVERLAY: usize = 9;
pub(crate) const HOOKS: usize = 10;

/// A script `advanced::Widget`: its value, its impl's methods, and which of
/// them the impl writes itself (bit `n` for method `n`).
#[derive(Clone)]
pub(crate) struct Instance {
    witness: host::runtime::Witness,
    identity: super::Identity,
    value: Dynamic,
    hooks: Arc<[item::Value]>,
    written: u64,
}

/// iced's `Mesh::Solid { buffers, transformation, clip_bounds }`, built
/// field by field and made a `Mesh` by `into`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SolidMesh {
    buffers: iced::advanced::graphics::mesh::Indexed<iced::advanced::graphics::mesh::SolidVertex2D>,
    transformation: iced::Transformation,
    clip_bounds: Rectangle,
}

/// The Message argument of an `Element` that a wrapping widget forwards to,
/// which those calls never read: any Message attests as it.
pub(crate) struct Erased;

pub(crate) const ERASED: &str = "iced::advanced::widget::Erased";

/// iced's `Never`, the Message of a view that sends none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Never {}

/// iced's `Size<Length>`, which `Widget::size` returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LengthSize(pub(crate) Size<Length>);

/// iced's `widget::Tree` as one call receives it: the call, and the path
/// from the call's tree to this subtree.
#[derive(Clone)]
pub(crate) struct Tree {
    nonce: lend::Nonce,
    path: Arc<[usize]>,
}

/// iced's `Shell`, as one `update` call receives it.
#[derive(Clone)]
pub(crate) struct Shell {
    nonce: lend::Nonce,
}

/// iced's `tree::Tag`: iced's own, as a wrapped Element's, or a script
/// type's logical tag, its name and its schema (`Tag::of::<State>()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    Native(tree::Tag),
    Script(Arc<str>),
}

/// iced's `tree::State`: iced's own, which the widget returning it from
/// `state` hands to iced once, or a script value (`State::new(value)`).
#[derive(Clone)]
pub(crate) enum State {
    Native(std::rc::Rc<std::cell::RefCell<Option<tree::State>>>),
    Script(Dynamic),
}

/// What a script widget keeps in iced's Tree: its State, and the logical
/// tag it was made under. Every script State has one native tag, so a
/// reload that keeps the widget keeps the State, and `diff` makes a new
/// one when the logical tag (the State's type or schema) differs.
struct ScriptState {
    tag: Arc<str>,
    value: Dynamic,
}

/// `tree.state`, as one call receives it.
#[derive(Clone)]
pub(crate) struct TreeState {
    nonce: lend::Nonce,
    path: Arc<[usize]>,
}

/// iced's `&mut dyn widget::Operation`, as one `operate` call receives it.
#[derive(Clone)]
pub(crate) struct Operation {
    nonce: lend::Nonce,
}

/// An overlay a wrapping widget's `overlay` returns: its child's, which
/// iced builds once the call returns.
#[derive(Clone)]
pub(crate) struct Overlay {
    child: usize,
    path: Arc<[usize]>,
    layout: Layout,
    translation: iced::Vector,
}

// SAFETY: a `tree::State` holds `Box<dyn Any>`. iced makes and keeps it on
// the UI thread, and so does every script call that carries one.
unsafe impl Send for State {}
// SAFETY: as above; the mutex orders the single take.
unsafe impl Sync for State {}

/// What one call asked for besides its result: its drawing, and its Shell
/// requests.
#[derive(Default)]
pub(crate) struct Effects {
    commands: Vec<Command>,
    children: Vec<ChildDraw>,
    redraw: bool,
    capture: bool,
    messages: Vec<Dynamic>,
}

/// A wrapped child's draw, recorded in its place.
struct ChildDraw {
    child: usize,
    path: Arc<[usize]>,
    style: native_renderer::Style,
    layout: Layout,
    cursor: mouse::Cursor,
    viewport: Rectangle,
}

thread_local! {
    static EFFECTS: RefCell<Vec<Effects>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::advanced::widget::Instance::new", |call, args| {
            if args.len() != HOOKS + 2 {
                return Err(arity(
                    "advanced::widget::Instance::new",
                    HOOKS + 2,
                    args.len(),
                ));
            }
            let value = args[0].take();
            let hooks = args[1..=HOOKS]
                .iter_mut()
                .map(|hook| take::<item::Value>(hook, "advanced::Widget method"))
                .collect::<Result<Arc<[_]>, _>>()?;
            let written = args[HOOKS + 1].as_int().map_err(|_| {
                runtime_error("`advanced::widget::Instance::new` expects its mask".into())
            })?;
            Ok(Dynamic::from(Instance {
                witness: call.output_witness().map_err(witness_error)?,
                identity: super::Identity::unique(),
                value,
                hooks,
                written: written as u64,
            }))
        })
        .expect("the iced facade must retain advanced::widget::Instance::new");
    super::helpers::bind_widget_type::<Instance>(builder, "iced::advanced::widget::Instance");
    bindings::bind(builder);
}

impl Witnessed for Instance {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl super::Descriptor for Instance {
    fn lower(&self, context: &super::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::Widget::boxed(Custom {
            value: self.value.clone(),
            instance: self.clone(),
            children: Children::new(context.owned()),
            context: context.owned(),
            failure: RefCell::new(None),
            logical: RefCell::new(None),
        })
    }

    fn identity(&self) -> super::Identity {
        self.identity.clone()
    }
}

/// A script `advanced::Widget` as an iced widget.
struct Custom {
    instance: Instance,
    /// The script value, as the `&mut self` methods leave it.
    value: Dynamic,
    context: super::OwnedContext,
    children: Children,
    /// The failure of the last call, which `draw` shows.
    failure: RefCell<Option<String>>,
    /// The logical tag `tag` last returned for a script State.
    logical: RefCell<Option<Arc<str>>>,
}

impl Custom {
    fn written(&self, hook: usize) -> bool {
        self.instance.written & (1 << hook) != 0
    }

    /// Call the method `hook` with `arguments`, its receiver first, while
    /// `loan` lends the call what it borrows. The call's effects are
    /// returned beside its result.
    fn call(&self, hook: usize, arguments: &mut [Dynamic]) -> (Result<Dynamic, String>, Effects) {
        EFFECTS.with(|effects| effects.borrow_mut().push(Effects::default()));
        let result = match self
            .context
            .context()
            .invoke(&self.instance.hooks[hook], arguments)
        {
            Some(result) => result,
            None => Err("this widget's code is not available".into()),
        };
        let effects = EFFECTS
            .with(|effects| effects.borrow_mut().pop())
            .unwrap_or_default();
        (result, effects)
    }

    fn fail(&self, hook: &str, error: &str) {
        *self.failure.borrow_mut() = Some(format!("`{hook}` failed: {error}"));
    }

    fn theme(&self) -> Dynamic {
        Dynamic::from(self.context.context().theme().clone())
    }

    /// The first value a call returned: a method with `&mut` parameters
    /// returns `[value, ..parameters]`.
    fn returned(value: Dynamic) -> Dynamic {
        match value.try_cast_result::<rawr::Array>() {
            Ok(mut array) if !array.is_empty() => array.swap_remove(0),
            Ok(_) => Dynamic::UNIT,
            Err(value) => value,
        }
    }
}

impl iced::advanced::widget::Meta for Custom {}

impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Custom {
    fn size(&self) -> Size<Length> {
        let shrink = Size::new(Length::Shrink, Length::Shrink);
        let (result, _) = {
            let _loan = Loan::new(Lent::None, None, None, &self.children);
            self.call(SIZE, &mut [self.value.clone()])
        };
        match result {
            Ok(size) => size.try_cast::<LengthSize>().map_or(shrink, |size| size.0),
            Err(error) => {
                self.fail("size", &error);
                shrink
            }
        }
    }

    fn tag(&self) -> tree::Tag {
        if !self.written(TAG) {
            return tree::Tag::stateless();
        }
        let (result, _) = {
            let _loan = Loan::new(Lent::None, None, None, &self.children);
            self.call(TAG, &mut [self.value.clone()])
        };
        match result.map(|tag| tag.try_cast::<Tag>()) {
            Ok(Some(Tag::Native(tag))) => tag,
            Ok(Some(Tag::Script(logical))) => {
                *self.logical.borrow_mut() = Some(logical);
                tree::Tag::of::<ScriptState>()
            }
            Ok(None) => tree::Tag::stateless(),
            Err(error) => {
                self.fail("tag", &error);
                tree::Tag::stateless()
            }
        }
    }

    fn state(&self) -> tree::State {
        if !self.written(STATE) {
            return tree::State::None;
        }
        let (result, _) = {
            let _loan = Loan::new(Lent::None, None, None, &self.children);
            self.call(STATE, &mut [self.value.clone()])
        };
        match result.map(|state| state.try_cast::<State>()) {
            Ok(Some(State::Native(state))) => state
                .try_borrow_mut()
                .ok()
                .and_then(|mut state| state.take())
                .unwrap_or(tree::State::None),
            Ok(Some(State::Script(value))) => {
                if self.logical.borrow().is_none() && self.written(TAG) {
                    let _ = self.tag();
                }
                let tag = self
                    .logical
                    .borrow()
                    .clone()
                    .unwrap_or_else(|| Arc::from(""));
                tree::State::new(ScriptState { tag, value })
            }
            Ok(None) => tree::State::None,
            Err(error) => {
                self.fail("state", &error);
                tree::State::None
            }
        }
    }

    fn diff(&mut self, tree: &mut NativeTree) {
        // A script State made under another logical tag, as after a reload
        // that changed its type or fields, starts again from `state`.
        if tree.tag == tree::Tag::of::<ScriptState>() {
            if self.logical.borrow().is_none() {
                let _ = self.tag();
            }
            let logical = self.logical.borrow().clone();
            let stale = match &tree.state {
                tree::State::Some(state) => state
                    .downcast_ref::<ScriptState>()
                    .is_none_or(|state| Some(&state.tag) != logical.as_ref()),
                tree::State::None => true,
            };
            if stale {
                tree.state = self.state();
            }
        }
        if !self.written(DIFF) {
            tree.children.clear();
            return;
        }
        let mut arguments = [self.value.clone(), Dynamic::UNIT];
        let (result, _) = {
            let loan = Loan::new(Lent::Mutable(tree), None, None, &self.children);
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            self.call(DIFF, &mut arguments)
        };
        match result {
            Ok(_) => self.value = std::mem::take(&mut arguments[0]),
            Err(error) => self.fail("diff", &error),
        }
    }

    fn layout(&mut self, tree: &mut NativeTree, renderer: &iced::Renderer, limits: &Limits) {
        let mut arguments = [
            self.value.clone(),
            Dynamic::UNIT,
            Dynamic::from(crate::marker::Renderer),
            Dynamic::from(*limits),
        ];
        let (result, _) = {
            let loan = Loan::new(Lent::Mutable(tree), Some(renderer), None, &self.children);
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            self.call(LAYOUT, &mut arguments)
        };
        match result {
            Ok(_) => self.value = std::mem::take(&mut arguments[0]),
            Err(error) => self.fail("layout", &error),
        }
    }

    fn draw(
        &self,
        tree: &NativeTree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &native_renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(failure) = self.failure.borrow().clone() {
            draw_failure(renderer, layout.bounds(), failure);
            return;
        }
        let mut arguments = [
            self.value.clone(),
            Dynamic::UNIT,
            Dynamic::from(crate::marker::Renderer),
            self.theme(),
            Dynamic::from(*style),
            Dynamic::from(layout),
            Dynamic::from(cursor),
            Dynamic::from(*viewport),
        ];
        let (result, effects) = {
            let loan = Loan::new(Lent::Shared(tree), None, None, &self.children);
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            self.call(DRAW, &mut arguments)
        };
        if let Err(error) = result {
            self.fail("draw", &error);
            draw_failure(renderer, layout.bounds(), format!("`draw` failed: {error}"));
            return;
        }
        let Effects {
            commands, children, ..
        } = effects;
        #[cfg(test)]
        tests::REPLAYED.with(|replayed| replayed.borrow_mut().push(commands.clone()));
        crate::renderer::replay(commands, renderer, |renderer, index| {
            let Some(draw) = children.get(index) else {
                return;
            };
            let Some(subtree) = subtree(tree, &draw.path) else {
                return;
            };
            let drawn = self.children.with(draw.child, |child| {
                child.draw(
                    subtree,
                    renderer,
                    theme,
                    &draw.style,
                    draw.layout,
                    draw.cursor,
                    &draw.viewport,
                );
            });
            if let Err(error) = drawn {
                self.fail("draw", &error.to_string());
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut NativeTree,
        event: &iced::Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut NativeShell<'_, runtime::Message>,
        viewport: &Rectangle,
    ) {
        if !self.written(UPDATE) {
            return;
        }
        let mut arguments = [
            self.value.clone(),
            Dynamic::UNIT,
            Dynamic::from(event.clone()),
            Dynamic::from(layout),
            Dynamic::from(cursor),
            Dynamic::from(crate::marker::Renderer),
            Dynamic::UNIT,
            Dynamic::from(*viewport),
        ];
        let (result, effects) = {
            let loan = Loan::new(
                Lent::Mutable(tree),
                Some(renderer),
                Some(&mut *shell),
                &self.children,
            );
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            arguments[6] = Dynamic::from(Shell {
                nonce: loan.nonce(),
            });
            self.call(UPDATE, &mut arguments)
        };
        match result {
            Ok(_) => {
                self.value = std::mem::take(&mut arguments[0]);
                let context = self.context.context();
                for message in effects.messages {
                    shell.publish(context.message(message));
                }
                if effects.redraw {
                    shell.request_redraw();
                }
                if effects.capture {
                    shell.capture_event();
                }
            }
            Err(error) => {
                self.fail("update", &error);
                shell.request_redraw();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &NativeTree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if !self.written(INTERACTION) {
            return mouse::Interaction::None;
        }
        let mut arguments = [
            self.value.clone(),
            Dynamic::UNIT,
            Dynamic::from(layout),
            Dynamic::from(cursor),
            Dynamic::from(*viewport),
            Dynamic::from(crate::marker::Renderer),
        ];
        let (result, _) = {
            let loan = Loan::new(Lent::Shared(tree), Some(renderer), None, &self.children);
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            self.call(INTERACTION, &mut arguments)
        };
        match result {
            Ok(interaction) => Self::returned(interaction)
                .try_cast::<mouse::Interaction>()
                .unwrap_or_default(),
            Err(error) => {
                self.fail("mouse_interaction", &error);
                mouse::Interaction::None
            }
        }
    }

    fn operate(
        &mut self,
        tree: &mut NativeTree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        if !self.written(OPERATE) {
            return;
        }
        let mut arguments = [
            self.value.clone(),
            Dynamic::UNIT,
            Dynamic::from(layout),
            Dynamic::from(*viewport),
            Dynamic::from(crate::marker::Renderer),
            Dynamic::UNIT,
        ];
        let (result, _) = {
            let loan = Loan::operating(tree, renderer, operation, &self.children);
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            arguments[5] = Dynamic::from(Operation {
                nonce: loan.nonce(),
            });
            self.call(OPERATE, &mut arguments)
        };
        match result {
            Ok(_) => self.value = std::mem::take(&mut arguments[0]),
            Err(error) => self.fail("operate", &error),
        }
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut NativeTree,
        layout: Layout,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
        window: Size,
    ) -> Vec<iced::advanced::overlay::Element<'a, runtime::Message, iced::Theme, iced::Renderer>>
    {
        if !self.written(OVERLAY) {
            return Vec::new();
        }
        let mut arguments = [
            self.value.clone(),
            Dynamic::UNIT,
            Dynamic::from(layout),
            Dynamic::from(crate::marker::Renderer),
            Dynamic::from(*viewport),
            Dynamic::from(translation),
            Dynamic::from(window),
        ];
        let (result, _) = {
            let loan = Loan::new(
                Lent::Mutable(&mut *tree),
                Some(renderer),
                None,
                &self.children,
            );
            arguments[1] = Dynamic::from(Tree::root(loan.nonce()));
            self.call(OVERLAY, &mut arguments)
        };
        let requests = match result {
            Ok(requests) => Self::returned(requests)
                .try_cast::<rawr::Array>()
                .unwrap_or_default(),
            Err(error) => {
                self.fail("overlay", &error);
                return Vec::new();
            }
        };
        self.value = std::mem::take(&mut arguments[0]);
        // iced lends a child's widget and tree to one overlay at a time,
        // so the first request is the widget's overlay.
        let Some(request) = requests
            .into_iter()
            .find_map(|request| request.try_cast::<Overlay>())
        else {
            return Vec::new();
        };
        let Some(subtree) = request
            .path
            .iter()
            .try_fold(tree, |tree, &index| tree.children.get_mut(index))
        else {
            return Vec::new();
        };
        let Some((_, child)) = self.children.lowered_mut().get_mut(request.child) else {
            return Vec::new();
        };
        child.overlay(
            subtree,
            request.layout,
            renderer,
            viewport,
            request.translation,
            window,
        )
    }
}

fn subtree<'a>(tree: &'a NativeTree, path: &[usize]) -> Option<&'a NativeTree> {
    path.iter()
        .try_fold(tree, |tree, &index| tree.children.get(index))
}

/// A failed call's message, drawn in the widget's bounds.
#[cfg(feature = "canvas")]
fn draw_failure(renderer: &mut iced::Renderer, bounds: Rectangle, message: String) {
    use iced::advanced::graphics::geometry::Renderer as _;
    use iced::advanced::Renderer as _;
    use iced::widget::canvas;

    let mut frame = canvas::Frame::new(renderer, bounds.size());
    frame.fill_text(canvas::Text {
        content: message,
        color: iced::Color::from_rgb(0.9, 0.2, 0.2),
        max_width: bounds.width.max(1.0),
        ..canvas::Text::default()
    });
    let geometry = frame.into_geometry();
    renderer.with_translation(iced::Vector::new(bounds.x, bounds.y), |renderer| {
        renderer.draw_geometry(geometry);
    });
}

/// A failed call's bounds, outlined in red: without iced's `canvas` there
/// is no frame to write the message in.
#[cfg(not(feature = "canvas"))]
fn draw_failure(renderer: &mut iced::Renderer, bounds: Rectangle, _message: String) {
    use iced::advanced::Renderer as _;

    renderer.fill_quad(
        iced::advanced::renderer::Quad {
            bounds,
            border: iced::Border {
                color: iced::Color::from_rgb(0.9, 0.2, 0.2),
                width: 2.0,
                radius: 0.0.into(),
            },
            ..iced::advanced::renderer::Quad::default()
        },
        iced::Background::Color(iced::Color::TRANSPARENT),
    );
}

impl Tree {
    fn root(nonce: lend::Nonce) -> Self {
        Self {
            nonce,
            path: Arc::from([]),
        }
    }

    fn frame(&self) -> Result<Frame, Box<EvalAltResult>> {
        Frame::find(self.nonce, "Tree").map_err(|error| runtime_error(error.to_string()))
    }
}

impl Shell {
    fn frame(&self) -> Result<Frame, Box<EvalAltResult>> {
        Frame::find(self.nonce, "Shell").map_err(|error| runtime_error(error.to_string()))
    }
}

/// Run `f` on the effects of the call running now.
fn record<R>(what: &str, f: impl FnOnce(&mut Effects) -> R) -> Result<R, Box<EvalAltResult>> {
    EFFECTS.with(|effects| {
        effects
            .borrow_mut()
            .last_mut()
            .map(f)
            .ok_or_else(|| runtime_error(format!("`{what}` is used outside a widget's call")))
    })
}

mod bindings;
pub(crate) mod declare;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use iced::{Color, Point, Size};

    use crate::renderer::Command;

    thread_local! {
        /// What each `Custom::draw` replayed, in order.
        pub(super) static REPLAYED: RefCell<Vec<Vec<Command>>> = const { RefCell::new(Vec::new()) };
    }

    const CUSTOM_WIDGET: &str = include_str!("../../tests/fixtures/advanced/custom_widget.rs");
    const CUSTOM_QUAD: &str = include_str!("../../tests/fixtures/advanced/custom_quad.rs");
    const GEOMETRY: &str = include_str!("../../tests/fixtures/advanced/geometry.rs");
    const LOUPE: &str = include_str!("../../tests/fixtures/advanced/loupe.rs");

    /// The example's view drawn once, the cursor at `cursor`: the drawing
    /// each script widget replayed.
    fn drawn(source: &str, cursor: Option<Point>) -> Vec<Vec<Command>> {
        REPLAYED.with(|replayed| replayed.borrow_mut().clear());
        let adapter = crate::application::Compiler::new()
            .unwrap()
            .check_named_crate_source("main.rs", source)
            .unwrap_or_else(|error| panic!("{error}"))
            .prepare()
            .unwrap()
            .into_adapter();
        let mut simulator = iced_test::simulator(adapter.view());
        if let Some(cursor) = cursor {
            simulator.point_at(cursor);
        }
        simulator.draw(&iced::Theme::Dark);
        assert!(
            adapter.diagnostics().is_empty(),
            "{:?}",
            adapter.diagnostics()
        );
        REPLAYED.with(|replayed| std::mem::take(&mut *replayed.borrow_mut()))
    }

    fn quads(commands: &[Command]) -> Vec<(iced::advanced::renderer::Quad, iced::Background)> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Quad(quad, background) => Some((*quad, *background)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn custom_widget_lays_out_its_circle_and_fills_one_rounded_quad() {
        let drawn = drawn(CUSTOM_WIDGET, None);
        let [commands] = &drawn[..] else {
            panic!("one script widget draws once, found {drawn:?}");
        };
        let [(quad, background)] = &quads(commands)[..] else {
            panic!("the circle is one quad, found {commands:?}");
        };
        assert_eq!(quad.bounds.size(), Size::new(100.0, 100.0));
        assert_eq!(quad.border.radius, 50.0.into());
        assert_eq!(*background, iced::Background::Color(Color::BLACK));
    }

    #[test]
    fn custom_quad_draws_its_border_shadow_and_radius() {
        let drawn = drawn(CUSTOM_QUAD, None);
        let [(quad, _)] = &quads(&drawn[0])[..] else {
            panic!("one quad, found {drawn:?}");
        };
        assert_eq!(quad.bounds.size(), Size::new(200.0, 200.0));
        assert_eq!(quad.border.color, Color::from_rgb(1.0, 0.0, 0.0));
        assert_eq!(quad.border.radius, 50.0.into());
        assert_eq!(quad.shadow.offset, iced::Vector::new(0.0, 8.0));
        assert_eq!(quad.shadow.blur_radius, 16.0);
        assert!(!quad.snap);
    }

    #[test]
    fn custom_quad_sets_a_nested_facade_field_in_place() {
        let source = format!(
            "{CUSTOM_QUAD}\nfn shift() -> Message {{ Message::ShadowXOffsetChanged(-30.0) }}\nfn corner() -> Message {{ Message::RadiusTopLeftChanged(10.0) }}\n"
        );
        let mut adapter = crate::application::Compiler::new()
            .unwrap()
            .check_named_crate_source("main.rs", &source)
            .unwrap_or_else(|error| panic!("{error}"))
            .prepare()
            .unwrap()
            .into_adapter();
        let messages = ["shift", "corner"].map(|name| adapter.message_of(name).unwrap().unwrap());
        adapter.deliver(messages);
        REPLAYED.with(|replayed| replayed.borrow_mut().clear());
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.draw(&iced::Theme::Dark);
        let drawn = REPLAYED.with(|replayed| std::mem::take(&mut *replayed.borrow_mut()));
        let [(quad, _)] = &quads(&drawn[0])[..] else {
            panic!("one quad, found {drawn:?}");
        };
        assert_eq!(quad.shadow.offset, iced::Vector::new(-30.0, 8.0));
        assert_eq!(quad.border.radius.top_left, 10.0);
        assert_eq!(quad.border.radius.top_right, 50.0);
    }

    #[test]
    fn geometry_draws_its_mesh_translated_and_follows_the_cursor() {
        let mesh = |cursor| {
            let drawn = drawn(GEOMETRY, cursor);
            let commands = drawn.into_iter().next().expect("the rainbow draws");
            let [Command::StartTransformation(translation), Command::Mesh(mesh), Command::EndTransformation] =
                &commands[..]
            else {
                panic!("a translated mesh, found {commands:?}");
            };
            let iced::advanced::graphics::Mesh::Solid { buffers, .. } = mesh else {
                panic!("a solid mesh, found {mesh:?}");
            };
            (translation.translation(), buffers.clone())
        };
        let (origin, buffers) = mesh(None);
        assert_eq!(buffers.vertices.len(), 9);
        assert_eq!(buffers.indices.len(), 24);
        let corner = buffers.vertices[5].position;
        assert_eq!(
            buffers.vertices[0].position,
            [corner[0] / 2.0, corner[1] / 2.0]
        );

        let cursor = Point::new(origin.x + 30.0, origin.y + 40.0);
        let (_, buffers) = mesh(Some(cursor));
        assert_eq!(buffers.vertices[0].position, [30.0, 40.0]);
    }

    #[test]
    fn loupe_draws_its_child_in_place_and_zoomed_inside_a_layer_under_the_cursor() {
        let drawn = drawn(LOUPE, None);
        assert_eq!(drawn, vec![vec![Command::Child(0)]]);

        let drawn = drawn_loupe_zoomed();
        let [commands] = &drawn[..] else {
            panic!("loupe draws once, found {drawn:?}");
        };
        let [Command::StartLayer(bounds), Command::StartTransformation(zoom), Command::Child(0), Command::EndTransformation, Command::EndLayer] =
            &commands[..]
        else {
            panic!("a zoomed child inside a layer, found {commands:?}");
        };
        assert!(bounds.width > 0.0 && bounds.height > 0.0);
        assert_eq!(zoom.scale_factor(), 3.0);
    }

    /// Loupe drawn with the cursor at the center of its bounds.
    fn drawn_loupe_zoomed() -> Vec<Vec<Command>> {
        let drawn = drawn(LOUPE, Some(Point::new(512.0, 384.0)));
        if drawn
            .iter()
            .flatten()
            .any(|command| matches!(command, Command::StartLayer(_)))
        {
            return drawn;
        }
        panic!("the window's center is over loupe's content: {drawn:?}")
    }

    #[test]
    fn a_failing_layout_replays_nothing_and_the_widget_shows_its_failure() {
        let failing = CUSTOM_WIDGET.replace(
            "tree.size = Size::new(self.radius * 2.0, self.radius * 2.0);",
            "tree.size = Size::new(self.radius * 2.0, self.radius * 2.0);\n            let _ = [1, 2][self.radius as usize];",
        );
        let adapter = crate::application::Compiler::new()
            .unwrap()
            .check_named_crate_source("main.rs", &failing)
            .unwrap_or_else(|error| panic!("{error}"))
            .prepare()
            .unwrap()
            .into_adapter();
        REPLAYED.with(|replayed| replayed.borrow_mut().clear());
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.draw(&iced::Theme::Dark);
        assert!(REPLAYED.with(|replayed| replayed.borrow().is_empty()));
    }

    /// A widget with a State of its own, counting the presses on it, and
    /// drawing a bar ten pixels wide per press.
    const TALLY: &str = r#"
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::Shell;
use iced::{mouse, Color, Event, Length, Rectangle, Size};

#[derive(Default)]
struct Presses {
    count: i64,
}

struct Tally;

impl widget::Meta for Tally {}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Tally
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Presses>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Presses::default())
    }

    fn layout(&mut self, tree: &mut widget::Tree, _renderer: &Renderer, _limits: &layout::Limits) {
        tree.size = Size::new(200.0, 20.0);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            if cursor.is_over(layout.bounds()) {
                let state = tree.state.downcast_mut::<Presses>();
                state.count += 1;
                shell.request_redraw();
            }
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<Presses>();
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(layout.position(), Size::new(10.0 * state.count as f32, 20.0)),
                ..renderer::Quad::default()
            },
            Color::BLACK,
        );
    }
}

#[derive(Default)]
struct App;

#[derive(Debug, Clone)]
enum Message {}

impl App {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl iced::Widget<Message> {
        Tally
    }
}

pub fn main() -> iced::Result {
    iced::run(App::update, App::view)
}
"#;

    fn adapter(source: &str) -> crate::program::Adapter {
        crate::application::Compiler::new()
            .unwrap()
            .check_named_crate_source("main.rs", source)
            .unwrap_or_else(|error| panic!("{error}"))
            .prepare()
            .unwrap()
            .into_adapter()
    }

    fn bar_width(drawn: &[Vec<Command>]) -> f32 {
        let [(quad, _)] = &quads(drawn.last().expect("the widget draws"))[..] else {
            panic!("one quad, found {drawn:?}");
        };
        quad.bounds.width
    }

    #[test]
    fn a_widget_keeps_a_state_of_its_own_and_changes_it_through_downcast_mut() {
        let adapter = adapter(TALLY);
        let mut simulator = iced_test::simulator(adapter.view());
        REPLAYED.with(|replayed| replayed.borrow_mut().clear());
        simulator.draw(&iced::Theme::Dark);
        assert_eq!(bar_width(&REPLAYED.with(|replayed| replayed.take())), 0.0);
        simulator.point_at(Point::new(5.0, 5.0));
        simulator.simulate(iced_test::simulator::click());
        simulator.simulate(iced_test::simulator::click());
        simulator.draw(&iced::Theme::Dark);
        assert_eq!(bar_width(&REPLAYED.with(|replayed| replayed.take())), 20.0);
        assert!(
            adapter.diagnostics().is_empty(),
            "{:?}",
            adapter.diagnostics()
        );
    }

    #[test]
    fn a_reload_keeps_a_script_state_while_its_logical_tag_and_schema_match() {
        use iced::advanced::widget::Tree;
        use iced::advanced::Widget as _;

        /// The view's subtree that holds a script State.
        fn script(tree: &mut Tree) -> Option<&mut Tree> {
            if tree.tag == iced::advanced::widget::tree::Tag::of::<super::ScriptState>() {
                return Some(tree);
            }
            tree.children.iter_mut().find_map(script)
        }
        let seeded = |tree: &mut Tree| {
            let tree = script(tree).expect("the view holds a script State");
            let iced::advanced::widget::tree::State::Some(state) = &mut tree.state else {
                panic!("a script State");
            };
            state
                .downcast_mut::<super::ScriptState>()
                .expect("a script State")
                .value = rawr::Dynamic::from(7_i64);
        };
        let kept = |tree: &mut Tree| match &script(tree).expect("a script State").state {
            iced::advanced::widget::tree::State::Some(state) => state
                .downcast_ref::<super::ScriptState>()
                .is_some_and(|state| state.value.as_int() == Ok(7)),
            iced::advanced::widget::tree::State::None => false,
        };

        let first = adapter(TALLY);
        let mut element = first.content();
        let mut tree = Tree::new(&element);
        tree.diff(&mut element);
        seeded(&mut tree);

        // The same code compiled again: the State carries over.
        let again = adapter(&TALLY.replace("struct Tally;", "struct Tally;\n"));
        let mut element = again.content();
        tree.diff(&mut element);
        assert!(kept(&mut tree), "a matching tag and schema keep the State");

        // A field more: the schema differs, so the State starts again.
        let changed =
            adapter(&TALLY.replace("    count: i64,", "    count: i64,\n    extra: bool,"));
        let mut element = changed.content();
        tree.diff(&mut element);
        assert!(!kept(&mut tree), "a changed schema starts the State again");
        let _ = element.size();
    }

    /// A widget that wraps its content and forwards every method to it.
    const WRAP: &str = r#"
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{overlay, Shell};
use iced::widget::{text, tooltip, button};
use iced::{mouse, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

struct Wrap<W> {
    content: W,
}

impl<W> widget::Meta for Wrap<W> {}

impl<W, Message> Widget<Message, Theme, Renderer> for Wrap<W>
where
    W: Widget<Message, Theme, Renderer>,
{
    fn tag(&self) -> tree::Tag {
        self.content.tag()
    }

    fn state(&self) -> tree::State {
        STATE
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        self.content.diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    fn layout(&mut self, tree: &mut widget::Tree, renderer: &Renderer, limits: &layout::Limits) {
        self.content.layout(tree, renderer, limits);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    OPERATE

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut widget::Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content.overlay(tree, layout, renderer, viewport, translation, window)
    }
}

#[derive(Default)]
struct App;

#[derive(Debug, Clone)]
enum Message {
    Pressed,
}

impl App {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl iced::Widget<Message> {
        let content: iced::Element<'_, Message> = CONTENT.boxed();
        Wrap { content }
    }
}

pub fn main() -> iced::Result {
    iced::run(App::update, App::view)
}
"#;

    const OPERATE: &str = r#"fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.operate(tree, layout, viewport, renderer, operation);
    }"#;

    fn wrap(content: &str, state: &str, operate: bool) -> String {
        WRAP.replace("CONTENT", content)
            .replace("STATE", state)
            .replace("OPERATE", if operate { OPERATE } else { "" })
    }

    #[test]
    fn operate_reaches_the_wrapped_element_only_when_the_widget_forwards_it() {
        let source = wrap(r#"text("hidden")"#, "self.content.state()", true);
        let adapter = adapter(&source);
        let mut simulator = iced_test::simulator(adapter.view());
        assert!(simulator.find("hidden").is_ok());
        assert!(
            adapter.diagnostics().is_empty(),
            "{:?}",
            adapter.diagnostics()
        );

        let adapter = self::adapter(&wrap(r#"text("hidden")"#, "self.content.state()", false));
        let mut simulator = iced_test::simulator(adapter.view());
        assert!(
            simulator.find("hidden").is_err(),
            "iced's default operate reaches nothing"
        );
    }

    #[test]
    fn overlay_shows_the_wrapped_tooltip() {
        let source = wrap(
            r#"tooltip(button("hover").on_press(Message::Pressed), "the tip", tooltip::Position::Bottom)"#,
            "self.content.state()",
            false,
        );
        let adapter = adapter(&source);
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.draw(&iced::Theme::Dark);
        assert!(
            simulator.find("the tip").is_err(),
            "no tip before the hover"
        );
        simulator.point_at(Point::new(10.0, 10.0));
        simulator.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: Point::new(10.0, 10.0),
        })]);
        simulator.draw(&iced::Theme::Dark);
        assert!(
            simulator.find("the tip").is_ok(),
            "the tooltip's overlay shows"
        );
    }

    #[test]
    fn a_panic_in_a_wrapped_element_is_the_call_s_error() {
        // The text input's tree is made without its State, so its `layout`
        // panics; the panic is the wrapping widget's `layout` failure.
        let source = wrap(
            r#"iced::widget::text_input("", "")"#,
            "tree::State::None",
            false,
        );
        let adapter = adapter(&source);
        REPLAYED.with(|replayed| replayed.borrow_mut().clear());
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.draw(&iced::Theme::Dark);
        assert!(REPLAYED.with(|replayed| replayed.borrow().is_empty()));
    }

    #[test]
    fn a_failed_view_says_why() {
        let adapter = adapter(
            r#"
use iced::widget::text;

#[derive(Default)]
struct App;

#[derive(Debug, Clone)]
enum Message {}

impl App {
    fn update(&mut self, _message: Message) {}

    fn view(&self) -> impl iced::Widget<Message> {
        let items = vec![1, 2];
        text(items[5])
    }
}

pub fn main() -> iced::Result {
    iced::run(App::update, App::view)
}
"#,
        );
        let mut simulator = iced_test::simulator(adapter.content());
        assert!(simulator.find("Rawr view unavailable").is_ok());
        let reason = simulator.find(
            |candidate: iced_test::selector::Candidate<'_>| match candidate {
                iced_test::selector::Candidate::Text { content, .. }
                    if content.contains("out of bounds") =>
                {
                    Some(content.to_owned())
                }
                _ => None,
            },
        );
        assert!(reason.is_ok(), "the reason is shown: {reason:?}");
    }
}
