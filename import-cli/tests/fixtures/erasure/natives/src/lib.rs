//! Generic native functions a script type fills as `rawr::Value`, each
//! observing its argument the way the review's findings name: a `TypeId`
//! cast, a closure fed a cast value, `Clone` and `Debug` bounds, and a
//! value passed through whole.

use std::any::Any;

/// `a` as a `B`, when they are one type.
pub fn cast<A: 'static, B: 'static>(a: A) -> Option<B> {
    let boxed: Box<dyn Any> = Box::new(a);
    boxed.downcast::<B>().ok().map(|b| *b)
}

/// `f` of `a` as a `B`, when they are one type, and -1 otherwise.
pub fn apply<A: 'static, B: 'static, F: FnOnce(B) -> i64>(a: A, f: F) -> i64 {
    let boxed: Box<dyn Any> = Box::new(a);
    match boxed.downcast::<B>() {
        Ok(b) => f(*b),
        Err(_) => -1,
    }
}

pub fn duplicate<T: Clone>(value: T) -> (T, T) {
    (value.clone(), value)
}

pub fn show<T: std::fmt::Debug>(value: T) -> String {
    format!("{value:?}")
}

pub fn identity<T>(value: T) -> T {
    value
}

/// A value that is not `Clone`.
pub struct Resource {
    id: i64,
}

impl Resource {
    pub fn new(id: i64) -> Self {
        Self { id }
    }

    pub fn id(&self) -> i64 {
        self.id
    }
}

/// A trait a script's `-> impl Labelled` returns, as Freya's `fn app() ->
/// impl IntoElement` does.
pub trait Labelled {
    fn label(&self) -> String;
}

impl Labelled for Resource {
    fn label(&self) -> String {
        format!("resource {}", self.id)
    }
}

/// rand's shape: `R: SampleRange<T>` with an impl for `Range<T>`, so the
/// call's `T` is the range's own.
pub trait SampleRange<T> {
    fn low(self) -> T;
}

impl<T> SampleRange<T> for std::ops::Range<T> {
    fn low(self) -> T {
        self.start
    }
}

/// The range's start, of the type the range holds.
pub fn pick<T, R: SampleRange<T>>(range: R) -> T {
    range.low()
}

/// Whether this runs on the process's main thread, where winit's event
/// loop must run.
#[cfg(target_os = "macos")]
pub fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn pthread_main_np() -> i32;
    }
    // SAFETY: it only reads which thread calls it.
    unsafe { pthread_main_np() == 1 }
}

/// Whether this runs on the process's main thread, where winit's event
/// loop must run.
#[cfg(not(target_os = "macos"))]
pub fn on_main_thread() -> bool {
    unsafe extern "C" {
        fn getpid() -> i32;
        fn gettid() -> i32;
    }
    // SAFETY: both only read the calling process's and thread's ids.
    unsafe { getpid() == gettid() }
}

/// A native panic, as winit's when its event loop is made off the main
/// thread.
pub fn fail(message: String) -> i64 {
    panic!("{message}")
}

// ---- Freya shapes (D551.6) ----

/// Freya's `VirtualScrollView<D, B>`: a builder whose type names its
/// callable parameter, with a method returning `Self`.
pub struct Lazy<B: Fn() -> i64> {
    build: B,
    flag: bool,
}

impl<B: Fn() -> i64> Lazy<B> {
    pub fn new(build: B) -> Self {
        Self { build, flag: false }
    }

    pub fn flag(mut self, flag: bool) -> Self {
        self.flag = flag;
        self
    }

    pub fn run(&self) -> i64 {
        (self.build)() + i64::from(self.flag)
    }
}

/// An element, as Freya's `Element`.
pub struct Element {
    pub text: String,
    presses: Vec<EventHandler<Event<PressEventData>>>,
}

impl Element {
    fn text(text: String) -> Self {
        Self {
            text,
            presses: Vec::new(),
        }
    }
}

/// What Freya's press gives its handler.
#[derive(Debug)]
pub struct PressEventData {
    pub clicks: i64,
}

/// Freya's `Event<D>`.
pub struct Event<D> {
    pub data: D,
}

/// Freya's `EventHandler<T>`, made from any `FnMut(T) + 'static`.
pub struct EventHandler<T>(std::rc::Rc<std::cell::RefCell<dyn FnMut(T)>>);

impl<T> Clone for EventHandler<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> EventHandler<T> {
    fn call(&self, data: T) {
        (self.0.borrow_mut())(data)
    }
}

impl<H: FnMut(D) + 'static, D> From<H> for EventHandler<D> {
    fn from(handler: H) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(handler)))
    }
}

/// Freya's `EventHandlersExt::on_press`, a default method that takes
/// `impl Into<EventHandler<Event<PressEventData>>>`.
pub trait EventHandlersExt: Sized {
    fn get_presses(&mut self) -> &mut Vec<EventHandler<Event<PressEventData>>>;

    fn on_press(mut self, on_press: impl Into<EventHandler<Event<PressEventData>>>) -> Self {
        self.get_presses().push(on_press.into());
        self
    }
}

/// A count native code keeps, as Freya's `State` does.
#[derive(Clone, Default)]
pub struct Count(std::rc::Rc<std::cell::Cell<i64>>);

impl Count {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, by: i64) {
        self.0.set(self.0.get() + by);
    }

    pub fn get(&self) -> i64 {
        self.0.get()
    }
}

/// Freya's `IntoElement`.
pub trait IntoElement {
    fn into_element(self) -> Element;
}

impl IntoElement for &str {
    fn into_element(self) -> Element {
        Element::text(self.to_owned())
    }
}

impl IntoElement for String {
    fn into_element(self) -> Element {
        Element::text(self)
    }
}

/// Freya's `LayoutExt`: what the extension traits build on.
pub trait LayoutExt: Sized {
    fn get_layout(&mut self) -> &mut Layout;
}

#[derive(Default, Clone)]
pub struct Layout {
    pub width: i64,
    pub centered: bool,
}

/// Freya's `ContainerExt` and `ContainerSizeExt`: a blanket impl over a
/// marker trait, with default methods returning `Self`.
pub trait ContainerExt: LayoutExt {}

pub trait ContainerSizeExt: LayoutExt {
    fn expanded(mut self) -> Self {
        self.get_layout().width = 100;
        self
    }
}

impl<T: ContainerExt> ContainerSizeExt for T {}

/// Freya's `ContainerWithContentExt`: a default method a concrete impl
/// keeps.
pub trait ContainerWithContentExt: LayoutExt {
    fn center(mut self) -> Self {
        self.get_layout().centered = true;
        self
    }
}

/// Freya's `ChildrenExt`: a generic default method.
pub trait ChildrenExt: Sized {
    fn get_children(&mut self) -> &mut Vec<Element>;

    fn child<C: IntoElement>(mut self, child: C) -> Self {
        self.get_children().push(child.into_element());
        self
    }
}

#[derive(Default)]
pub struct Rect {
    layout: Layout,
    children: Vec<Element>,
    presses: Vec<EventHandler<Event<PressEventData>>>,
}

pub fn rect() -> Rect {
    Rect::default()
}

impl Rect {
    pub fn describe(&self) -> String {
        let texts = self
            .children
            .iter()
            .map(|child| child.text.as_str())
            .collect::<Vec<_>>();
        format!(
            "rect {} {} [{}]",
            self.layout.width,
            self.layout.centered,
            texts.join(", ")
        )
    }
}

impl LayoutExt for Rect {
    fn get_layout(&mut self) -> &mut Layout {
        &mut self.layout
    }
}

impl ContainerExt for Rect {}
impl ContainerWithContentExt for Rect {}

impl ChildrenExt for Rect {
    fn get_children(&mut self) -> &mut Vec<Element> {
        &mut self.children
    }
}

impl EventHandlersExt for Rect {
    fn get_presses(&mut self) -> &mut Vec<EventHandler<Event<PressEventData>>> {
        &mut self.presses
    }
}

impl IntoElement for Rect {
    fn into_element(mut self) -> Element {
        Element {
            text: self.describe(),
            presses: std::mem::take(&mut self.presses),
        }
    }
}

/// Freya's `AppComponent`, made from any `Fn() -> impl IntoElement`.
pub struct AppComponent {
    render: Box<dyn Fn() -> Element>,
}

impl<F, E> From<F> for AppComponent
where
    F: Fn() -> E + 'static,
    E: IntoElement,
{
    fn from(render: F) -> Self {
        AppComponent {
            render: Box::new(move || render().into_element()),
        }
    }
}

/// Freya's `WindowConfig::new(impl Into<AppComponent>)`.
pub struct WindowConfig {
    app: AppComponent,
}

impl WindowConfig {
    pub fn new(app: impl Into<AppComponent>) -> Self {
        Self { app: app.into() }
    }

    /// Render the app `times` times, as an event loop would.
    pub fn launch(self, times: i64) -> String {
        (0..times)
            .map(|_| (self.app.render)().text)
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Render the app `times` times and press it after each, as an event
    /// loop would on a click.
    pub fn click(self, times: i64) -> String {
        (1..=times)
            .map(|clicks| {
                let element = (self.app.render)();
                for press in &element.presses {
                    press.call(Event {
                        data: PressEventData { clicks },
                    });
                }
                element.text
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}
