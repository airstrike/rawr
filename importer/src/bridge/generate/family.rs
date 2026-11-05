//! A scope family as a runner names it (D558 tier 1): a family struct of
//! `rawr_families`, or what one of its wrapper functions gives (#936).
//!
//! A crate's opaque `impl Trait` type that borrows has no Rust name, as what
//! `element.explain(color)` gives doesn't. The runner declares a function of
//! its own that makes it, `fn rawr_given_0<'x>((receiver, argument_0):
//! (Element<'x, ..>, u32)) -> impl Widget<..> + use<'x>`, and a region holds
//! the function's output by the function's item type, which rustc infers
//! where the function is passed by value (`scope::Region::insert_given`).
//! So a call in a region whose output is such a type is made through the
//! function.

use std::collections::BTreeMap;

thread_local! {
    /// The wrapper of each opaque type a region holds, by the type spelled
    /// at `'x`.
    static WRAPPERS: std::cell::RefCell<BTreeMap<String, Wrapper>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// A region family.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Family {
    /// A family struct the runner declares, as `RawrFamily2`.
    Named(String),
    /// What the runner's wrapper function `rawr_given_{0}` gives for the
    /// inputs `RawrGiven{0}`.
    Given(usize),
}

/// How a region's value is reached.
#[derive(Clone, Copy, Debug)]
pub(super) enum Reach {
    Insert,
    Get,
    GetMut,
    Take,
}

/// The function a call is made through, in a region, when its output is
/// an opaque type that borrows.
pub(super) struct Wrapper {
    index: usize,
    /// Each input's name in the call and its type at `'x`.
    inputs: Vec<(String, String)>,
    /// The call, of the inputs by their names.
    call: String,
    /// The opaque type's bounds, as `Widget<crate::Message, ..>`.
    bounds: String,
}

impl Family {
    /// The method call that reaches a region's value of the family, after
    /// `region.`, with `arguments` after the family's own.
    pub(super) fn call(&self, reach: Reach, arguments: &str) -> String {
        let method = match reach {
            Reach::Insert => "insert",
            Reach::Get => "get",
            Reach::GetMut => "get_mut",
            Reach::Take => "take",
        };
        match self {
            Self::Named(name) => format!("{method}::<{name}>({arguments})"),
            Self::Given(index) => {
                format!("{method}_given(RawrGiven{index}, rawr_given_{index}, {arguments})")
            }
        }
    }

    /// The family struct's name, for a family Rust code can write as a
    /// type, as `<RawrFamily2 as Family>::Of<'x>`.
    pub(super) fn named(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            Self::Given(_) => None,
        }
    }

    /// The call that makes the value of a family a wrapper gives, of the
    /// inputs by the names the call through it named them.
    pub(super) fn made(&self) -> Option<String> {
        let Self::Given(index) = self else {
            return None;
        };
        WRAPPERS.with_borrow(|wrappers| {
            let wrapper = wrappers.values().find(|wrapper| wrapper.index == *index)?;
            let names = wrapper
                .inputs
                .iter()
                .map(|(name, _)| format!("{name}, "))
                .collect::<String>();
            Some(format!("rawr_given_{index}(({names}))"))
        })
    }
}

impl Wrapper {
    /// The wrapper of a call `call` of `inputs`, by name and type at `'x`,
    /// whose output has `bounds`.
    pub(super) fn new(inputs: Vec<(String, String)>, call: String, bounds: String) -> Self {
        Self {
            index: 0,
            inputs,
            call,
            bounds,
        }
    }

    /// Declare the wrapper of the opaque type spelled `key` at `'x`, unless
    /// one is, and give its family.
    pub(super) fn declare(mut self, key: String) -> Family {
        WRAPPERS.with_borrow_mut(|wrappers| {
            let next = wrappers.len();
            let wrapper = wrappers.entry(key).or_insert_with(|| {
                self.index = next;
                self
            });
            Family::Given(wrapper.index)
        })
    }
}

/// The family of the opaque type spelled `key` at `'x`, when a wrapper
/// makes it.
pub(super) fn of(key: &str) -> Option<Family> {
    WRAPPERS.with_borrow(|wrappers| wrappers.get(key).map(|wrapper| Family::Given(wrapper.index)))
}

pub(super) fn clear() {
    WRAPPERS.with_borrow_mut(BTreeMap::clear);
}

/// Each wrapper, with the family of its inputs, into `rawr_families`.
pub(super) fn render(output: &mut String) {
    use std::fmt::Write as _;
    WRAPPERS.with_borrow(|wrappers| {
        for wrapper in wrappers.values() {
            let Wrapper {
                index,
                inputs,
                call,
                bounds,
            } = wrapper;
            let names = inputs
                .iter()
                .map(|(name, _)| format!("mut {name}, "))
                .collect::<String>();
            let types = inputs
                .iter()
                .map(|(_, ty)| format!("{ty}, "))
                .collect::<String>();
            let shown = format!("impl {}", bounds.replace("'x", "'_"));
            let _ = writeln!(
                output,
                "pub(super) struct RawrGiven{index};

impl __rawr_internal::scope::Family for RawrGiven{index} {{
    type Of<'x> = ({types});
}}

impl __rawr_internal::scope::Inputs for RawrGiven{index} {{
    fn name() -> &'static str {{
        {shown:?}
    }}
}}

#[allow(unused_mut)]
pub(super) fn rawr_given_{index}<'x>(({names}): ({types})) -> impl {bounds} + use<'x> {{
    {call}
}}
"
            );
        }
    });
}
