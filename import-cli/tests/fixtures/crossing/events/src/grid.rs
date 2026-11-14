//! A buffer made by calling a closure for each cell, as image's
//! `ImageBuffer::from_fn(width, height, |x, y| ..)` takes an `FnMut` with
//! no `'static` bound, and function pointers of any arity.

/// A grid of cells, as image's `ImageBuffer`.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    width: u32,
    height: u32,
    cells: Vec<u8>,
}

impl Grid {
    /// A `width` by `height` grid whose cell at `(x, y)` is `f(x, y)`,
    /// called once per cell, row by row.
    pub fn from_fn<F>(width: u32, height: u32, mut f: F) -> Grid
    where
        F: FnMut(u32, u32) -> u8,
    {
        let mut cells = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                cells.push(f(x, y));
            }
        }
        Grid {
            width,
            height,
            cells,
        }
    }

    /// Each cell replaced by `f` of it, through `impl FnMut`.
    pub fn map(mut self, mut f: impl FnMut(u8) -> u8) -> Grid {
        for cell in &mut self.cells {
            *cell = f(*cell);
        }
        self
    }

    /// The cell at `(x, y)`.
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.cells[(y * self.width + x) as usize]
    }

    /// The cells folded by `f`, a function pointer of two inputs.
    pub fn fold(&self, start: u32, f: fn(u32, u8) -> u32) -> u32 {
        self.cells.iter().fold(start, |sum, cell| f(sum, *cell))
    }

    /// Its size, as `width x height`.
    pub fn size(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }
}

/// `f` of `a`, `b` and `c`, a function pointer of three inputs.
pub fn combine(a: i64, b: i64, c: i64, f: fn(i64, i64, i64) -> i64) -> i64 {
    f(a, b, c)
}

/// `f` of nothing, a function pointer of no input.
pub fn produce(f: fn() -> i64) -> i64 {
    f()
}

/// `label` of `grid`, a function pointer that borrows its input, which
/// rustc reads as `for<'a> fn(&'a Grid) -> String` (#966).
pub fn labeled(grid: &Grid, label: fn(&Grid) -> String) -> String {
    label(grid)
}

/// The text of a borrowed tuple that holds borrows, as log's `&(&str,
/// &'static str, &'static Location)` takes (#1559).
pub fn tagged(tag: &(&str, &'static str, u32)) -> String {
    let &(target, module, line) = tag;
    format!("{target} {module}:{line}")
}

/// Where it is called from, as log's `__private_api::loc()` gives (#1560).
#[track_caller]
pub fn here() -> &'static std::panic::Location<'static> {
    std::panic::Location::caller()
}

/// The place a borrowed tuple's location names, as log's `log` reads the
/// `&'static Location` beside its target (#1560).
pub fn placed(tag: &(&str, &'static std::panic::Location<'static>)) -> String {
    let &(target, location) = tag;
    format!(
        "{target} {}:{}:{}",
        location.file(),
        location.line(),
        location.column()
    )
}

/// A descriptor generic over its label, as wgpu-types'
/// `DeviceDescriptor<L>` is.
pub struct Described<L> {
    pub label: L,
    pub count: u32,
}

/// A descriptor whose label borrows, as wgpu's `type DeviceDescriptor<'a>
/// = wgt::DeviceDescriptor<Option<&'a str>>` is.
pub type Descriptor<'a> = Described<Option<&'a str>>;

/// A descriptor labeled `label`.
pub fn described(label: Option<&'static str>, count: u32) -> Descriptor<'static> {
    Described { label, count }
}

/// The count and label length of a descriptor whose label borrows inside
/// a type argument, as wgpu's `request_device(&self, desc:
/// &DeviceDescriptor<'_>)` takes (#1953).
pub fn requested(descriptor: &Descriptor<'_>) -> u32 {
    descriptor.count + descriptor.label.map_or(0, |label| label.len() as u32)
}

/// A builder whose key is held by a borrow, as iced's `Sensor<'a, Key,
/// ..>` is.
pub struct Keyed<'a, K> {
    key: K,
    width: u32,
    marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Keyed<'a, ()> {
    /// An unkeyed builder `width` wide.
    pub fn new(width: u32) -> Self {
        Keyed {
            key: (),
            width,
            marker: std::marker::PhantomData,
        }
    }
}

impl<'a, K> Keyed<'a, K> {
    /// The builder keyed by a borrow of a `?Sized` key, as iced's
    /// `Sensor::key_ref<K>(self, key: &'a K) -> Sensor<'a, &'a K, ..>`
    /// takes a `&str` (#801).
    pub fn key_ref<T>(self, key: &'a T) -> Keyed<'a, &'a T>
    where
        T: ToOwned + PartialEq<T::Owned> + ?Sized,
        T::Owned: 'static,
    {
        Keyed {
            key,
            width: self.width,
            marker: std::marker::PhantomData,
        }
    }

    /// The builder `width` wide.
    pub fn width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }
}

impl<K: std::fmt::Debug> Keyed<'_, K> {
    /// Its key and width.
    pub fn shown(&self) -> String {
        format!("{:?} {}", self.key, self.width)
    }
}

/// A surface a window lends, as iced's `Window` is.
pub trait Surface {
    /// The surface's name.
    fn name(&self) -> String;
}

/// The surface `with_surface` lends.
pub struct Screen {
    width: u32,
}

impl Surface for Screen {
    fn name(&self) -> String {
        format!("screen {}", self.width)
    }
}

/// `f` of a surface lent for the call, as iced's `window::run(id, f: impl
/// FnOnce(&dyn Window) -> T)` lends the window (#28).
pub fn with_surface<T>(f: impl FnOnce(&dyn Surface) -> T) -> T {
    let screen = Screen { width: 640 };
    f(&screen)
}

/// The name of a surface, as rfd's `set_parent(&W)` reads a window.
pub fn surface_name<S: Surface + ?Sized>(surface: &S) -> String {
    surface.name()
}

/// A scale a builder borrows.
pub struct Scale {
    factor: u32,
}

impl Scale {
    /// A scale of `factor`.
    pub fn new(factor: u32) -> Self {
        Scale { factor }
    }
}

/// A scale in a static, as loading_spinners' `easing::STANDARD` is.
pub static DOUBLE: Scale = Scale { factor: 2 };

/// A builder that keeps a borrowed scale in `Self`, as loading_spinners'
/// `Linear<'a>` keeps its `&'a Easing`.
pub struct Scaled<'a> {
    scale: &'a Scale,
    value: u32,
}

impl<'a> Scaled<'a> {
    /// `value` at the unit scale.
    pub fn new(value: u32) -> Self {
        static UNIT: Scale = Scale { factor: 1 };
        Scaled {
            scale: &UNIT,
            value,
        }
    }

    /// The builder at `scale`, kept in `Self`, as `Linear::easing(mut
    /// self, easing: &'a Easing) -> Self` keeps its easing (#610).
    pub fn scale(mut self, scale: &'a Scale) -> Self {
        self.scale = scale;
        self
    }

    /// The scaled value.
    pub fn shown(&self) -> u32 {
        self.value * self.scale.factor
    }
}

/// A palette a styler reads, as iced's `Theme` is.
#[derive(Clone)]
pub struct Palette {
    base: u32,
}

impl Palette {
    /// The palette's base style.
    pub fn base(&self) -> u32 {
        self.base
    }
}

/// A tone a styler styles, as iced's `Code` is.
#[derive(Clone, Copy, Debug)]
pub enum Tone {
    Light,
    Dark,
}

impl Tone {
    /// The tone's style in `palette`, as iced's `Code::highlight(self,
    /// &Theme)` is.
    pub fn shade(self, palette: &Palette) -> u32 {
        match self {
            Tone::Light => palette.base + 1,
            Tone::Dark => palette.base + 9,
        }
    }
}

/// What styles an input in a palette, as iced's `text::Highlighter<Input,
/// Theme = Theme>` does.
pub trait Styler<Input, P = Palette> {
    /// The style of `input` in `palette`.
    fn style(&self, input: Input, palette: &P) -> u32;
}

/// Every function of an input and a palette is a styler, as iced's
/// `Highlighter` is implemented for `Fn(Input, &Theme) -> Style`.
impl<T, Input, P> Styler<Input, P> for T
where
    T: Fn(Input, &P) -> u32,
{
    fn style(&self, input: Input, palette: &P) -> u32 {
        (self)(input, palette)
    }
}

/// The style `styler` gives `input` in the standard palette, as iced's
/// markdown takes a `&dyn Highlighter<Code>`.
pub fn styled(styler: &dyn Styler<u8>, input: u8) -> u32 {
    styler.style(input, &Palette { base: 100 })
}

/// The style a boxed `styler` gives `input` in the standard palette.
pub fn boxed_style(styler: Box<dyn Styler<u8>>, input: u8) -> u32 {
    styler.style(input, &Palette { base: 100 })
}

/// A view that names its styler, as iced's markdown `Viewer::highlighter`
/// does.
pub trait Styled {
    /// The styler of each tone.
    fn styler(&self) -> &dyn Styler<Tone>;
}

/// Each tone's style by the styler `styled` names.
pub fn styles<S: Styled>(styled: &S) -> String {
    let palette = Palette { base: 10 };
    let styler = styled.styler();
    format!(
        "{} {}",
        styler.style(Tone::Light, &palette),
        styler.style(Tone::Dark, &palette)
    )
}
