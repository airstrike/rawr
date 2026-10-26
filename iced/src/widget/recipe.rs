//! Recipes: an imported widget value that borrows for iced's view lifetime,
//! kept as the construction that makes it and replayed at each lowering
//! (D558 tier 2).
//!
//! A `PaneGrid<'a, ..>` borrows the `pane_grid::State` the app's State
//! holds, so it can't be made once and kept: it lives only as long as the
//! view that borrows the home. A [`Recipe`] is the function of the
//! lowering [`Context`] that makes it, generic over that lifetime through a
//! [`Family`], as D321's `Replay<T>` is for `'static` widgets. A builder
//! call on the value (`.spacing(10)`) extends the recipe, and the value
//! reaches a view as a [`Descriptor`] that runs the recipe when it lowers.
//!
//! A callback the native constructor calls while it builds the value, as
//! `PaneGrid::new` calls its pane closure, runs at the transition that
//! makes the recipe, never while lowering (D546.2). What it returns is
//! owned (a descriptor or another recipe), and the replayed constructor is
//! given a native closure that hands back what each call returned.

use std::any::Any;
use std::sync::{Arc, OnceLock};

use rawr::Dynamic;

use super::{Context, Descriptor, Identity};
use crate::runtime;

/// A type with a lifetime parameter, named without it: `Of<'a>` is the
/// type at `'a`.
pub trait Family: 'static {
    /// The type at the view lifetime `'a`.
    type Of<'a>;
}

/// The function of the lowering context that makes a value of `F` at
/// whatever view lifetime the context lends.
pub struct Recipe<F: Family>(Arc<Make<F>>);

/// What a [`Recipe`] runs.
type Make<F> = dyn for<'a> Fn(&Context<'a>) -> <F as Family>::Of<'a> + Send + Sync;

impl<F: Family> Clone for Recipe<F> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<F: Family> Recipe<F> {
    /// A recipe that makes its value with `make`.
    pub fn new(make: impl for<'a> Fn(&Context<'a>) -> F::Of<'a> + Send + Sync + 'static) -> Self {
        Self(Arc::new(make))
    }

    /// The value at the lifetime `context` lends.
    pub fn run<'a>(&self, context: &Context<'a>) -> F::Of<'a> {
        (self.0)(context)
    }

    /// This recipe followed by `step`, as a builder call on its value: the
    /// value of `G` that `step` makes from this one.
    pub fn then<G: Family>(
        self,
        step: impl for<'a> Fn(F::Of<'a>, &Context<'a>) -> G::Of<'a> + Send + Sync + 'static,
    ) -> Recipe<G> {
        Recipe::new(move |context| step(self.run(context), context))
    }
}

/// iced's element of Rawr messages, as a family.
pub struct ElementFamily;

impl Family for ElementFamily {
    type Of<'a> = iced::Element<'a, runtime::Message>;
}

/// A recipe that makes an element, as a view's widget.
pub struct Widget {
    recipe: Recipe<ElementFamily>,
    identity: Identity,
}

impl Widget {
    /// The widget `recipe` makes.
    pub fn new(recipe: Recipe<ElementFamily>) -> Self {
        Self {
            recipe,
            identity: Identity::unique(),
        }
    }
}

impl Descriptor for Widget {
    fn lower<'a>(&self, context: &Context<'a>) -> iced::Element<'a, runtime::Message> {
        self.recipe.run(context)
    }

    fn identity(&self) -> Identity {
        self.identity.clone()
    }
}

/// An imported view value as the runner's arena holds it: its recipe, and,
/// for a type iced implements `Widget` for, the recipe of the element it
/// shows as. Copies share the recipe.
#[derive(Clone)]
pub struct View {
    recipe: Arc<dyn Any + Send + Sync>,
    element: Option<Recipe<ElementFamily>>,
}

impl View {
    /// A view value that is part of a widget, as pane_grid's `Content`.
    pub fn part<F: Family>(recipe: Recipe<F>) -> Self {
        Self {
            recipe: Arc::new(recipe),
            element: None,
        }
    }

    /// A view value iced shows as an element, through `element`.
    pub fn widget<F: Family>(
        recipe: Recipe<F>,
        element: impl FnOnce(Recipe<F>) -> Recipe<ElementFamily>,
    ) -> Self {
        Self {
            element: Some(element(recipe.clone())),
            recipe: Arc::new(recipe),
        }
    }

    /// The recipe of `F` this value holds.
    pub fn recipe<F: Family>(&self) -> Option<Recipe<F>> {
        self.recipe.downcast_ref::<Recipe<F>>().cloned()
    }
}

/// A native value a recipe borrows at each lowering, as `PaneGrid::new`
/// borrows the `pane_grid::State` (D568): the value itself, which moves
/// from the runner's arena into the application's home the first time a
/// view borrows it and stays there. The lowering context lends the home
/// for iced's view lifetime, and the arena's slot reaches the value in the
/// home for the State's other uses, as `update` changing it in place.
pub struct Native<T> {
    handle: crate::home::Handle,
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Clone for Native<T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            marker: std::marker::PhantomData,
        }
    }
}

impl<T: Any + Send> Native<T> {
    /// `value`, kept in the home of the transition running now.
    pub fn keep(value: T) -> Result<Self, String> {
        crate::home::insert(value)
            .map(|handle| Self {
                handle,
                marker: std::marker::PhantomData,
            })
            .map_err(|error| error.to_string())
    }

    /// The value, borrowed for the view lifetime `context` lends.
    pub fn get<'a>(&self, context: &Context<'a>) -> Option<&'a T> {
        context.home()?.get::<T>(&self.handle).ok()
    }

    /// `f` of the value, in the home of the transition running now or the
    /// one the running call borrows.
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> Result<R, String> {
        crate::home::get::<T, R>(&self.handle, f).map_err(|error| error.to_string())
    }

    /// `f` of the value changed in place, which only a transition may: a
    /// call a view runs borrows the home shared (E0502).
    pub fn write<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, String> {
        crate::home::with_mut::<T, R>(&self.handle, f).map_err(|error| error.to_string())
    }
}

/// A context for a view's dry run at its transition (D546.2): a native
/// constructor runs once there so the callbacks it calls run then, and
/// what it makes is dropped. It lends no home and maps no message.
pub fn dry_context() -> Context<'static> {
    let instance = rawr::application::Instance::new();
    let generation = rawr::program::GenerationId::detached();
    Context::unavailable(
        instance,
        generation,
        &Arc::new(super::diagnostic::Sink::new(instance, generation)),
        &crate::theme::Value::default(),
    )
}

/// The message a view value's event callback gives, stamped for the
/// application `context` lowers for, as iced's `on_click(Message::Clicked)`
/// gives one.
pub fn message(context: &Context<'_>, payload: Dynamic) -> runtime::Message {
    context.message(payload)
}

/// How rawr-iced finds the view value a script value names in the runner's
/// arena: the runner's generated code installs it.
static VIEWS: OnceLock<fn(&Dynamic) -> Option<View>> = OnceLock::new();

/// Install the runner's lookup of a view value by the script value naming
/// it. The first install holds; every runner's lookup is the same code.
pub fn install(views: fn(&Dynamic) -> Option<View>) {
    let _ = VIEWS.set(views);
}

/// `iced::widget::recipe::element(view)`: the element an imported view
/// value shows as, which the compiler calls where a view takes it as an
/// `Element`.
pub(crate) fn bind(builder: &mut rawr::host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::recipe::element", |call, args| {
            let [view] = args else {
                return Err(super::builtin::arity("recipe::element", 1, args.len()));
            };
            let element = VIEWS
                .get()
                .and_then(|views| views(view))
                .and_then(|view| view.element)
                .ok_or_else(|| {
                    super::builtin::runtime_error(format!(
                        "`{}` is no view value iced shows as an element",
                        view.type_name()
                    ))
                })?;
            super::Value::from_factory(Widget::new(element), call)
                .map(Dynamic::from)
                .map_err(super::builtin::witness_error)
        })
        .expect("the iced facade must retain recipe::element");
}

/// D558's check (c): what the importer generates for iced's pane_grid
/// example, written by hand against the pinned iced. The home holds the
/// `pane_grid::State` of script values, the pane closure runs at the view's
/// transition and gives each pane's `Content` recipe with a `TitleBar`, and
/// the grid lowers to an `Element<'a>` that borrows the State, with no
/// unsafe code.
#[cfg(test)]
mod check {
    use std::collections::HashMap;
    use std::sync::Arc;

    use iced::advanced::Widget as _;
    use iced::widget::pane_grid;
    use rawr::Dynamic;

    use super::{ElementFamily, Family, Recipe, Widget};
    use crate::home::{self, Home};
    use crate::runtime;
    use crate::widget::{self, Context, Descriptor};

    type Element<'a> = iced::Element<'a, runtime::Message>;
    type State = pane_grid::State<Dynamic>;

    struct PaneGridFamily;
    impl Family for PaneGridFamily {
        type Of<'a> = pane_grid::PaneGrid<'a, runtime::Message, Element<'a>, Element<'a>>;
    }

    struct ContentFamily;
    impl Family for ContentFamily {
        type Of<'a> = pane_grid::Content<'a, runtime::Message, Element<'a>, Element<'a>>;
    }

    struct TitleBarFamily;
    impl Family for TitleBarFamily {
        type Of<'a> = pane_grid::TitleBar<'a, runtime::Message, Element<'a>>;
    }

    /// `pane_grid::TitleBar::new(title)`, where `title` is a view's widget.
    fn title_bar_new(title: widget::Value) -> Recipe<TitleBarFamily> {
        Recipe::new(move |context| pane_grid::TitleBar::new(title.lower(context)))
    }

    /// `TitleBar::controls(pane_grid::Controls::new(controls))`.
    fn title_bar_controls(
        bar: Recipe<TitleBarFamily>,
        controls: widget::Value,
    ) -> Recipe<TitleBarFamily> {
        bar.then::<TitleBarFamily>(move |bar, context| {
            bar.controls(pane_grid::Controls::new(controls.lower(context)))
        })
    }

    /// `pane_grid::Content::new(body).title_bar(bar)`.
    fn content_new(body: widget::Value, bar: Recipe<TitleBarFamily>) -> Recipe<ContentFamily> {
        Recipe::new(move |context| {
            pane_grid::Content::new(body.lower(context)).title_bar(bar.run(context))
        })
    }

    /// `PaneGrid::new(&state, view)`, run in a transition: `view` is the
    /// script's pane closure, called here for every pane, and the recipe
    /// hands each replayed call the content it returned.
    fn pane_grid_new(
        state: &home::Handle,
        view: impl Fn(pane_grid::Pane, &Dynamic, bool) -> Recipe<ContentFamily>,
    ) -> Result<Recipe<PaneGridFamily>, home::Error> {
        let contents = home::get::<State, _>(state, |panes| {
            panes
                .iter()
                .map(|(pane, value)| {
                    let maximized = panes.maximized() == Some(*pane);
                    ((*pane, maximized), view(*pane, value, maximized))
                })
                .collect::<HashMap<_, _>>()
        })?;
        let state = state.clone();
        Ok(Recipe::new(move |context: &Context<'_>| {
            let panes = context
                .home()
                .and_then(|home| home.get::<State>(&state).ok())
                .expect("the view's home holds the State");
            pane_grid::PaneGrid::new(panes, |pane, _, maximized| {
                match contents.get(&(pane, maximized)) {
                    Some(content) => content.run(context),
                    None => pane_grid::Content::new(widget::failure("no content")).title_bar(
                        pane_grid::TitleBar::new(widget::failure("this pane is new")),
                    ),
                }
            })
        }))
    }

    fn text(content: &str) -> widget::Value {
        widget::diagnostic(content)
    }

    #[test]
    fn a_pane_grid_recipe_borrows_the_homes_state_and_replays_its_contents() {
        let mut home = Home::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let grid = home::enter(&mut home, || {
            let (mut panes, first) = State::new(Dynamic::from(0_i64));
            panes
                .split(pane_grid::Axis::Vertical, first, Dynamic::from(1_i64))
                .expect("the first pane splits");
            let state = home::insert(panes).expect("a transition has a home");
            let calls = Arc::clone(&calls);
            let grid = pane_grid_new(&state, move |_, value, _| {
                calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let label = format!("Pane {}", value.as_int().unwrap_or(-1));
                let bar = title_bar_controls(title_bar_new(text(&label)), text("Close"));
                content_new(text("body"), bar)
            })
            .expect("the State is in the home");
            let grid = grid.then::<ElementFamily>(|grid, _| grid.spacing(10)._boxed());
            (state, Widget::new(grid))
        });
        let (_state, grid) = grid;
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 2);

        let mut engine = rawr::Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = rawr::Shared::new(engine);
        let candidate = engine
            .compile_application(
                r#"
use iced::widget::text;
use iced::Element;
struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("grid").boxed() }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#,
            )
            .unwrap();
        let launch = engine
            .launch_application(candidate.validate().unwrap())
            .unwrap();
        let mut registry = rawr::host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = Arc::new(launch.bind(registry).unwrap());
        let instance = rawr::application::Instance::new();
        let sink = Arc::new(widget::diagnostic::Sink::new(instance, launch.generation()));
        let theme = crate::theme::Value::default();
        let context = Context::application(&launch, instance, &sink, &theme).with_home(&home);
        for _ in 0..2 {
            let mut element = grid.lower(&context);
            let mut tree = iced::advanced::widget::Tree::new(&element);
            element.diff(&mut tree);
            assert_eq!(tree.children.len(), 2, "one tree per pane's content");
            let renderer = iced::futures::executor::block_on(
                <iced::Renderer as iced::advanced::renderer::Headless>::new(
                    iced::advanced::renderer::Settings::default(),
                    Some("tiny-skia"),
                ),
            )
            .expect("the pinned tiny-skia renderer must be available");
            let size = iced::Size::new(400.0, 300.0);
            element.layout(
                &mut tree,
                &renderer,
                &iced::advanced::layout::Limits::new(size, size),
            );
        }
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "lowering runs no script"
        );
    }
}
