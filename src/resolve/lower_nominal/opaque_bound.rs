//! What rustc knows a value of a program function's `impl Trait` output
//! by: the bound's trait alone. Rawr runs the value as its hidden type, so
//! a method call on it would find that type's inherent methods first, where
//! rustc finds only the trait's. The bound is kept by the value's span and
//! by each local that holds the value or a borrow of it, and a method call
//! whose receiver has one calls the trait's method over its `Self`
//! (`surface::member`), as `Trait::method(&value)` does.

use super::*;

/// The crate trait each value and local is known by (`Parser::opaque_bounds`).
#[derive(Default)]
pub(super) struct Bounds {
    /// By the span of the value a call gives, or of a borrow of one.
    values: BTreeMap<Span, ImmutableString>,
    /// By the local's name, with the declaration it was bound at, so a later
    /// local of the name is apart.
    locals: BTreeMap<ImmutableString, (Span, ImmutableString)>,
    /// The member positions `bound_method` spliced the trait's method at.
    /// A macro's body takes its call's position for each of its tokens, so
    /// a later member there is at the same position, and is the trait's
    /// only when its own receiver has the bound.
    spliced: BTreeSet<Position>,
}

impl Parser<'_> {
    /// The value spanning `at`, which a call of a function whose output is
    /// written `written` gives, is known by the trait `written` bounds, when
    /// it is an `impl Trait`.
    pub(super) fn note_opaque_bound(&self, written: Option<&type_ref::Type>, at: Span) {
        if let Some(bound) = written.and_then(type_ref::Type::opaque_trait) {
            self.opaque_bounds
                .borrow_mut()
                .values
                .insert(at, bound.into());
        }
    }

    /// The crate trait `value` is known by: its own, or that of the local
    /// it reads.
    pub(super) fn opaque_bound(&self, value: &ParsedExpr) -> Option<ImmutableString> {
        let bounds = self.opaque_bounds.borrow();
        if let Some(bound) = bounds.values.get(&value.span) {
            return Some(bound.clone());
        }
        // A local, or a borrow that reads the whole of one, as `let r =
        // &value;` does.
        let binding = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => binding,
            Some(Lvalue::Place { binding, place, .. }) if place.as_str() == binding.as_str() => {
                binding
            }
            _ => return None,
        };
        let (declaration, bound) = bounds.locals.get(binding)?;
        (self.bindings.get(binding)?.declaration == *declaration).then(|| bound.clone())
    }

    /// The local `name` declared at `declaration` holds `value`, and so is
    /// known by `value`'s bound, or by none.
    pub(super) fn bind_opaque_bound(&self, name: &str, declaration: Span, value: &ParsedExpr) {
        let bound = self.opaque_bound(value);
        let locals = &mut self.opaque_bounds.borrow_mut().locals;
        match bound {
            Some(bound) => {
                locals.insert(name.into(), (declaration, bound));
            }
            None => {
                locals.remove(name);
            }
        }
    }

    /// `&value`, spanning `whole`, is known by `value`'s bound, which
    /// autoderef reaches through the borrow.
    pub(super) fn borrow_opaque_bound(&self, value: &ParsedExpr, whole: Span) {
        if let Some(bound) = self.opaque_bound(value) {
            self.opaque_bounds.borrow_mut().values.insert(whole, bound);
        }
    }

    /// `value.member(..)` with its member at `member_span`, on a value
    /// known by a trait that has `member`: the trait's method, as
    /// `Trait::member(&value, ..)` names it.
    pub(super) fn bound_method(&mut self, value: &ParsedExpr, member: &str, member_span: Span) {
        let at = member_span.start();
        // A splice this made for an earlier member at the position holds
        // for that member alone.
        if self.opaque_bounds.borrow_mut().spliced.remove(&at) {
            self.spliced_traits.remove(&at);
        }
        if self.spliced_traits.contains_key(&at) {
            return;
        }
        let Some(bound) = self.opaque_bound(value) else {
            return;
        };
        let path = rawr_import::surface::member::path(&bound, member);
        if self.models.host_items(&path).is_some() {
            self.spliced_traits.insert(at, bound);
            self.opaque_bounds.borrow_mut().spliced.insert(at);
        }
    }
}
