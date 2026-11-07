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
//!
//! A wrapper that takes what another gives, as `element.explain(a).
//! explain(b)`'s outer call would, is generic over the inner wrapper, and
//! rustc fails to compile its output where a region holds it. Such a type is
//! held boxed instead, as `Box<dyn RawrOpaque0<'x> + 'x>`: a trait of the
//! runner's own, implemented for every type of the opaque type's bounds,
//! whose methods are the calls the program makes on it ([`Ops`]). So is a
//! type that holds an opaque type, as `Container<'x, impl Widget<..>,
//! Theme>`, and an opaque type a free function makes of a region's value,
//! as `opaque(content)`: its trait is implemented for the type's shape, each
//! opaque position a parameter of the opaque type's bounds ([`Shape`]).

use std::collections::BTreeMap;

thread_local! {
    /// The wrapper of each opaque type a region holds, by the type spelled
    /// at `'x`.
    static WRAPPERS: std::cell::RefCell<BTreeMap<String, Wrapper>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
    /// The trait each opaque type a region holds boxed is held through, by
    /// the type spelled at `'x`.
    static OPS: std::cell::RefCell<BTreeMap<String, Ops>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
    /// The bounds of each opaque type a call gives, by the type spelled at
    /// `'x`, as the call's signature writes them.
    static BOUNDS: std::cell::RefCell<BTreeMap<String, String>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
    /// The runner's name of each opaque type a script type's proxy takes
    /// as an argument, by the type spelled at `'x` ([`named`]).
    static NAMES: std::cell::RefCell<BTreeMap<String, usize>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The name a script type's proxy takes the type spelled `key` at `'x`
/// by, a type that holds an opaque type, as loupe's `Zoomed<impl Widget>`
/// takes what `explain` gives: `RawrOpaqueName0`. A proxy holds only the
/// script's value, so its arguments name types and hold none, and each
/// opaque type takes a name no other type has.
pub(super) fn named(key: String) -> String {
    NAMES.with_borrow_mut(|names| {
        let next = names.len();
        format!("RawrOpaqueName{}", names.entry(key).or_insert(next))
    })
}

/// A type that holds opaque types, as an impl names it: the type spelled at
/// `'x` with `RawrHeld{i}` at its `i`-th opaque position, and each
/// position's bounds. An opaque type itself is the shape `RawrHeld0`.
pub(super) struct Shape {
    spelled: String,
    bounds: Vec<String>,
}

/// The trait of the runner's own an opaque type a region holds boxed is
/// held through, `RawrOpaque{index}`: one method for each call the program
/// makes on a value of it, as `fn rawr_0(self: Box<Self>) -> Element<'x,
/// ..>` for `.boxed()`.
pub(super) struct Ops {
    index: usize,
    /// The types it is implemented for.
    shape: Shape,
    methods: Vec<Method>,
}

/// One method of an [`Ops`] trait: the call `consumer` of the program on
/// the opaque type, the `consumer`-th that takes it.
pub(super) struct Method {
    consumer: usize,
    /// `self: Box<Self>`, `&self` or `&mut self`.
    receiver: &'static str,
    /// How the call reaches the value, as `*self` or `self`.
    reached: &'static str,
    inputs: Vec<(String, String)>,
    output: String,
    /// The call, of `receiver` and the inputs by their names, boxed when
    /// its output is held boxed too.
    call: String,
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
    WRAPPERS.with_borrow(|wrappers| {
        wrappers
            .get(key)
            .map(|wrapper| Family::Given(wrapper.index))
    })
}

pub(super) fn clear() {
    WRAPPERS.with_borrow_mut(BTreeMap::clear);
    OPS.with_borrow_mut(BTreeMap::clear);
    BOUNDS.with_borrow_mut(BTreeMap::clear);
    NAMES.with_borrow_mut(BTreeMap::clear);
}

/// Record `bounds` as those of the opaque type spelled `key` at `'x`,
/// unless some are.
pub(super) fn bound(key: String, bounds: String) {
    BOUNDS.with_borrow_mut(|known| {
        known.entry(key).or_insert(bounds);
    });
}

/// The bounds of the opaque type spelled `key` at `'x`, when a call that
/// gives it has them.
pub(super) fn bounds(key: &str) -> Option<String> {
    BOUNDS.with_borrow(|known| known.get(key).cloned())
}

impl Shape {
    /// The shape spelled `spelled`, whose `i`-th opaque position is
    /// `parameter(i)`, of `bounds[i]`.
    pub(super) fn new(spelled: String, bounds: Vec<String>) -> Self {
        Self { spelled, bounds }
    }

    /// The impl's parameter at the `position`-th opaque position.
    pub(super) fn parameter(position: usize) -> String {
        format!("RawrHeld{position}")
    }

    /// The type as messages show it, `impl Bounds` at each position.
    fn shown(&self) -> String {
        let mut shown = self.spelled.clone();
        // From the last, so `RawrHeld1` isn't read inside `RawrHeld10`.
        for (position, bounds) in self.bounds.iter().enumerate().rev() {
            shown = shown.replace(&Self::parameter(position), &format!("impl {bounds}"));
        }
        shown
    }
}

impl Ops {
    /// Declare the trait the type spelled `key` at `'x`, of `shape`, is
    /// held boxed through, unless one is, and give the type a region holds
    /// it as, `Box<dyn RawrOpaque0<'x> + 'x>`, and whether it is new.
    pub(super) fn declare(key: String, shape: Shape) -> (String, bool) {
        OPS.with_borrow_mut(|ops| {
            let next = ops.len();
            let mut new = false;
            let index = ops
                .entry(key)
                .or_insert_with(|| {
                    new = true;
                    Self {
                        index: next,
                        shape,
                        methods: Vec::new(),
                    }
                })
                .index;
            (format!("Box<dyn RawrOpaque{index}<'x> + 'x>"), new)
        })
    }

    /// The Rust name of the type a region holds as `held`, as `impl
    /// Widget<..>`, when it holds one so.
    pub(super) fn shown(held: &str) -> Option<String> {
        OPS.with_borrow(|ops| {
            ops.values()
                .find(|ops| held == format!("Box<dyn RawrOpaque{}<'x> + 'x>", ops.index))
                .map(|ops| ops.shape.shown())
        })
    }

    /// The type a region holds the opaque type spelled `key` at `'x` as,
    /// when it is held boxed.
    pub(super) fn held(key: &str) -> Option<String> {
        OPS.with_borrow(|ops| {
            ops.get(key)
                .map(|ops| format!("Box<dyn RawrOpaque{}<'x> + 'x>", ops.index))
        })
    }

    /// The call of the `consumer`-th call on the opaque type spelled `key`,
    /// on `receiver`, a region's `Box<dyn ..>` of it reached as `receiver`
    /// says, with `arguments` inputs: its trait's method.
    pub(super) fn call(
        key: &str,
        consumer: usize,
        receiver: &str,
        arguments: usize,
    ) -> Option<String> {
        OPS.with_borrow(|ops| {
            let ops = ops.get(key)?;
            ops.methods
                .iter()
                .any(|method| method.consumer == consumer)
                .then(|| {
                    let mut inputs = vec![receiver.to_owned()];
                    inputs.extend((0..arguments).map(|position| format!("argument_{position}")));
                    format!(
                        "RawrOpaque{}::rawr_{consumer}({})",
                        ops.index,
                        inputs.join(", ")
                    )
                })
        })
    }

    /// Add `method`, the `consumer`-th call on the opaque type spelled
    /// `key`, to the type's trait.
    pub(super) fn add(key: &str, method: Method) {
        OPS.with_borrow_mut(|ops| {
            if let Some(ops) = ops.get_mut(key)
                && !ops
                    .methods
                    .iter()
                    .any(|known| known.consumer == method.consumer)
            {
                ops.methods.push(method);
            }
        });
    }
}

impl Method {
    /// The `consumer`-th call on an opaque type, `call`, of the receiver as
    /// `receiver` takes it (`Box<Self>`, `&self` or `&mut self`) and of
    /// `inputs`, by name and type at `'x`, giving `output`.
    pub(super) fn new(
        consumer: usize,
        receiver: crate::classify::representability::ownership::Receiver,
        inputs: Vec<(String, String)>,
        output: String,
        call: String,
    ) -> Self {
        use crate::classify::representability::ownership::Receiver;
        let (receiver, reached) = match receiver {
            Receiver::Owned => ("self: Box<Self>", "*self"),
            Receiver::Shared => ("&self", "self"),
            Receiver::Mutable => ("&mut self", "self"),
        };
        Self {
            consumer,
            receiver,
            reached,
            inputs,
            output,
            call,
        }
    }
}

/// Each wrapper, with the family of its inputs, each trait a region holds a
/// value boxed through, and each opaque type's name, into `rawr_families`.
pub(super) fn render(output: &mut String) {
    use std::fmt::Write as _;
    NAMES.with_borrow(|names| {
        for (key, index) in names {
            let _ = writeln!(
                output,
                "/// `{}` as a proxy's argument.\npub(super) struct RawrOpaqueName{index};\n",
                key.replace("'x", "'_")
            );
        }
    });
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
    OPS.with_borrow(|ops| {
        for Ops {
            index,
            shape,
            methods,
        } in ops.values()
        {
            let parameters = shape
                .bounds
                .iter()
                .enumerate()
                .map(|(position, bounds)| format!(", {}: {bounds} + 'x", Shape::parameter(position)))
                .collect::<String>();
            let mut declared = String::new();
            let mut implemented = String::new();
            for method in methods {
                let Method {
                    consumer,
                    receiver,
                    reached,
                    inputs,
                    output: given,
                    call,
                } = method;
                let inputs = inputs
                    .iter()
                    .map(|(name, ty)| format!(", {name}: {ty}"))
                    .collect::<String>();
                let head = format!("fn rawr_{consumer}({receiver}{inputs}) -> {given}");
                let _ = write!(declared, "\n    {head};");
                let _ = write!(
                    implemented,
                    "\n    #[allow(unused_mut)]\n    {head} {{\n        let mut receiver = {reached};\n        {call}\n    }}"
                );
            }
            let _ = writeln!(
                output,
                "pub(super) trait RawrOpaque{index}<'x> {{{declared}
}}

impl<'x{parameters}> RawrOpaque{index}<'x> for {spelled} {{{implemented}
}}
",
                spelled = shape.spelled
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{Family, Method, Ops, Reach, Shape};

    /// A family struct is reached by its type, and a wrapper's family by
    /// its inputs' family and the wrapper itself, which rustc infers it from.
    #[test]
    fn a_wrapper_s_family_is_reached_by_its_inputs_and_function() {
        assert_eq!(
            Family::Named("RawrFamily2".to_owned()).call(Reach::Take, "scoped"),
            "take::<RawrFamily2>(scoped)"
        );
        assert_eq!(
            Family::Given(3).call(Reach::GetMut, "scoped"),
            "get_mut_given(RawrGiven3, rawr_given_3, scoped)"
        );
        assert_eq!(Family::Given(3).named(), None);
    }

    /// A type that holds opaque types is held boxed through a trait
    /// implemented for its shape, each opaque position a parameter of its
    /// bounds, and a call on it is that trait's method.
    #[test]
    fn a_shape_is_implemented_over_its_opaque_positions() {
        super::clear();
        let shape = Shape::new(
            "Container<'x, RawrHeld0, Theme>".to_owned(),
            vec!["Widget<Message>".to_owned()],
        );
        let (held, new) = Ops::declare("container".to_owned(), shape);
        assert_eq!(
            (held.as_str(), new),
            ("Box<dyn RawrOpaque0<'x> + 'x>", true)
        );
        assert_eq!(
            Ops::shown(&held).as_deref(),
            Some("Container<'x, impl Widget<Message>, Theme>")
        );
        Ops::add(
            "container",
            Method::new(
                2,
                crate::classify::representability::ownership::Receiver::Owned,
                vec![("argument_0".to_owned(), "u16".to_owned())],
                held.clone(),
                "Box::new(receiver.padding(argument_0))".to_owned(),
            ),
        );
        assert_eq!(
            Ops::call("container", 2, "receiver", 1).as_deref(),
            Some("RawrOpaque0::rawr_2(receiver, argument_0)")
        );
        let mut rendered = String::new();
        super::render(&mut rendered);
        assert!(
            rendered.contains(
                "impl<'x, RawrHeld0: Widget<Message> + 'x> RawrOpaque0<'x> for Container<'x, RawrHeld0, Theme> {"
            ),
            "{rendered}"
        );
        super::clear();
    }
}
