//! A trait, values of it held as `&'static dyn` and `Box<dyn>`, a function
//! taking `&dyn`, and tracing's callsite shape: metadata that holds its
//! callsite as an `Identifier(&'static dyn Site)`, which a macro expanded
//! at the call builds from the caller's own `__CALLSITE` static, as
//! tracing's `identify_callsite!` does.

pub trait Shape: Sync {
    fn area(&self) -> i64;
}

pub struct Square {
    pub side: i64,
}

impl Shape for Square {
    fn area(&self) -> i64 {
        self.side * self.side
    }
}

impl Shape for i64 {
    fn area(&self) -> i64 {
        *self
    }
}

impl Shape for u8 {
    fn area(&self) -> i64 {
        i64::from(*self) + 1000
    }
}

impl Shape for bool {
    fn area(&self) -> i64 {
        i64::from(*self)
    }
}

/// A shape kept for the program's life.
pub struct Held(pub &'static dyn Shape);

impl Held {
    pub fn area(&self) -> i64 {
        self.0.area()
    }

    /// Whether it holds the shape at `shape`'s address.
    pub fn holds(&self, shape: &'static Square) -> bool {
        std::ptr::addr_eq(self.0, shape)
    }
}

pub fn area_of(shape: &dyn Shape) -> i64 {
    shape.area()
}

pub fn boxed(side: i64) -> Box<dyn Shape> {
    Box::new(Square { side })
}

pub fn boxed_area(shape: Box<dyn Shape>) -> i64 {
    shape.area()
}

/// A shape shared by count, and how many hold it.
pub fn counted_area(shape: std::rc::Rc<dyn Shape>) -> (i64, usize) {
    (shape.area(), std::rc::Rc::strong_count(&shape))
}

pub fn shared_area(shape: std::sync::Arc<dyn Shape>) -> i64 {
    shape.area()
}

/// What metadata points back at.
pub trait Site: Sync {
    fn name(&self) -> &'static str;
}

/// A callsite's identity, as tracing-core's `Identifier`.
pub struct Identifier(pub &'static dyn Site);

pub struct Metadata {
    name: &'static str,
    site: Identifier,
}

impl Metadata {
    pub const fn new(name: &'static str, site: Identifier) -> Self {
        Self { name, site }
    }
}

pub struct Callsite {
    meta: &'static Metadata,
}

impl Callsite {
    pub const fn new(meta: &'static Metadata) -> Self {
        Self { meta }
    }

    /// Its metadata's name, and whether its metadata points back at it.
    pub fn describe(&'static self) -> String {
        let back = std::ptr::addr_eq(self.meta.site.0, self);
        format!("{} {} back {back}", self.meta.name, self.meta.site.0.name())
    }
}

impl Site for Callsite {
    fn name(&self) -> &'static str {
        self.meta.name
    }
}

/// tracing's `event!`: a `__CALLSITE` static whose metadata names it.
#[macro_export]
macro_rules! event {
    ($name:expr) => {{
        static __CALLSITE: $crate::Callsite = {
            static META: $crate::Metadata =
                $crate::Metadata::new($name, $crate::Identifier(&__CALLSITE));
            $crate::Callsite::new(&META)
        };
        __CALLSITE.describe()
    }};
}
