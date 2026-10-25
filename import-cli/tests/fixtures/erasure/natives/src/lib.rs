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
