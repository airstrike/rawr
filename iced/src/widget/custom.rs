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
        let (result, _) = self.call(SIZE, &mut [self.value.clone()]);
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
        let (result, _) = self.call(TAG, &mut [self.value.clone()]);
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
        let (result, _) = self.call(STATE, &mut [self.value.clone()]);
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
