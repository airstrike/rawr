//! iced's `ViewFn` shape, as elevato's: a trait `View<'a, P, M>` whose
//! method gives an opaque `impl Widget<M> + 'a`, a blanket impl of it for
//! every `Fn(&'a P, &'a Building) -> W`, and a `run` that takes a
//! higher-ranked `View` and maps and boxes what its `view` gives.

use std::fmt::Debug;
use std::marker::PhantomData;

/// What a view shows.
#[derive(Debug)]
pub struct Building {
    /// Its floors.
    pub floors: u8,
}

/// A widget that sends `M`s.
pub trait Widget<M> {
    /// What it shows.
    fn label(&self) -> String;

    /// The message a press sends.
    fn press(&self) -> Option<M>;

    /// The widget sending what `f` makes of its messages.
    fn map<N, F>(self, f: F) -> Map<Self, F, M>
    where
        Self: Sized,
        F: Fn(M) -> N,
    {
        Map {
            widget: self,
            f,
            message: PhantomData,
        }
    }

    /// The widget as an element.
    fn boxed<'a>(self) -> Element<'a, M>
    where
        Self: Sized + 'a,
    {
        Element(Box::new(self))
    }
}

/// A boxed widget.
pub struct Element<'a, M>(Box<dyn Widget<M> + 'a>);

impl<M> Widget<M> for Element<'_, M> {
    fn label(&self) -> String {
        self.0.label()
    }

    fn press(&self) -> Option<M> {
        self.0.press()
    }
}

/// A widget whose messages a function maps.
pub struct Map<W, F, M> {
    widget: W,
    f: F,
    message: PhantomData<fn(M)>,
}

impl<W, F, M, N> Widget<N> for Map<W, F, M>
where
    W: Widget<M>,
    F: Fn(M) -> N,
{
    fn label(&self) -> String {
        format!("map {}", self.widget.label())
    }

    fn press(&self) -> Option<N> {
        self.widget.press().map(&self.f)
    }
}

/// The floors of a building, as a canvas.
struct Floors<'a, M> {
    building: &'a Building,
    lit: u8,
    on_press: M,
}

impl<M: Clone> Widget<M> for Floors<'_, M> {
    fn label(&self) -> String {
        format!("floors {}/{}", self.lit, self.building.floors)
    }

    fn press(&self) -> Option<M> {
        (self.lit < self.building.floors).then(|| self.on_press.clone())
    }
}

/// The floors of `building`, `lit` of them lit, sending `on_press` while
/// some are dark.
pub fn floors<'a, M: Clone + 'a>(
    building: &'a Building,
    lit: u8,
    on_press: M,
) -> impl Widget<M> + 'a {
    Floors {
        building,
        lit,
        on_press,
    }
}

/// A tower of `floors`, as a canvas that borrows nothing.
struct Tower<M> {
    floors: u8,
    lit: u8,
    on_press: M,
}

impl<M: Clone> Widget<M> for Tower<M> {
    fn label(&self) -> String {
        format!("tower {}/{}", self.lit, self.floors)
    }

    fn press(&self) -> Option<M> {
        (self.lit < self.floors).then(|| self.on_press.clone())
    }
}

/// A tower of `floors`, `lit` of them lit, sending `on_press` while some
/// are dark.
pub fn tower<M: Clone>(floors: u8, lit: u8, on_press: M) -> impl Widget<M> {
    Tower {
        floors,
        lit,
        on_press,
    }
}

/// A view of a program and a building, as iced's `ViewFn`.
pub trait View<'a, P, M> {
    /// The widget `program` shows of `building`.
    fn view(&self, program: &'a P, building: &'a Building) -> impl Widget<M> + 'a;
}

impl<'a, P, M, T, W> View<'a, P, M> for T
where
    P: 'a,
    T: Fn(&'a P, &'a Building) -> W,
    W: Widget<M> + 'a,
{
    fn view(&self, program: &'a P, building: &'a Building) -> impl Widget<M> + 'a {
        self(program, building)
    }
}

/// A message the game sends, holding the program's.
#[derive(Debug, Clone)]
pub struct Input<M>(pub M);

impl<M> Input<M> {
    /// The input holding `message`.
    pub fn new(message: M) -> Self {
        Self(message)
    }
}

/// Shows the program's view of a building three times, pressing it each
/// time, as elevato's game does.
pub fn run<P, M, U, V>(update: U, view: V)
where
    P: Default + 'static,
    M: Debug + Clone + 'static,
    U: Fn(&mut P, M),
    V: for<'a> View<'a, P, M> + Clone + 'static,
{
    let building = Building { floors: 2 };
    let mut program = P::default();
    for event in 1..=3 {
        let element = view.view(&program, &building).map(Input::new).boxed();
        println!("{event} view {}", element.label());
        let pressed = element.press();
        drop(element);
        if let Some(Input(message)) = pressed {
            println!("{event} message {message:?}");
            update(&mut program, message);
        }
    }
}
