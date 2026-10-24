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
use std::sync::{Arc, Mutex};

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
pub(crate) const HOOKS: usize = 8;

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

/// iced's `tree::State`, which the widget returning it from `state` hands
/// to iced once.
#[derive(Clone)]
pub(crate) struct State(Arc<Mutex<Option<tree::State>>>);

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
                return Err(arity("advanced::widget::Instance::new", HOOKS + 2, args.len()));
            }
            let value = args[0].take();
            let hooks = args[1..=HOOKS]
                .iter_mut()
                .map(|hook| take::<item::Value>(hook, "advanced::Widget method"))
                .collect::<Result<Arc<[_]>, _>>()?;
            let written = args[HOOKS + 1]
                .as_int()
                .map_err(|_| runtime_error("`advanced::widget::Instance::new` expects its mask".into()))?;
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
        match result {
            Ok(tag) => tag.try_cast::<tree::Tag>().unwrap_or_else(tree::Tag::stateless),
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
        match result {
            Ok(state) => state
                .try_cast::<State>()
                .and_then(|state| state.0.lock().ok()?.take())
                .unwrap_or(tree::State::None),
            Err(error) => {
                self.fail("state", &error);
                tree::State::None
            }
        }
    }

    fn diff(&mut self, tree: &mut NativeTree) {
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
            self.children.with(draw.child, |child| {
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
            arguments[6] = Dynamic::from(Shell { nonce: loan.nonce() });
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
}

fn subtree<'a>(tree: &'a NativeTree, path: &[usize]) -> Option<&'a NativeTree> {
    path.iter()
        .try_fold(tree, |tree, &index| tree.children.get(index))
}

/// A failed call's message, drawn in the widget's bounds.
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
        assert!(adapter.diagnostics().is_empty(), "{:?}", adapter.diagnostics());
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
        assert_eq!(buffers.vertices[0].position, [corner[0] / 2.0, corner[1] / 2.0]);

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
        if drawn.iter().flatten().any(|command| matches!(command, Command::StartLayer(_))) {
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
}
