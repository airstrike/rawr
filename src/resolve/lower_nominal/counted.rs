//! `std::rc::Rc<T>` and `std::sync::Arc<T>`, and their `Weak`s, as real
//! shared pointers (D712.2).
//!
//! At runtime a pointer is a `Counted`: a handle to one allocation, which
//! holds the value in a cell (`cell.rs`). Each runtime copy of a handle is
//! one of Rust's pointers: a clone is a copy, a move leaves `()` in the
//! local it leaves, and a copy goes when its local's scope ends. So the
//! strong count is the number of handles alive, as Rust's is, two
//! handles are `ptr_eq` when they share the allocation, and a `Weak`
//! upgrades while a handle is alive. Std's `Deref` reads the value, and
//! `get_mut` and `make_mut` lend the cell as the place a `&mut T` changes.
//!
//! An `Rc` or `Arc` of a crate's value stands for the crate's own pointer
//! once one crosses (#775): its value is the crate value's handle, whose
//! slot the runner's native pointer is the home of, and the allocation is
//! linked to that pointer ([`Link`]). Its count is then
//! rawr's handles and the native pointers outside rawr, as Rust's is, and
//! two pointers to one native allocation are `ptr_eq`.

use super::*;

#[cfg(not(feature = "sync"))]
type WeakShared<T> = std::rc::Weak<T>;
#[cfg(feature = "sync")]
type WeakShared<T> = std::sync::Weak<T>;

/// What a runner reads of a crate's own `Rc` or `Arc` that a rawr `Rc` or
/// `Arc` stands for (#775), given the pointer's value, the handle of the
/// crate value it points to: its strong count and its address, while the
/// value is in it.
pub type Link = fn(&Dynamic) -> Option<(usize, usize)>;

/// One allocation: the value, in a cell `get_mut` and `make_mut` lend,
/// and the crate's own pointer it stands for, once one crosses.
pub struct Allocation {
    value: Dynamic,
    native: std::sync::OnceLock<Link>,
}

/// An `Rc` or `Arc`.
#[derive(Clone)]
pub struct Counted(crate::Shared<Allocation>);

/// A `Weak`: the allocation while a `Counted` of it is alive. `Weak::new`
/// has none.
#[derive(Clone)]
pub struct Weak(WeakShared<Allocation>);

impl Counted {
    fn new(value: Dynamic) -> Self {
        Self(crate::Shared::new(Allocation {
            value: cell::made(value, false),
            native: std::sync::OnceLock::new(),
        }))
    }

    fn get(&self) -> Dynamic {
        cell::value(&self.0.value).unwrap_or(Dynamic::UNIT)
    }

    /// The strong count and address of the crate's own pointer this
    /// stands for, while its value is in the pointer.
    fn native(&self) -> Option<(usize, usize)> {
        let link = self.0.native.get()?;
        link(&self.get())
    }

    /// rawr's handles, and the crate's pointers but the one rawr's
    /// allocation holds.
    fn strong(&self) -> usize {
        let native = self
            .native()
            .map_or(0, |(count, _)| count.saturating_sub(1));
        crate::Shared::strong_count(&self.0) + native
    }

    /// Whether `self` and `other` point to one allocation: rawr's, or the
    /// crate's own both stand for.
    fn ptr_eq(&self, other: &Self) -> bool {
        crate::Shared::ptr_eq(&self.0, &other.0)
            || matches!(
                (self.native(), other.native()),
                (Some((_, left)), Some((_, right))) if left == right
            )
    }

    fn weak(&self) -> usize {
        crate::Shared::weak_count(&self.0)
    }

    /// Whether this is the only pointer to the allocation, strong or weak,
    /// as `get_mut` needs.
    fn unique(&self) -> bool {
        self.strong() == 1 && self.weak() == 0
    }
}

/// What `value` is when it is an `Rc` or `Arc`, read where it is: a copy
/// of the handle would count as one more pointer.
fn with_counted<R>(value: &Dynamic, read: impl FnOnce(&Counted) -> R) -> Option<R> {
    value.read_lock::<Counted>().map(|counted| read(&counted))
}

fn with_weak<R>(value: &Dynamic, read: impl FnOnce(&Weak) -> R) -> Option<R> {
    value.read_lock::<Weak>().map(|weak| read(&weak))
}

/// The value an `Rc` or `Arc` points to, for `==`, ordering and `Debug`;
/// `None` when `value` is no pointer.
pub(crate) fn value(value: &Dynamic) -> Option<Dynamic> {
    with_counted(value, Counted::get)
}

/// Whether `value` is a `Weak`, whose `Debug` is `(Weak)`.
pub(crate) fn is_weak(value: &Dynamic) -> bool {
    value.is::<Weak>()
}

/// A new `Rc` or `Arc` of `value`.
pub(crate) fn new(value: Dynamic) -> Dynamic {
    Dynamic::from(Counted::new(value))
}

/// A new `Rc` or `Arc` of `value`, a crate value's handle, standing for
/// the crate's own pointer `link` reads.
#[cfg(feature = "sync")]
pub(crate) fn native(value: Dynamic, link: Link) -> Dynamic {
    let counted = Counted::new(value);
    let _ = counted.0.native.set(link);
    Dynamic::from(counted)
}

/// The value the `Rc` or `Arc` `value` points to, rawr's strong count of
/// it, and whether `value` is rawr's only pointer to it, strong or weak.
#[cfg(feature = "sync")]
pub(crate) fn pointee(value: &Dynamic) -> Option<(Dynamic, usize, bool)> {
    with_counted(value, |pointer| {
        let strong = crate::Shared::strong_count(&pointer.0);
        let only = strong == 1 && crate::Shared::weak_count(&pointer.0) == 0;
        (pointer.get(), strong, only)
    })
}

/// Link the `Rc` or `Arc` `value` to the crate's own pointer `link` reads,
/// once.
#[cfg(feature = "sync")]
pub(crate) fn link(value: &Dynamic, link: Link) {
    with_counted(value, |pointer| {
        let _ = pointer.0.native.set(link);
    });
}

/// `Weak::new()`: a `Weak` of no allocation.
pub(crate) fn weak() -> Dynamic {
    Dynamic::from(Weak(WeakShared::new()))
}

fn int(count: usize) -> Dynamic {
    Dynamic::from_int(INT::try_from(count).unwrap_or(INT::MAX))
}

fn not_one() -> Box<EvalAltResult> {
    invariant("an `Rc` or `Arc` is not one at runtime")
}

pub(super) fn invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    let not_weak = || invariant("a `Weak` is not one at runtime");
    match op {
        PreludeOp::CountedNew => Ok(new(args[0].take())),
        PreludeOp::WeakNew => Ok(weak()),

        PreludeOp::WeakUpgrade => with_weak(args[0], |pointer| match pointer.0.upgrade() {
            Some(allocation) => crate::option::some(Dynamic::from(Counted(allocation))),
            None => crate::option::none(),
        })
        .ok_or_else(not_weak),
        PreludeOp::WeakStrong => {
            with_weak(args[0], |pointer| int(pointer.0.strong_count())).ok_or_else(not_weak)
        }
        // No strong pointer left: none, as std's `weak_count`.
        PreludeOp::WeakWeakCount => with_weak(args[0], |pointer| match pointer.0.strong_count() {
            0 => int(0),
            _ => int(pointer.0.weak_count()),
        })
        .ok_or_else(not_weak),
        PreludeOp::WeakPtrEq => {
            let (first, second) = (&*args[0], &*args[1]);
            with_weak(first, |pointer| {
                with_weak(second, |other| pointer.0.ptr_eq(&other.0))
            })
            .flatten()
            .map(Dynamic::from_bool)
            .ok_or_else(not_weak)
        }
        PreludeOp::CountedGet => with_counted(args[0], Counted::get).ok_or_else(not_one),
        PreludeOp::CountedStrong => {
            with_counted(args[0], |pointer| int(pointer.strong())).ok_or_else(not_one)
        }
        PreludeOp::CountedWeakCount => {
            with_counted(args[0], |pointer| int(pointer.weak())).ok_or_else(not_one)
        }
        PreludeOp::CountedPtrEq => {
            let (first, second) = (&*args[0], &*args[1]);
            with_counted(first, |pointer| {
                with_counted(second, |other| pointer.ptr_eq(other))
            })
            .flatten()
            .map(Dynamic::from_bool)
            .ok_or_else(not_one)
        }
        PreludeOp::CountedDowngrade => with_counted(args[0], |pointer| {
            Dynamic::from(Weak(crate::Shared::downgrade(&pointer.0)))
        })
        .ok_or_else(not_one),
        // `Some` of the value's cell, the place the `&mut T` changes, when
        // no other pointer, strong or weak, shares it.
        PreludeOp::CountedGetMut => with_counted(args[0], |pointer| {
            if pointer.unique() {
                crate::option::some(pointer.0.value.clone())
            } else {
                crate::option::none()
            }
        })
        .ok_or_else(not_one),
        // A pointer that shares its value is given a copy of its own first,
        // as std's `make_mut` clones it into a new allocation. The pointer
        // is taken where it is.
        PreludeOp::CountedMakeMut => {
            let (unique, value) =
                with_counted(args[0], |pointer| (pointer.unique(), pointer.get()))
                    .ok_or_else(not_one)?;
            if !unique {
                *args[0] = new(value);
            }
            with_counted(args[0], |pointer| pointer.0.value.clone()).ok_or_else(not_one)
        }
        // `Ok` of the value when this was its only strong pointer, else
        // `Err` of the pointer, as `Rc::try_unwrap`; `into_inner`'s
        // `Some` or `None`, and `unwrap_or_clone`'s value either way.
        PreludeOp::CountedTryUnwrap
        | PreludeOp::CountedIntoInner
        | PreludeOp::CountedUnwrapOrClone => {
            let (only, value) =
                with_counted(args[0], |pointer| (pointer.strong() == 1, pointer.get()))
                    .ok_or_else(not_one)?;
            let taken = args[0].take();
            Ok(match (op, only) {
                (PreludeOp::CountedTryUnwrap, true) => crate::result::ok(value),
                (PreludeOp::CountedTryUnwrap, false) => crate::result::err(taken),
                (PreludeOp::CountedIntoInner, true) => crate::option::some(value),
                (PreludeOp::CountedIntoInner, false) => crate::option::none(),
                _ => value,
            })
        }
        _ => Err(invariant("not an `Rc` or `Arc` operation")),
    }
}

/// `std::rc::Weak` or `std::sync::Weak` as a path is written with its
/// module: rawr's name for it, `Weak` or `SyncWeak`.
pub(crate) fn std_weak(path: &str) -> Option<&'static str> {
    match path {
        "rc::Weak" | "std::rc::Weak" | "alloc::rc::Weak" => Some("Weak"),
        "sync::Weak" | "std::sync::Weak" | "alloc::sync::Weak" => Some("SyncWeak"),
        _ => None,
    }
}

impl Parser<'_> {
    /// The `Weak` `name` names in a path: as imported from `std::rc` or
    /// `std::sync`, or written with its module, unless the program names
    /// something so.
    pub(super) fn weak_named(&self, name: &str) -> Option<Counter> {
        if self.models.get(name).is_some() {
            return None;
        }
        let imported = || {
            self.models
                .import(name)
                .and_then(|binding| std_weak(&binding.qualified))
        };
        std_weak(name).or_else(imported).map(Counter::weak_named)
    }

    /// `value`, an `Rc` or `Arc` (or a reference to one), as the value it
    /// points to, read through std's `Deref`: a copy, as a read of a
    /// shared reference's referent is at runtime.
    pub(super) fn counted_value(&mut self, value: &mut ParsedExpr) {
        value.source = self.counted_call(PreludeOp::CountedGet, value.rendered_source(), []);
        value.prelude = origin::Fragment::default();
        value.lvalue = None;
    }

    /// `op` called on the pointer `receiver` where it is, as a method, so
    /// the call makes no copy of the handle, which would count.
    pub(super) fn counted_call(
        &mut self,
        op: PreludeOp,
        receiver: origin::Fragment,
        arguments: impl IntoIterator<Item = origin::Fragment>,
    ) -> origin::Fragment {
        let helper = self.push_op(Op::Prelude(op));
        let mut source = receiver.surrounded("(", ")");
        source.push_text(format!(".{helper}("));
        source.push(origin::Fragment::join(arguments, ", "));
        source.push_text(")");
        source
    }

    /// The error for a member of an `Rc`, `Arc` or `Weak` (`owner`, whose
    /// std definition is `path`) that rawr doesn't run: the not-yet error
    /// for one std has (D712.1), else rustc's E0599.
    fn counted_unknown(
        &self,
        owner: &str,
        path: &str,
        member: &ImmutableString,
        reference: Span,
    ) -> Error {
        if crate::std_table::item(&format!("{path}::{member}")).is_some() {
            return Error::NotYet {
                path: format!("{owner}::{member}").into(),
                reference,
            };
        }
        Error::UnknownMember {
            owner: owner.into(),
            member: member.clone(),
            reference,
        }
    }

    /// `argument` where `&Rc<T>` (a `Weak` when `weak`, `&mut` when
    /// `mutable`) is taken; `T`.
    fn counted_argument(
        &mut self,
        counter: Counter,
        weak: bool,
        mutable: bool,
        argument: &mut ParsedExpr,
    ) -> Result<Ty, Error> {
        let element = self.inference.fresh();
        let pointer = if weak {
            Ty::Weak(counter, Box::new(element.clone()))
        } else {
            Ty::Counted(counter, Box::new(element.clone()))
        };
        let expected = Ty::Reference {
            mutable,
            target: Box::new(pointer),
        };
        self.coerce_expr(&expected, argument)?;
        Ok(self.inference.resolve(&element))
    }

    /// `Rc::member` or `Weak::member` (`weak`), after the path: std's
    /// associated functions rawr runs (D711's execution mapping for `Rc`,
    /// `Arc` and their `Weak`s). Any other std has is the not-yet error.
    pub(super) fn parse_counted_path(
        &mut self,
        counter: Counter,
        weak: bool,
        member: ImmutableString,
        name_span: Span,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (rc_path, weak_path) = counter.std_paths();
        let (owner, path) = if weak {
            ("Weak", weak_path)
        } else {
            (counter.name(), rc_path)
        };
        // `.map(Arc::new)`: the function as a value.
        if !weak
            && member.as_str() == "new"
            && !matches!(self.peek(), Token::LeftParen | Token::Unit)
        {
            let input = self.inference.fresh();
            let output = Ty::Counted(counter, Box::new(input.clone()));
            let parameter = self.temporary("value");
            let mut source = origin::Fragment::plain(format!("|{parameter}| "));
            let made = self.prelude_op(PreludeOp::CountedNew, [origin::Fragment::plain(parameter)]);
            source.push(made);
            let ty = Ty::Fn {
                inputs: Box::new([input]),
                output: Box::new(output),
            };
            return Ok(self.operator_expr(source, ty, span(name_span.start(), member_span.end())));
        }
        let runs = matches!(
            (weak, member.as_str()),
            (
                false,
                "new"
                    | "from"
                    | "clone"
                    | "strong_count"
                    | "weak_count"
                    | "ptr_eq"
                    | "downgrade"
                    | "get_mut"
                    | "make_mut"
                    | "try_unwrap"
                    | "into_inner"
                    | "unwrap_or_clone"
            ) | (
                true,
                "new" | "clone" | "upgrade" | "strong_count" | "weak_count" | "ptr_eq"
            )
        );
        if !runs || !matches!(self.peek(), Token::LeftParen | Token::Unit) {
            return Err(self.counted_unknown(owner, path, &member, member_span));
        }
        let (mut arguments, end) =
            self.parse_borrowing_arguments(&format!("`{owner}` arguments"))?;
        let call = span(name_span.start(), end);
        let arity = match member.as_str() {
            "new" if weak => 0,
            "ptr_eq" => 2,
            _ => 1,
        };
        if arguments.len() != arity {
            return Err(Error::argument_count(
                crate::resolve::Callee::Function,
                format!("{owner}::{member}"),
                arity,
                arguments.len(),
                call,
            ));
        }
        if weak {
            return self.weak_function(counter, &member, arguments, call);
        }
        let mut argument = arguments.remove(0);
        let pointer = |element: Ty| Ty::Counted(counter, Box::new(element));
        match member.as_str() {
            "new" => {
                let element = self.inference.resolve(&argument.ty);
                let ty = pointer(element);
                // A value with drop glue in an `Rc` or `Arc` is refused
                // (`needs_drop`).
                self.needs_drop(&ty, call)?;
                self.move_local(&mut argument)?;
                let source = self.prelude_op(PreludeOp::CountedNew, [argument.rendered_source()]);
                Ok(self.operator_expr(source, ty, call))
            }
            // `Rc::from(value)`, by std's `impl From<T> for Rc<T>`.
            "from" => {
                let element = self.inference.resolve(&argument.ty);
                let ty = pointer(element.clone());
                let Some(Some(op)) = self.std_conversion(&ty, &element) else {
                    return Err(self.conversion_not_yet(&ty, &element, call).unwrap_or_else(
                        || self.counted_unknown(owner, path, &member, member_span),
                    ));
                };
                self.needs_drop(&ty, call)?;
                self.move_local(&mut argument)?;
                let source = self.prelude_op(op, [argument.rendered_source()]);
                Ok(self.operator_expr(source, ty, call))
            }
            // Another pointer to the value: a copy of the handle.
            "clone" => {
                let element = self.counted_argument(counter, false, false, &mut argument)?;
                Ok(self.operator_expr(argument.rendered_source(), pointer(element), call))
            }
            "strong_count" | "weak_count" | "downgrade" => {
                let element = self.counted_argument(counter, false, false, &mut argument)?;
                let (op, ty) = match member.as_str() {
                    "strong_count" => (PreludeOp::CountedStrong, Ty::Int(Int::Usize)),
                    "weak_count" => (PreludeOp::CountedWeakCount, Ty::Int(Int::Usize)),
                    _ => (
                        PreludeOp::CountedDowngrade,
                        Ty::Weak(counter, Box::new(element)),
                    ),
                };
                let source = self.counted_call(op, argument.rendered_source(), []);
                Ok(self.operator_expr(source, ty, call))
            }
            "ptr_eq" => {
                let mut other = arguments.remove(0);
                self.counted_argument(counter, false, false, &mut argument)?;
                self.counted_argument(counter, false, false, &mut other)?;
                let source = self.counted_call(
                    PreludeOp::CountedPtrEq,
                    argument.rendered_source(),
                    [other.rendered_source()],
                );
                Ok(self.operator_expr(source, Ty::Bool, call))
            }
            // `Some(&mut T)` of the value's place when no other pointer
            // shares it, read and stored as an imported method's
            // `Option<&mut T>` is.
            "get_mut" => {
                let element = self.counted_argument(counter, false, true, &mut argument)?;
                let source =
                    self.counted_call(PreludeOp::CountedGetMut, argument.rendered_source(), []);
                let value = self.operator_expr(source, Ty::Option(Box::new(element)), call);
                Ok(self.optional_guard(value))
            }
            // `&mut T` of the value's place, which the pointer is given a
            // copy of its own for first when it shares it.
            "make_mut" => {
                let element = self.counted_argument(counter, false, true, &mut argument)?;
                if !self.type_provides(&element, capability::Kind::Clone) {
                    return Err(Error::MissingCapability {
                        owner: element.name(&self.models),
                        capability: "Clone".into(),
                        operation: call,
                        derive: matches!(element, Ty::Script(..)),
                        required_by: None,
                    });
                }
                let source =
                    self.counted_call(PreludeOp::CountedMakeMut, argument.rendered_source(), []);
                let value = self.operator_expr(source, element, call);
                Ok(self.guarded_output(value, crate::host::item::Guard::Mutable))
            }
            // The pointer by value: `Ok` or `Some` of the value when it was
            // the only strong one.
            "try_unwrap" | "into_inner" | "unwrap_or_clone" => {
                let element = self.inference.fresh();
                self.coerce_expr(&pointer(element.clone()), &mut argument)?;
                self.move_local(&mut argument)?;
                let element = self.inference.resolve(&element);
                let (op, ty) = match member.as_str() {
                    "try_unwrap" => (
                        PreludeOp::CountedTryUnwrap,
                        Ty::Result(Box::new(element.clone()), Box::new(pointer(element))),
                    ),
                    "into_inner" => (PreludeOp::CountedIntoInner, Ty::Option(Box::new(element))),
                    _ => (PreludeOp::CountedUnwrapOrClone, element),
                };
                let source = self.prelude_op(op, [argument.rendered_source()]);
                Ok(self.operator_expr(source, ty, call))
            }
            _ => Err(self.counted_unknown(owner, path, &member, member_span)),
        }
    }

    /// `Weak::new()`, and `Weak::upgrade(&w)` and its kin.
    fn weak_function(
        &mut self,
        counter: Counter,
        member: &str,
        mut arguments: Vec<ParsedExpr>,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        if member == "new" {
            let ty = Ty::Weak(counter, Box::new(self.inference.fresh()));
            let source = self.prelude_op(PreludeOp::WeakNew, []);
            return Ok(self.operator_expr(source, ty, call));
        }
        let mut receiver = arguments.remove(0);
        let element = self.counted_argument(counter, true, false, &mut receiver)?;
        self.weak_method(counter, member, element, &receiver, arguments, call)
    }

    /// A `Weak`'s method on `receiver`, whose arguments are parsed.
    fn weak_method(
        &mut self,
        counter: Counter,
        member: &str,
        element: Ty,
        receiver: &ParsedExpr,
        mut arguments: Vec<ParsedExpr>,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        let (op, ty) = match member {
            "clone" => {
                let ty = Ty::Weak(counter, Box::new(element));
                return Ok(self.operator_expr(receiver.rendered_source(), ty, call));
            }
            "upgrade" => (
                PreludeOp::WeakUpgrade,
                Ty::Option(Box::new(Ty::Counted(counter, Box::new(element)))),
            ),
            "strong_count" => (PreludeOp::WeakStrong, Ty::Int(Int::Usize)),
            "weak_count" => (PreludeOp::WeakWeakCount, Ty::Int(Int::Usize)),
            _ => {
                let mut other = arguments.remove(0);
                self.counted_argument(counter, true, false, &mut other)?;
                arguments.push(other);
                (PreludeOp::WeakPtrEq, Ty::Bool)
            }
        };
        let rest: Vec<_> = arguments.iter().map(ParsedExpr::rendered_source).collect();
        let source = self.counted_call(op, receiver.rendered_source(), rest);
        Ok(self.operator_expr(source, ty, call))
    }

    /// `pointer.member` of an `Rc`, `Arc` or `Weak`, or a reference to one:
    /// its own method (`clone`, `as_ref`, a `Weak`'s `upgrade`), with what
    /// the call gives. Else `None`, and an `Rc`'s or `Arc`'s `value` is
    /// `&T` now, read through std's `Deref`, whose member it is.
    pub(super) fn counted_member(
        &mut self,
        value: &mut ParsedExpr,
        member: &ImmutableString,
        member_span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let mut pointer = self.inference.resolve(&value.ty);
        while let Ty::Reference { target, .. } = pointer {
            pointer = self.inference.resolve(&target);
        }
        let call = matches!(self.peek(), Token::LeftParen | Token::Unit);
        match pointer {
            Ty::Weak(counter, element) => {
                let runs = matches!(
                    member.as_str(),
                    "clone" | "upgrade" | "strong_count" | "weak_count" | "ptr_eq"
                );
                if !runs || !call {
                    let path = counter.std_paths().1;
                    return Err(self.counted_unknown("Weak", path, member, member_span));
                }
                let (arguments, end) = self.parse_call_arguments("`Weak` arguments")?;
                let arity = usize::from(member.as_str() == "ptr_eq");
                let at = span(value.span.start(), end);
                if arguments.len() != arity {
                    return Err(Error::argument_count(
                        crate::resolve::Callee::Method,
                        format!("Weak::{member}"),
                        arity,
                        arguments.len(),
                        at,
                    ));
                }
                self.weak_method(counter, member, *element, value, arguments, at)
                    .map(Some)
            }
            Ty::Counted(counter, element) => {
                // `rc.clone()`: another pointer to the value, a copy of the
                // handle.
                if member.as_str() == "clone" && matches!(self.peek(), Token::Unit) {
                    let end = self.next().1;
                    let mut parsed = value.clone();
                    parsed.ty = Ty::Counted(counter, element);
                    parsed.span = span(value.span.start(), end);
                    parsed.lvalue = None;
                    return Ok(Some(parsed));
                }
                // `rc.as_ref()`: the value, borrowed.
                let as_ref = member.as_str() == "as_ref" && matches!(self.peek(), Token::Unit);
                self.counted_value(value);
                // Its parts can't be moved out of (rustc's E0507).
                self.unmovable.insert(
                    value.span,
                    Unplaced {
                        moved: (
                            "E0507",
                            format!("cannot move out of an `{}`", counter.name()),
                        ),
                        assigned: Some(counter.unassignable()),
                    },
                );

                value.ty = Ty::Reference {
                    mutable: false,
                    target: element,
                };
                if as_ref {
                    let end = self.next().1;
                    value.span = span(value.span.start(), end);
                    return Ok(Some(value.clone()));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

impl Parser<'_> {
    /// Whether a value of `ty` may be shared between threads, as rustc's
    /// `Sync` has it for the types rawr represents: a `RefCell`, a `Cell`,
    /// an `Rc` and an `rc::Weak` aren't, and what holds one isn't. An
    /// `Arc<T>` is `Send` only when `T` is `Send` and this.
    pub(super) fn shared_between_threads(&self, ty: &Ty) -> bool {
        self.sync_in(ty, &mut Vec::new())
    }

    /// Whether rawr knows a value of `ty` isn't `Send`: it holds an `Rc`,
    /// an `rc::Weak`, an `Arc` of what isn't `Sync`, or a shared
    /// reference to what isn't. rawr proves no auto trait, so any other
    /// type passes a `Send` bound, as a closure does whatever it captures.
    pub(super) fn not_send(&self, ty: &Ty) -> bool {
        self.not_send_in(ty, &mut Vec::new())
    }

    fn not_send_in(&self, ty: &Ty, visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>) -> bool {
        match self.inference.resolve(ty) {
            Ty::Counted(Counter::Rc, _) | Ty::Weak(Counter::Rc, _) => true,
            Ty::Counted(Counter::Arc, inner) | Ty::Weak(Counter::Arc, inner) => {
                !self.shared_between_threads(&inner) || self.not_send_in(&inner, visiting)
            }
            Ty::Reference {
                mutable: false,
                target,
            } => !self.shared_between_threads(&target),
            Ty::Reference { target: inner, .. }
            | Ty::Boxed(inner)
            | Ty::Option(inner)
            | Ty::Array { element: inner, .. }
            | Ty::Reverse(inner)
            | Ty::Iter(inner, _)
            | Ty::Cell(_, inner) => self.not_send_in(&inner, visiting),
            Ty::Thread(kind, inner) => kind.sends() && self.not_send_in(&inner, visiting),
            Ty::Result(ok, err) => {
                self.not_send_in(&ok, visiting) || self.not_send_in(&err, visiting)
            }
            Ty::Tuple(members) | Ty::Keyed(_, members) => members
                .iter()
                .any(|member| self.not_send_in(member, visiting)),
            Ty::Script(id, arguments) => {
                let key = (id, arguments.clone());
                if visiting.contains(&key) {
                    return false;
                }
                visiting.push(key);
                let not_send = self
                    .field_types(id, &arguments)
                    .iter()
                    .any(|field| self.not_send_in(field, visiting));
                visiting.pop();
                not_send
            }
            _ => false,
        }
    }

    /// The types of the fields and variant payloads of the program's type
    /// `id` with `arguments`.
    fn field_types(&self, id: type_def::Id, arguments: &[Ty]) -> Vec<Ty> {
        let substitution = self.nominal_substitution(id, arguments);
        match self.models.definition(id) {
            Nominal::Struct { item, .. } => item
                .fields
                .iter()
                .map(|field| instantiate(&Ty::from_resolved(&field.ty), &substitution))
                .collect(),
            Nominal::Enum { item, .. } => item
                .variants
                .iter()
                .flat_map(|variant| variant.payload.iter())
                .map(|ty| instantiate(&Ty::from_resolved(ty), &substitution))
                .collect(),
        }
    }

    fn sync_in(&self, ty: &Ty, visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>) -> bool {
        match self.inference.resolve(ty) {
            // A `Mutex<T>` is `Sync` where `T` is `Send`, as a `Sender`
            // is; a `Receiver` never is.
            Ty::Cell(CellKind::Mutex, inner) => !self.not_send(&inner),
            Ty::Thread(kind, inner) => kind.syncs() && !self.not_send(&inner),
            Ty::Cell(..) | Ty::Counted(Counter::Rc, _) | Ty::Weak(Counter::Rc, _) => false,
            Ty::Counted(Counter::Arc, inner) | Ty::Weak(Counter::Arc, inner) => {
                self.sync_in(&inner, visiting) && !self.not_send(&inner)
            }

            Ty::Reference { target: inner, .. }
            | Ty::Boxed(inner)
            | Ty::Option(inner)
            | Ty::Array { element: inner, .. }
            | Ty::Reverse(inner)
            | Ty::Iter(inner, _) => self.sync_in(&inner, visiting),
            Ty::Result(ok, err) => self.sync_in(&ok, visiting) && self.sync_in(&err, visiting),
            Ty::Tuple(members) | Ty::Keyed(_, members) => {
                members.iter().all(|member| self.sync_in(member, visiting))
            }
            Ty::Script(id, arguments) => {
                let key = (id, arguments.clone());
                if visiting.contains(&key) {
                    return true;
                }
                visiting.push(key);
                let sync = self
                    .field_types(id, &arguments)
                    .iter()
                    .all(|field| self.sync_in(field, visiting));
                visiting.pop();
                sync
            }
            _ => true,
        }
    }
}

impl Parser<'_> {
    /// The keyword a local binding lowers to: `const` for an immutable one,
    /// which the runtime also keeps a copy of at the top level for the
    /// program's functions, unless its value holds an `Rc` or `Arc`, whose
    /// copy would count as a pointer.
    pub(super) fn binding_keyword(&self, mutable: bool, ty: &Ty) -> &'static str {
        if mutable || self.holds_counted(ty, &mut Vec::new()) {
            "let"
        } else {
            "const"
        }
    }

    fn holds_counted(&self, ty: &Ty, visiting: &mut Vec<(type_def::Id, Box<[Ty]>)>) -> bool {
        match self.inference.resolve(ty) {
            // A thread or channel handle is moved on, never copied: a copy of
            // a `Sender` would keep its channel open.
            Ty::Counted(..) | Ty::Weak(..) | Ty::Thread(..) => true,
            Ty::Boxed(inner)
            | Ty::Option(inner)
            | Ty::Array { element: inner, .. }
            | Ty::Reverse(inner)
            | Ty::Iter(inner, _)
            | Ty::Cell(_, inner) => self.holds_counted(&inner, visiting),
            Ty::Result(ok, err) => {
                self.holds_counted(&ok, visiting) || self.holds_counted(&err, visiting)
            }
            Ty::Tuple(members) | Ty::Keyed(_, members) => members
                .iter()
                .any(|member| self.holds_counted(member, visiting)),
            Ty::Script(id, arguments) => {
                let key = (id, arguments.clone());
                if visiting.contains(&key) {
                    return false;
                }
                visiting.push(key);
                let holds = self
                    .field_types(id, &arguments)
                    .iter()
                    .any(|field| self.holds_counted(field, visiting));
                visiting.pop();
                holds
            }
            _ => false,
        }
    }
}
