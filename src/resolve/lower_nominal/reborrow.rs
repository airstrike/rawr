//! A `&mut` local given new borrows (T107, D573.4): `r = &mut b`, or
//! `prev = r` of another `&mut` local. A `&mut` local that keeps one
//! borrow is the place it borrows (`places.rs`). One given new borrows
//! refers to a place that only the run decides, as in a loop that moves it
//! on to the next local, so it is a runtime local of its own holding where
//! the local it borrows is: `r = __rawr_slot_of_b` (`KEYWORD_SLOT_OF`).
//! Each use is `__rawr_through_r`, which the evaluator resolves to that
//! local itself, as it resolves a local's name, so a call or a write
//! through `r` reaches the place and nothing is copied.
//!
//! Where a local is, is its index in the function's scope, or the receiver.
//! A local keeps its index while it lives, and NLL keeps a borrowed local
//! alive while the borrow is used. An async body's locals move between
//! scopes at each `.await`, so there a new borrow stays refused.

use super::*;
use crate::engine::{KEYWORD_SLOT_OF, KEYWORD_THROUGH};

impl Parser<'_> {
    /// Whether the whole local `name` may be what a `&mut` local refers to:
    /// a mutable local of the body that is not itself a place, a `&mut`
    /// parameter's copy, or the receiver `this`.
    fn reborrow_root(&self, name: &str) -> bool {
        let Some(binding) = self.bindings.get(name) else {
            return false;
        };
        let aliased = self
            .aliases
            .get(name)
            .is_some_and(|(aliased, _)| *aliased == binding.declaration)
            || self.entry_aliases.contains_key(name)
            || self.cell_guards.contains_key(name);
        !aliased && binding.mutable && matches!(binding.kind, BindingKind::Local)
    }

    /// The `&mut` local given new borrows that `place` reaches, by name.
    fn selector_of(&self, binding: &str, place: &origin::Fragment) -> bool {
        place
            .as_str()
            .strip_prefix(KEYWORD_THROUGH)
            .is_some_and(|name| name == binding)
            && self.bindings.get(binding).is_some_and(|local| {
                self.reborrow_selectors
                    .get(binding)
                    .is_some_and(|(declaration, _)| *declaration == local.declaration)
            })
    }

    /// What a `&mut` local given new borrows holds for `value`, a `&mut`
    /// to a whole local (`&mut b`, or `r` of a `let r = &mut b;`) or
    /// another such local (`prev = r`): the runtime text of where its
    /// referent is, and whether it is another such local's. `None` where a
    /// new borrow can't be held yet.
    pub(super) fn reborrow_source(&self, value: &ParsedExpr) -> Option<(String, bool)> {
        if self.closures.last().is_some_and(|scope| scope.asynchronous) {
            return None;
        }
        let root = match value.lvalue.as_ref()? {
            Lvalue::Binding { binding, .. } => binding,
            Lvalue::Place { binding, place, .. } if self.selector_of(binding, place) => {
                return Some((binding.to_string(), true));
            }
            Lvalue::Place { binding, place, .. } if place.as_str() == binding.as_str() => binding,
            _ => return None,
        };
        self.reborrow_root(root)
            .then(|| (format!("{KEYWORD_SLOT_OF}{root}"), false))
    }

    /// Whether the local `name`, just declared, is given a new value
    /// before its block ends: `name = ..` as a statement ahead.
    pub(super) fn rebound_later(&self, name: &str) -> bool {
        let mut ahead = self.input.clone();
        let mut depth = 0usize;
        let mut previous = Token::EOF;
        while let Some((token, _)) = ahead.next() {
            match &token {
                Token::EOF => return false,
                Token::LeftBrace | Token::MapStart => depth += 1,
                Token::RightBrace => match depth.checked_sub(1) {
                    Some(outer) => depth = outer,
                    None => return false,
                },
                Token::Identifier(identifier)
                    if identifier.as_str() == name
                        && !matches!(previous, Token::Period | Token::DoubleColon)
                        && matches!(ahead.peek(), Some((Token::Equals, _))) =>
                {
                    return true;
                }
                _ => {}
            }
            previous = token;
        }
        false
    }

    /// `name` (declared at `declaration`, of the type `ty`, a `&mut`) is a
    /// `&mut` local given new borrows, whose runtime local of the same name
    /// holds where its referent is. `mutable` is whether it was declared
    /// `let mut`.
    pub(super) fn select_local(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        ty: Ty,
        mutable: bool,
    ) {
        self.bindings.insert(
            name.clone(),
            Binding {
                ty: ty.clone(),
                // Writes through it need the place it refers to, which
                // was borrowed `&mut`; whether it may be given a new
                // borrow is `mutable`.
                mutable: true,
                available: true,
                moved_fields: Default::default(),
                declaration,
                kind: BindingKind::Local,
            },
        );
        self.reborrow_selectors
            .insert(name.clone(), (declaration, mutable));
        let place = Lvalue::Place {
            binding: name.clone(),
            ty,
            place: origin::Fragment::plain(format!("{KEYWORD_THROUGH}{name}")),
        };
        self.aliases.insert(name, (declaration, place));
    }

    /// The `&mut` local given new borrows that `lvalue`, the left side of
    /// an assignment of the type `target`, is as a whole (not `*r`): its
    /// name and whether it was declared `let mut`.
    pub(super) fn rebound_selector(
        &self,
        target: &Ty,
        lvalue: &Lvalue,
    ) -> Option<(ImmutableString, bool)> {
        let Lvalue::Place { binding, place, .. } = lvalue else {
            return None;
        };
        if !self.selector_of(binding, place) {
            return None;
        }
        let local = self.bindings.get(binding)?;
        let (_, mutable) = self.reborrow_selectors.get(binding)?;
        (self.inference.resolve(target) == self.inference.resolve(&local.ty))
            .then(|| (binding.clone(), *mutable))
    }

    /// `r = value;` of the `&mut` local given new borrows `selector`
    /// (`target` is the use of `r`, of the type `target_ty`): the statement
    /// giving it where what `value` borrows is, and the value's span.
    pub(super) fn rebind_selector(
        &mut self,
        target: Span,
        target_ty: &Ty,
        selector: ImmutableString,
        mutable: bool,
    ) -> Result<(origin::Fragment, Span), Error> {
        let declaration = self
            .bindings
            .get(&selector)
            .map_or(Span::NONE, |binding| binding.declaration);
        if !mutable {
            return Err(Error::ImmutableBinding {
                binding: selector,
                mutation: target,
                declaration,
            });
        }
        let borrowing =
            matches!(self.peek(), Token::Ampersand) && matches!(self.peek_second(), Token::Mut);
        let outer = self.mutable_borrow;
        if borrowing {
            self.mutable_borrow = Some(self.expression_stack.len() + 1);
        }
        let value_start = self.flow().value_start();
        let replacement = self.parse_expr();
        self.mutable_borrow = outer;
        let replacement = replacement?;
        self.require_type(target_ty, &replacement.ty, replacement.span)?;
        let Some((source, _)) = self.reborrow_source(&replacement) else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "giving the `&mut` local `{selector}` a borrow of a field or an element: \
                     not supported in Rawr yet"
                )
                .into(),
                source: replacement.span,
            });
        };
        let whole = span(target.start(), replacement.span.end());
        let into = self.flow().rebound(target.start(), whole);
        if let Some(into) = into {
            self.flow().claim(into, replacement.span, value_start, true);
        }
        self.effect(target.start());
        Ok((
            origin::Fragment::plain(format!("{selector} = {source};")),
            replacement.span,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Each time the runtime copies a `Tally`: the type has no `Clone` for
    /// Rawr code, so any copy is one standing in for a borrow.
    static COPIES: AtomicUsize = AtomicUsize::new(0);

    struct Tally(i64);

    impl Clone for Tally {
        fn clone(&self) -> Self {
            COPIES.fetch_add(1, Ordering::SeqCst);
            Self(self.0)
        }
    }

    fn facade() -> crate::host::facade::Definition {
        use crate::host::{facade, item, signature, type_ref};
        let mut builder = facade::Builder::new("tally", facade::Abi::new(1).unwrap()).unwrap();
        builder.type_::<Tally>("Tally", &[], &[]).unwrap();
        let tally = || type_ref::Type::registered("Tally").unwrap();
        let int = || type_ref::Type::builtin(type_ref::Builtin::I64);
        let mut new = signature::Builder::new();
        new.output(tally());
        builder
            .item(item::Definition::associated_function("Tally::new", new.finish()).unwrap())
            .unwrap();
        let mut add = signature::Builder::new();
        add.parameter(int()).output(type_ref::Type::unit());
        builder
            .item(
                item::Definition::method(
                    "Tally::add",
                    crate::receiver::Mode::Mutable,
                    add.finish(),
                )
                .unwrap(),
            )
            .unwrap();
        let mut total = signature::Builder::new();
        total.output(int());
        builder
            .item(
                item::Definition::method(
                    "Tally::total",
                    crate::receiver::Mode::Shared,
                    total.finish(),
                )
                .unwrap(),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn run(source: &str) -> i64 {
        let mut engine = crate::Engine::new();
        engine.register_host_facade(facade()).unwrap();
        let engine = crate::Shared::new(engine);
        let authority = engine
            .compile_authority(source)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut runtime = crate::host::runtime::Builder::new(facade());
        runtime
            .bind("tally::Tally::new", |_| Ok(crate::Dynamic::from(Tally(0))))
            .unwrap();
        runtime
            .bind("tally::Tally::add", |args| {
                let n = args[1].as_int().expect("an i64");
                args[0].write_lock::<Tally>().expect("a tally").0 += n;
                Ok(crate::Dynamic::UNIT)
            })
            .unwrap();
        runtime
            .bind("tally::Tally::total", |args| {
                Ok(crate::Dynamic::from_int(
                    args[0].read_lock::<Tally>().expect("a tally").0,
                ))
            })
            .unwrap();
        let mut registry = crate::host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();
        let run = authority
            .items()
            .find(|item| item.key().name() == "run")
            .unwrap()
            .clone();
        authority
            .invoke_bound(&run, &mut [], crate::Shared::new(registry))
            .unwrap_or_else(|error| panic!("{error}"))
            .as_int()
            .unwrap()
    }

    /// What `run` returns, and how many times the run copied a `Tally`.
    fn copies(body: &str) -> (i64, usize) {
        COPIES.store(0, Ordering::SeqCst);
        let value = run(&format!(
            "use tally::Tally;\npub fn run() -> i64 {{\n{body}\n}}"
        ));
        (value, COPIES.load(Ordering::SeqCst))
    }

    const TAIL: &str =
        "a.total() * 1000000 + b.total() * 10000 + low.len() as i64 * 10 + high.len() as i64";

    /// A `&mut` local given new borrows reaches a native value that has no
    /// `Clone`, and a large `Vec` of them used in a loop, as the place
    /// itself: the run copies a `Tally` exactly as often as the same calls
    /// made on the locals by name do. A copy of the referent at each use
    /// would add copies for each call through `r`, and one per element
    /// already pushed for each `push` through `v`.
    #[test]
    fn rebound_borrow_reaches_its_referent_without_a_copy() {
        let (through, through_copies) = copies(&format!(
            "let mut a = Tally::new();
            let mut b = Tally::new();
            let mut r = &mut a;
            for n in 0..10 {{
                r.add(n);
                if n == 4 {{
                    r = &mut b;
                }}
            }}
            let mut low = Vec::new();
            let mut high = Vec::new();
            let mut v = &mut low;
            for n in 0..4000 {{
                v.push(Tally::new());
                if n == 999 {{
                    v = &mut high;
                }}
            }}
            {TAIL}"
        ));
        let (named, named_copies) = copies(&format!(
            "let mut a = Tally::new();
            let mut b = Tally::new();
            for n in 0..10 {{
                if n <= 4 {{
                    a.add(n);
                }} else {{
                    b.add(n);
                }}
            }}
            let mut low = Vec::new();
            let mut high = Vec::new();
            for n in 0..4000 {{
                if n <= 999 {{
                    low.push(Tally::new());
                }} else {{
                    high.push(Tally::new());
                }}
            }}
            {TAIL}"
        ));
        assert_eq!(through, 10 * 1000000 + 35 * 10000 + 1000 * 10 + 3000);
        assert_eq!(named, through);
        assert_eq!(
            through_copies, named_copies,
            "a copy stood in for the borrow"
        );
    }
}
