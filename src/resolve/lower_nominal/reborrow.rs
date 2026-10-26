//! A `&mut` local given new borrows (T107, D573.4): `r = &mut b`, or
//! `prev = r` of another `&mut` local. Rawr has no references at runtime,
//! and a `&mut` local that keeps one borrow is the place it borrows
//! (`places.rs`). One given new borrows refers to a place that only the
//! run decides, as in a loop that moves it on to the next local, so it
//! is a runtime local of its own holding which local it borrows, by that
//! local's key. Each use reads that local and stores it back
//! (`PlaceStep::Select`), so a write through `r` reaches the local it
//! refers to at that point, as in Rust.
//!
//! A use of `r` reaches any local in scope there that could be its
//! referent. NLL keeps a borrowed local alive while the borrow is used, so
//! the local `r` refers to is in scope at each use. A shadowed one is not
//! nameable, and the run stops there rather than reading another local.

use super::*;

/// What the run reports if a `&mut` local's referent is not nameable at a
/// use: only a shadowed local, which a later `let` hid.
const SHADOWED: &str =
    "the local a `&mut` local refers to is shadowed here: not supported in Rawr yet";

impl Parser<'_> {
    /// The key of the local declared at `declaration`, which a `&mut`
    /// local given new borrows holds while it refers to that local.
    fn reborrow_key(&mut self, declaration: Span) -> usize {
        if let Some(key) = self
            .reborrow_roots
            .iter()
            .position(|root| *root == declaration)
        {
            return key;
        }
        self.reborrow_roots.push(declaration);
        self.reborrow_roots.len() - 1
    }

    /// The key of the whole local `name`, when a `&mut` local may refer to
    /// it: a mutable local of the body that is not itself a place, or a
    /// `&mut` parameter, whose runtime local is the caller's place
    /// (`this`) or a copy of it given back.
    fn root_key(&mut self, name: &str) -> Option<usize> {
        let binding = self.bindings.get(name)?;
        let declaration = binding.declaration;
        let aliased = self
            .aliases
            .get(name)
            .is_some_and(|(aliased, _)| *aliased == declaration)
            || self.entry_aliases.contains_key(name)
            || self.cell_guards.contains_key(name);
        if aliased || !binding.mutable || !matches!(binding.kind, BindingKind::Local) {
            return None;
        }
        Some(self.reborrow_key(declaration))
    }

    /// The type of the value the runtime local of the root `ty` holds: a
    /// `&mut` parameter's is what it refers to.
    fn root_value(&self, ty: &Ty) -> Ty {
        match self.inference.resolve(ty) {
            Ty::Reference {
                mutable: true,
                target,
            } => *target,
            ty => ty,
        }
    }

    /// What a `&mut` local given new borrows holds for `value`, a `&mut`
    /// to a whole local (`&mut b`, or `r` of a `let r = &mut b;`) or
    /// another such local (`prev = r`): the runtime text of the key, and
    /// whether it is another such local's.
    pub(super) fn reborrow_source(&mut self, value: &ParsedExpr) -> Option<(String, bool)> {
        match value.lvalue.as_ref()? {
            Lvalue::Binding { binding, .. } => {
                let binding = binding.clone();
                self.root_key(&binding).map(|key| (key.to_string(), false))
            }
            Lvalue::Place { binding, place, .. } if place.as_str() == binding.as_str() => {
                let binding = binding.clone();
                self.root_key(&binding).map(|key| (key.to_string(), false))
            }
            Lvalue::Nested(nested) => match nested.steps.as_slice() {
                [PlaceStep::Select { selector, .. }] => Some((selector.to_string(), true)),
                _ => None,
            },
            _ => None,
        }
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

    /// `name` (declared at `declaration`, of the type `ty`, a `&mut` to
    /// `target`) is a `&mut` local given new borrows, whose runtime local
    /// of the same name holds its referent's key. `mutable` is whether it
    /// was declared `let mut`.
    pub(super) fn select_local(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        ty: Ty,
        target: Ty,
        mutable: bool,
    ) {
        self.bindings.insert(
            name.clone(),
            Binding {
                ty,
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
        let place = NestedLvalue {
            binding: name.clone(),
            base: origin::Fragment::plain(name.to_string()),
            steps: vec![PlaceStep::Select {
                selector: name.clone(),
                target: target.clone(),
                mutable,
            }],
            ty: target,
        };
        self.aliases
            .insert(name, (declaration, Lvalue::Nested(Box::new(place))));
    }

    /// The `&mut` local given new borrows that `target`, the left side of
    /// an assignment, is as a whole (not `*r`): its name and whether it
    /// was declared `let mut`.
    pub(super) fn rebound_selector(
        &self,
        target: &Ty,
        lvalue: &Lvalue,
    ) -> Option<(ImmutableString, bool)> {
        let Lvalue::Nested(nested) = lvalue else {
            return None;
        };
        let [PlaceStep::Select {
            selector, mutable, ..
        }] = nested.steps.as_slice()
        else {
            return None;
        };
        let binding = self.bindings.get(selector)?;
        (self.inference.resolve(target) == self.inference.resolve(&binding.ty))
            .then(|| (selector.clone(), *mutable))
    }

    /// `r = value;` of the `&mut` local given new borrows `selector`
    /// (`target` is the use of `r`, of the type `target_ty`): the statement giving it the key of
    /// what `value` borrows, and the value's span.
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
                    "giving the `&mut` local `{selector}` a borrow of a place other than a \
                     whole local: not supported in Rawr yet"
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

    /// The place a `&mut` local given new borrows refers to, read into a
    /// local after `read` (or not read when the statement `overwrite`s
    /// it): that local, and the statement storing it back into the place.
    pub(super) fn selected_place(
        &mut self,
        selector: &str,
        target: &Ty,
        read: &mut origin::Fragment,
        overwrite: bool,
    ) -> (origin::Fragment, origin::Fragment) {
        let arms = self.reborrow_arms(selector, target);
        let part = self.temporary("part");
        let mut text = format!("let {part} = ");
        if overwrite {
            text.push_str("();");
        } else {
            for (key, root) in &arms {
                text.push_str(&format!("if {selector} == {key} {{ {root} }} else "));
            }
            text.push_str(&format!("{{ throw {SHADOWED:?} }};"));
        }
        text.push(' ');
        read.push_text(text);
        let mut store = String::new();
        for (key, root) in &arms {
            store.push_str(&format!(
                "if {selector} == {key} {{ {root} = {part}; }} else "
            ));
        }
        store.push_str(&format!("{{ throw {SHADOWED:?}; }} "));
        (
            origin::Fragment::plain(part),
            origin::Fragment::plain(store),
        )
    }

    /// The locals in scope that a `&mut` local `selector` to `target` may
    /// refer to, with their keys: each mutable local of a type that
    /// unifies with `target`.
    fn reborrow_arms(&mut self, selector: &str, target: &Ty) -> Vec<(usize, ImmutableString)> {
        let candidates: Vec<(ImmutableString, Ty)> = self
            .bindings
            .iter()
            .filter(|(name, binding)| {
                name.as_str() != selector
                    && binding.mutable
                    && matches!(binding.kind, BindingKind::Local)
                    && !self
                        .aliases
                        .get(name.as_str())
                        .is_some_and(|(aliased, _)| *aliased == binding.declaration)
            })
            .map(|(name, binding)| (name.clone(), self.root_value(&binding.ty)))
            .collect();
        let mut arms = Vec::new();
        for (name, ty) in candidates {
            if self.unifies(target, &ty)
                && let Some(key) = self.root_key(&name)
            {
                arms.push((key, name));
            }
        }
        arms.sort();
        arms
    }
}
