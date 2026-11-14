//! A program's value coerced to a trait object of a crate's trait (D872),
//! as `&count as &dyn tracing::field::Value` is.
//!
//! Rawr never makes the object. The value stays the program's, tagged with
//! the type it was coerced from, and the runner's adapter coerces it where
//! native code takes the object, as rustc coerces it: it finds the tag
//! among the crate's impls of the trait and makes that type's Rust value,
//! or reaches the crate value where it is. A program's function coerced
//! to a `&'static dyn Trait` is the one object the adapter made of it,
//! which [`lasting`] keeps.

use crate::{Dynamic, ImmutableString};

/// A program's value where a crate's trait object is.
#[derive(Clone)]
pub struct Coerced {
    value: Dynamic,
    /// The type it was coerced from, as the importer tags it
    /// (`rawr_import::classify::representability::object::tag`).
    tag: ImmutableString,
    /// A callable's type, which attests what native code gives it.
    witness: Option<crate::host::runtime::Witness>,
}

impl Coerced {
    /// `value`, coerced from the type `tag` names.
    pub(crate) fn new(value: Dynamic, tag: ImmutableString) -> Self {
        Self {
            value,
            tag,
            witness: None,
        }
    }

    /// The program's callable `value`, coerced through the crate's impl of
    /// the trait for every callable, as `tag` names it, with the witness
    /// of its type (#1981).
    pub(crate) fn called(
        value: Dynamic,
        tag: ImmutableString,
        witness: Option<crate::host::runtime::Witness>,
    ) -> Self {
        Self {
            value,
            tag,
            witness,
        }
    }

    /// The witness of each of the `count` inputs of the callable `value`
    /// holds, where it has one: what a native value native code gives it
    /// is attested as.
    #[must_use]
    pub fn witnesses(value: &Dynamic, count: usize) -> Vec<Option<crate::host::runtime::Witness>> {
        let witness = value
            .read_lock::<Self>()
            .and_then(|coerced| coerced.witness.clone());
        (0..count)
            .map(|input| {
                witness
                    .as_ref()
                    .and_then(|witness| witness.argument_witness(input))
            })
            .collect()
    }

    /// The value and its tag, when `value` is a coerced one; `None` for an
    /// object native code gave.
    #[must_use]
    pub fn of(value: &Dynamic) -> Option<(Dynamic, ImmutableString)> {
        value
            .read_lock::<Self>()
            .map(|coerced| (coerced.value.clone(), coerced.tag.clone()))
    }
}

thread_local! {
    /// Each object [`lasting`] kept, by its type and the function it calls.
    static LASTING: std::cell::RefCell<
        std::collections::HashMap<(std::any::TypeId, crate::item::Value), Box<dyn std::any::Any>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The object of type `T` that the adapter's `make` makes of the program's
/// function `value`, kept for the runner's life, as rustc promotes a borrow
/// of a function, or of a closure that captures nothing, to a static:
/// `&Code::highlight` is a `&'static dyn Highlighter<Code>` (#1981). The
/// same function gives the same object each time. `None` for a closure
/// that captures, which lives in no static.
pub fn lasting<T: ?Sized + 'static, E>(
    value: &Dynamic,
    make: impl FnOnce() -> Result<Box<T>, E>,
) -> Option<Result<&'static T, E>> {
    let function = value.read_lock::<crate::item::Value>()?.clone();
    if !function.captures().is_empty() {
        return None;
    }
    let key = (std::any::TypeId::of::<T>(), function);
    let kept = LASTING.with_borrow(|kept| {
        kept.get(&key)
            .and_then(|object| object.downcast_ref::<&'static T>().copied())
    });
    if let Some(object) = kept {
        return Some(Ok(object));
    }
    let object: &'static T = match make() {
        Ok(made) => Box::leak(made),
        Err(error) => return Some(Err(error)),
    };
    LASTING.with_borrow_mut(|kept| kept.insert(key, Box::new(object)));
    Some(Ok(object))
}
