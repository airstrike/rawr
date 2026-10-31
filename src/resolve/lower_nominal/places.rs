//! Rust's mutable iteration: `for x in &mut v`, `v.iter_mut()` (also
//! `.enumerate()`), a map's `iter_mut()` and `values_mut()`, and
//! `v.iter_mut().for_each(|x| ..)`. Rawr has no references at runtime, so
//! the loop walks the indexes and each element is a place: its binding
//! reads `v[i]`, and `*x = ..` (or `x.push(..)`) writes it there.

use super::*;
use crate::engine::{
    HELD_STEPS, KEYWORD_HELD, KEYWORD_LOCATE, KEYWORD_SHARE, KEYWORD_THIS, KEYWORD_THROUGH,
};

/// A `&mut` borrow of a place declared outside a closure that its body
/// uses (`Parser::held`): the borrow's name and declaration, the runtime
/// local holding the shared handle of the place's local (the place's path
/// is in that name followed by `HELD_STEPS`), and the place's path, taken
/// where the closure is made.
pub(super) struct Held {
    pub(super) name: ImmutableString,
    declaration: Span,
    holder: ImmutableString,
    located: origin::Fragment,
}

/// What a mutable loop walks: a `Vec`'s elements, or a map's values with
/// or without their keys.
enum Walk {
    Elements { element: Ty, enumerate: bool },
    Values { key: Ty, value: Ty, keys: bool },
}

impl Parser<'_> {
    /// The element `name` (declared at `declaration`) is the place
    /// `place` in the local `root` while the loop body is parsed.
    pub(super) fn alias_element(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        ty: Ty,
        place: origin::Fragment,
        root: ImmutableString,
    ) {
        self.bindings.insert(
            name.clone(),
            Binding {
                ty,
                mutable: false,
                available: true,
                declaration,
                kind: BindingKind::Local,
            },
        );
        self.note_alias_root(declaration, &root);
        self.aliases.insert(
            name,
            (
                declaration,
                Lvalue::Place {
                    binding: root,
                    ty: Ty::Unknown,
                    place,
                },
            ),
        );
    }

    /// The alias declared at `declaration` is a place in the local `root`
    /// as it is now.
    fn note_alias_root(&mut self, declaration: Span, root: &str) {
        if let Some(root) = self.bindings.get(root) {
            self.alias_roots.insert(declaration, root.declaration);
        }
    }

    /// A use of `name` (declared at `declaration`) that reaches the local
    /// holding its place by that local's name: a `&mut` local that is the
    /// place it borrows. Once a new local shadows that name, the name
    /// reaches the new local, which would be a wrong answer (T125), so the
    /// use is refused. A `let r = &mut b;` whose `b` is shadowed ahead
    /// holds where `b` is instead (`reborrow.rs`), as a `&mut` pattern
    /// binding holds its part's path (`PlacePattern`).
    pub(super) fn reject_shadowed_root(
        &self,
        name: &str,
        declaration: Span,
        span: Span,
    ) -> Result<(), Error> {
        let moved = |root: &str, at: Span| {
            self.bindings
                .get(root)
                .is_some_and(|binding| binding.declaration != at)
        };
        let alias = self
            .aliases
            .get(name)
            .filter(|(aliased, _)| *aliased == declaration)
            .and_then(|(_, lvalue)| {
                let at = self.alias_roots.get(&declaration)?;
                Some((lvalue.binding().clone(), *at))
            });
        match alias.filter(|(root, at)| moved(root, *at)) {
            Some((root, _)) => Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "a borrow of `{root}` used after a new local named `{root}` shadows it: \
                     not supported in Rawr yet"
                )
                .into(),
                source: span,
            }),
            None => Ok(()),
        }
    }

    /// `name` (declared at `declaration`, of type `ty`) is a `&mut` borrow
    /// of the place `lvalue`, which a call gave: `let r = c.slot(0);`.
    pub(super) fn borrow_lvalue(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        ty: Ty,
        lvalue: Lvalue,
    ) {
        self.bindings.insert(
            name.clone(),
            Binding {
                ty,
                mutable: false,
                available: true,
                declaration,
                kind: BindingKind::Local,
            },
        );
        self.note_alias_root(declaration, lvalue.binding());
        self.aliases.insert(name, (declaration, lvalue));
    }

    /// `name` (declared at `declaration`, of type `ty`) is a `&mut` borrow
    /// of the place whose path the runtime local `holder` holds
    /// (`place_located`): each use reaches it there, though a new local
    /// takes the name of the place's local.
    pub(super) fn borrow_located(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        ty: Ty,
        holder: &str,
    ) {
        self.bindings.insert(
            name.clone(),
            Binding {
                ty,
                mutable: false,
                available: true,
                declaration,
                kind: BindingKind::Local,
            },
        );
        self.alias_through(name, declaration, holder);
    }

    /// `name` (declared at `declaration`) is the place whose path, or whose
    /// local's slot, the runtime local `holder` holds.
    fn alias_through(&mut self, name: ImmutableString, declaration: Span, holder: &str) {
        let place = origin::Fragment::plain(format!("{KEYWORD_THROUGH}{holder}"));
        let lvalue = Lvalue::Place {
            binding: name.clone(),
            ty: Ty::Unknown,
            place,
        };
        self.aliases.insert(name.clone(), (declaration, lvalue));
        self.alias_by_itself(&name);
    }

    /// Whether the body being read is an async one, whose locals move
    /// between scopes at each `.await`, so where a local is doesn't last.
    pub(super) fn in_async_body(&self) -> bool {
        self.closures.last().is_some_and(|scope| scope.asynchronous)
    }

    /// The alias `name` reaches its place by where its local is, not by the
    /// local's name, which a new local may take: the place's local was
    /// checked mutable when it was borrowed, and a write through the alias
    /// needs only the `&mut` it is, so the alias is its own local.
    fn alias_by_itself(&mut self, name: &ImmutableString) {
        if let Some((_, Lvalue::Place { binding, .. })) = self.aliases.get_mut(name) {
            *binding = name.clone();
        }
        if let Some(binding) = self.bindings.get_mut(name) {
            if !binding.mutable {
                self.undeclared_mut.insert(binding.declaration);
            }
            binding.mutable = true;
        }
    }

    /// `name` (declared at `declaration`) is a `&mut` borrow of `place` in
    /// the local `root`: `let r = &mut v;`, `let ref mut r = v;`. Rawr has
    /// no references at runtime, so each use of `name` is the place; the
    /// borrow checker (`borrows.rs`) ends the borrow where rustc does.
    pub(super) fn borrow_place(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        target: Ty,
        place: origin::Fragment,
        root: ImmutableString,
    ) {
        let ty = Ty::Reference {
            mutable: true,
            target: Box::new(target),
        };
        self.alias_element(name, declaration, ty, place, root);
    }

    /// A use of a loop element bound to its place, or `None`.
    pub(super) fn aliased_element(
        &mut self,
        name: &ImmutableString,
        declaration: Span,
        ty: &Ty,
        span: Span,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some((aliased, lvalue)) = self.aliases.get(name).cloned() else {
            return Ok(None);
        };
        let root = lvalue.binding().clone();
        if aliased != declaration {
            return Ok(None);
        }
        // A borrow a `move` closure took is used after the move.
        if let Some(binding) = self.bindings.get(name.as_str())
            && !binding.available
            && self.peek() != Token::Equals
        {
            return Err(Error::MovedValue {
                moved: self
                    .moved_at
                    .get(&declaration)
                    .copied()
                    .unwrap_or(declaration),
                name: name.clone(),
                reference: span,
                previous_iteration: false,
            });
        }
        // A closure using the borrow reaches the place through what it
        // captured where it was made.
        if let Some(closure) = self
            .closures
            .iter()
            .position(|scope| scope.outer.get(name) == Some(&declaration))
        {
            let holder = self.held(closure, name, declaration, &lvalue, span)?;
            let source = origin::Fragment::plain(format!("{KEYWORD_HELD}{holder}"));
            let mut value = self.operator_expr(source.clone(), ty.clone(), span);
            value.lvalue = Some(Lvalue::Place {
                binding: root,
                ty: ty.clone(),
                place: source,
            });
            return Ok(Some(value));
        }
        let (source, lvalue) = match lvalue {
            Lvalue::Place { place, .. } => (
                place.clone(),
                Lvalue::Place {
                    binding: root,
                    ty: ty.clone(),
                    place,
                },
            ),
            lvalue => {
                let mut nested = as_nested(&lvalue);
                nested.ty = ty.clone();
                let (read, place, store) = self.nested_parts(&nested);
                let value = self.temporary("value");
                let mut source = origin::Fragment::plain("({ ");
                source.push(read);
                source.push_text(format!("let {value} = "));
                source.push(place);
                source.push_text("; ");
                source.push(store);
                source.push_text(format!("{value} }})"));
                (source, Lvalue::Nested(Box::new(nested)))
            }
        };
        let mut value = self.operator_expr(source, ty.clone(), span);
        value.lvalue = Some(lvalue);
        Ok(Some(value))
    }

    /// After `for pattern in`: a mutable loop's head, read when it is one
    /// of the forms this module lowers; `None`, reading nothing, otherwise.
    /// The walk's root is its place's text (a `&mut` parameter's is
    /// `this`) and the binding owning it.
    #[allow(clippy::type_complexity)]
    fn mutable_walk(
        &mut self,
    ) -> Result<Option<(String, ImmutableString, Walk, Option<origin::Fragment>)>, Error> {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..9)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        let named = |token: &Token, text: &str| matches!(token, Token::Identifier(name) if name.as_str() == text);
        let (root, consumed, method, enumerate) = match &tokens[..] {
            [Token::Ampersand, Token::Mut, Token::Identifier(root), Token::LeftBrace | Token::LeftBracket, ..] => {
                (root.as_str().into(), 3, "iter_mut", false)
            }
            [Token::Identifier(root), Token::Period, method, Token::Unit, Token::Period, enumerate, Token::Unit, Token::LeftBrace, ..]
                if named(method, "iter_mut") && named(enumerate, "enumerate") =>
            {
                (root.as_str().into(), 7, "iter_mut", true)
            }
            [Token::Identifier(root), Token::Period, Token::Identifier(method), Token::Unit, Token::LeftBrace, ..]
                if matches!(method.as_str(), "iter_mut" | "values_mut") =>
            {
                let method = if method.as_str() == "iter_mut" {
                    "iter_mut"
                } else {
                    "values_mut"
                };
                (root.as_str().into(), 4, method, false)
            }
            _ => return self.mutable_place_walk(),
        };
        let root: ImmutableString = root;
        let Some(binding) = self.bindings.get(&root) else {
            return Ok(None);
        };
        let (place, owner) = match self.aliases.get(&root) {
            Some((
                _,
                Lvalue::Place {
                    place,
                    binding: owner,
                    ..
                },
            )) if place.as_str() == "this" => (place.as_str().to_string(), owner.clone()),
            Some(_) => return Ok(None),
            None => (root.to_string(), root.clone()),
        };
        let ty = match self.inference.resolve(&binding.ty) {
            Ty::Reference {
                mutable: true,
                target,
            } => *target,
            ty => ty,
        };
        let walk = match (ty, method) {
            (Ty::Array { element, .. }, "iter_mut") => Walk::Elements {
                element: *element,
                enumerate,
            },
            (Ty::Keyed(kind, arguments), method) if kind.is_map() && !enumerate => Walk::Values {
                key: arguments[0].clone(),
                value: arguments[1].clone(),
                keys: method == "iter_mut",
            },
            _ => return Ok(None),
        };
        for _ in 0..consumed {
            self.next();
        }
        let position = self.position();
        self.require_mutable_binding(&owner, point_span(position))?;
        // `&mut v[a..b]`: the indexes of the range, with Rust's panics.
        let range = if matches!(self.peek(), Token::LeftBracket) {
            self.next();
            let bounds = self.parse_range_or_expr()?;
            self.expect(Token::RightBracket, "']' after a slice range")?;
            let Err((low, high, inclusive)) = bounds else {
                return Err(self.unsupported("`&mut v[i]` as a loop's iterable"));
            };
            let mut bound = |bound: Option<ParsedExpr>, open: String| -> Result<_, Error> {
                Ok(match bound {
                    Some(bound) => {
                        self.require_type(&Ty::Int(Int::Usize), &bound.ty, bound.span)?;
                        bound.rendered_source()
                    }
                    None => origin::Fragment::plain(open),
                })
            };
            let low = bound(low, "0".into())?;
            let high = bound(high, format!("{place}.len()"))?;
            let (lo, hi) = (self.temporary("low"), self.temporary("high"));
            let mut range = origin::Fragment::plain(format!("({{ let {lo} = "));
            range.push(low);
            range.push_text(format!("; let {hi} = "));
            range.push(high);
            if inclusive {
                range.push_text(" + 1");
            }
            range.push_text(format!(
                "; if {hi} > {place}.len() {{ throw \"range end index \" + {hi} + \" out of range \
                 for slice of length \" + {place}.len(); }} if {lo} > {hi} {{ throw \"slice index \
                 starts at \" + {lo} + \" but ends at \" + {hi}; }} {lo}..{hi} }})"
            ));
            Some(range)
        } else {
            None
        };
        Ok(Some((place, owner, walk, range)))
    }

    /// A mutable loop over a place below a field or an element
    /// (`&mut self.items`, `self.items.iter_mut()`, `v[0].cells.iter_mut()
    /// .enumerate()`, `g.map.values_mut()`): the place is read as an
    /// expression, its fields copied out around the loop and stored back
    /// (`nested_place`); `None`, reading nothing, for any other head.
    #[allow(clippy::type_complexity)]
    fn mutable_place_walk(
        &mut self,
    ) -> Result<Option<(String, ImmutableString, Walk, Option<origin::Fragment>)>, Error> {
        let mut ahead = self.input.clone();
        let mut head = Vec::new();
        let mut depth = 0usize;
        loop {
            let Some((token, _)) = ahead.next() else {
                return Ok(None);
            };
            match token {
                Token::EOF | Token::SemiColon => return Ok(None),
                Token::LeftBrace if depth == 0 => break,
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            head.push(token);
        }
        let named = |token: &Token, text: &str| matches!(token, Token::Identifier(name) if name.as_str() == text);
        let (path, method, enumerate) = match &head[..] {
            [Token::Ampersand, Token::Mut, path @ ..] => (path.len(), "iter_mut", false),
            [path @ .., Token::Period, method, Token::Unit, Token::Period, enumerate, Token::Unit]
                if named(method, "iter_mut") && named(enumerate, "enumerate") =>
            {
                (path.len(), "iter_mut", true)
            }
            [path @ .., Token::Period, method, Token::Unit]
                if named(method, "iter_mut") || named(method, "values_mut") =>
            {
                let method = if named(method, "iter_mut") {
                    "iter_mut"
                } else {
                    "values_mut"
                };
                (path.len(), method, false)
            }
            _ => return Ok(None),
        };
        if path == 0 {
            return Ok(None);
        }
        // Take the head, give back only the place's tokens.
        let skip = usize::from(matches!(head.first(), Some(Token::Ampersand)));
        let taken = (0..head.len()).map(|_| self.next()).collect::<Vec<_>>();
        let start = if skip == 1 { 2 } else { 0 };
        self.splice_tokens(taken[start..start + path].to_vec());
        let outer = std::mem::replace(&mut self.no_struct, true);
        let value = self.parse_postfix();
        self.no_struct = outer;
        let value = value?;
        let (place, owner) = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => (binding.to_string(), binding.clone()),
            Some(Lvalue::Place { binding, place, .. }) => {
                (place.as_str().to_string(), binding.clone())
            }
            Some(Lvalue::Field(field)) => {
                let nested = NestedLvalue {
                    binding: field.binding.clone(),
                    base: origin::Fragment::plain(field.binding.to_string()),
                    steps: vec![PlaceStep::Field { slot: field.slot }],
                    ty: field.ty.clone(),
                };
                (
                    self.nested_place(&nested).as_str().to_string(),
                    field.binding.clone(),
                )
            }
            Some(Lvalue::Nested(nested)) => {
                let nested = (**nested).clone();
                (
                    self.nested_place(&nested).as_str().to_string(),
                    nested.binding.clone(),
                )
            }
            _ => {
                return Err(self.unsupported("a mutable loop over a value that is not a place"));
            }
        };
        let mutable = self.bindings.get(&owner).is_some_and(|binding| {
            binding.mutable || matches!(binding.ty, Ty::Reference { mutable: true, .. })
        });
        if !mutable {
            self.require_mutable_binding(&owner, value.span)?;
        }
        let ty = match self.inference.resolve(&value.ty) {
            Ty::Reference { target, .. } => self.inference.resolve(&target),
            ty => ty,
        };
        let walk = match (ty, method) {
            (Ty::Array { element, .. }, "iter_mut") => Walk::Elements {
                element: *element,
                enumerate,
            },
            (Ty::Keyed(kind, arguments), method) if kind.is_map() && !enumerate => Walk::Values {
                key: arguments[0].clone(),
                value: arguments[1].clone(),
                keys: method == "iter_mut",
            },
            _ => return Err(self.unsupported("a mutable loop over this value")),
        };
        Ok(Some((place, owner, walk, None)))
    }

    /// `for x in &mut v { .. }` and its kin (`mutable_walk`), or `None`.
    pub(super) fn parse_mutable_for(
        &mut self,
        name: ImmutableString,
        declaration: Span,
        pattern: Option<&PatternSyntax>,
        start: Span,
        label: Option<ImmutableString>,
    ) -> Result<Option<ParsedExpr>, Error> {
        let Some((root, owner, walk, range)) = self.mutable_walk()? else {
            return Ok(None);
        };
        let index = self.temporary("index");
        let outer = self.bindings.clone();
        let outer_aliases = self.aliases.clone();
        let mutable = |ty: Ty| Ty::Reference {
            mutable: true,
            target: Box::new(ty),
        };
        let shared = |ty: Ty| Ty::Reference {
            mutable: false,
            target: Box::new(ty),
        };
        let mut head = String::new();
        // When a `let` ahead shadows the local walked, each iteration takes
        // where the local is first, and the element is reached from there.
        let shadowed = !self.in_async_body() && self.root_shadowed_later(&root);
        let base = if shadowed {
            let holder = self.temporary("walked");
            head += &format!("let {holder} = {}{root}; ", crate::engine::KEYWORD_SLOT_OF);
            format!("{KEYWORD_THROUGH}{holder}")
        } else {
            root.clone()
        };
        let (element_place, element_ty, first, first_ty) = match walk {
            Walk::Elements { element, enumerate } => (
                format!("{base}[{index}]"),
                mutable(element),
                enumerate.then(|| index.clone()),
                Ty::Int(Int::Usize),
            ),
            Walk::Values { key, value, keys } => (
                format!("{base}[{index}][1]"),
                mutable(value),
                keys.then(|| format!("{base}[{index}][0]")),
                shared(key),
            ),
        };
        // The element alone, or `(i, x)` / `(k, v)` with the element second.
        // An element pattern other than a name binds the element's parts,
        // as `let pattern = &mut element;` (`bind_let_pattern`).
        let mut parts = None;
        let element = match (pattern, &first) {
            (None, None) => Some((name, declaration)),
            (Some(PatternSyntax::Tuple { members, .. }), Some(first)) if members.len() == 2 => {
                match &members[0] {
                    PatternSyntax::Binding {
                        name,
                        span,
                        mutable,
                        ..
                    } => {
                        let keyword = if *mutable { "let" } else { "const" };
                        head += &format!("{keyword} {name} = {first}; ");
                        self.bindings.insert(
                            name.clone(),
                            Binding {
                                ty: first_ty,
                                mutable: *mutable,
                                available: true,
                                declaration: *span,
                                kind: BindingKind::Local,
                            },
                        );
                    }
                    PatternSyntax::Discard { .. } => {}
                    other => {
                        self.bindings = outer;
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "a pattern other than a name in a mutable loop".into(),
                            source: other.span(),
                        });
                    }
                }
                match &members[1] {
                    PatternSyntax::Binding { name, span, .. } => Some((name.clone(), *span)),
                    PatternSyntax::Discard { .. } => None,
                    other => {
                        parts = Some(other.clone());
                        None
                    }
                }
            }
            (Some(other), None) => {
                parts = Some(other.clone());
                None
            }
            _ => {
                self.bindings = outer;
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a pattern other than a name or a pair in a mutable loop".into(),
                    source: declaration,
                });
            }
        };
        if let Some((name, declaration)) = element {
            self.alias_element(
                name.clone(),
                declaration,
                element_ty.clone(),
                origin::Fragment::plain(element_place.clone()),
                owner.clone(),
            );
            // Reached by where the local is, the element outlives its name.
            if shadowed {
                self.alias_roots.remove(&declaration);
                self.alias_by_itself(&name);
            }
        }
        let mut head = origin::Fragment::plain(head);
        if let Some(parts) = parts {
            let target = match &element_ty {
                Ty::Reference { target, .. } => (**target).clone(),
                ty => ty.clone(),
            };
            let place = origin::Fragment::plain(element_place.clone());
            let subject = ParsedExpr {
                prelude: origin::Fragment::default(),
                source: place.clone(),
                ty: element_ty.clone(),
                span: parts.span(),
                lvalue: Some(Lvalue::Place {
                    binding: owner.clone(),
                    ty: target,
                    place,
                }),
                outcome: crate::host::usage::Outcome::Conservative,
                evaluation: crate::host::usage::Evaluation::Required,
            };
            let output = std::mem::take(&mut self.output);
            let last_type = self.last_type.clone();
            let last_expression = self.last_expression.take();
            let bound = self.bind_let_pattern(
                parts.span().start(),
                &parts,
                subject,
                None,
                None,
                parts.span().end(),
                self.field_entries.len(),
            );
            let statement = std::mem::replace(&mut self.output, output);
            self.last_type = last_type;
            self.last_expression = last_expression;
            if let Err(error) = bound {
                self.bindings = outer;
                return Err(error);
            }
            head.push(statement);
            head.push_text(" ");
        }
        let header = match range {
            Some(range) => {
                let mut header = origin::Fragment::plain(format!("for {index} in "));
                header.push(range);
                header
            }
            None => origin::Fragment::plain(format!("for {index} in 0..{root}.len()")),
        };
        let wrap = Some((head, origin::Fragment::default()));
        let looped = self.parse_loop_body(header, start, label, false, wrap, Some(&outer));
        let inner = std::mem::replace(&mut self.bindings, outer);
        self.aliases = outer_aliases;
        self.keep_moves_from(&inner);
        looped.map(Some)
    }

    /// `Some(&mut v[slot])`, or `None` when `locate` sets `slot` below 0:
    /// what `v.get_mut(i)` and `v.iter_mut().find(f)` are. The value has
    /// the element's place, which a pattern's `&mut` binding reaches
    /// (`pattern_place`). `slot` is a local declared before the statement;
    /// `locate` sets it each time the value is evaluated.
    pub(super) fn optional_element(
        &mut self,
        value: &ParsedExpr,
        element: &Ty,
        slot: &str,
        locate: origin::Fragment,
        expression: Span,
    ) -> Result<ParsedExpr, Error> {
        self.optional_place(value, element, slot, locate, expression, "")
    }

    /// [`Self::optional_element`] of the part `member` (`"[1]"`, a map
    /// entry's value) of the element at `slot`: `map.get_mut(&k)`.
    pub(super) fn optional_place(
        &mut self,
        value: &ParsedExpr,
        element: &Ty,
        slot: &str,
        locate: origin::Fragment,
        expression: Span,
        member: &str,
    ) -> Result<ParsedExpr, Error> {
        let index = origin::Fragment::plain(slot);
        let lvalue = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => Some(Lvalue::Place {
                binding: binding.clone(),
                ty: element.clone(),
                place: origin::Fragment::plain(format!("{binding}[{slot}]{member}")),
            }),
            Some(Lvalue::Place { binding, place, .. }) => {
                let mut place = place.clone();
                place.push_text(format!("[{slot}]{member}"));
                Some(Lvalue::Place {
                    binding: binding.clone(),
                    ty: element.clone(),
                    place,
                })
            }
            Some(lvalue @ (Lvalue::Field(_) | Lvalue::Nested(_))) => {
                let mut lvalue = nested_step(lvalue, PlaceStep::Index(index), element.clone());
                if let Some(part) = member.strip_prefix('[').and_then(|m| m.strip_suffix(']')) {
                    lvalue = nested_step(
                        &lvalue,
                        PlaceStep::Index(part.to_string().into()),
                        element.clone(),
                    );
                }
                Some(lvalue)
            }
            _ => None,
        };
        let Some(lvalue) = lvalue else {
            return Err(
                self.unsupported("`get_mut` or `iter_mut().find` of a value that is not a place")
            );
        };
        let owner = lvalue.binding().clone();
        let mutable = self.bindings.get(&owner).is_some_and(|binding| {
            binding.mutable || matches!(binding.ty, Ty::Reference { mutable: true, .. })
        });
        if !mutable {
            self.require_mutable_binding(&owner, value.span)?;
        }
        self.field_entries.push((
            origin::Fragment::plain(format!("let {slot} = -1; ")),
            origin::Fragment::default(),
        ));
        let receiver = value.rendered_source().surrounded("(", ")");
        let mut element_at = receiver;
        element_at.push_text(format!("[{slot}]{member}"));
        let some = self.prelude_op(PreludeOp::Some, [element_at]);
        let none = self.prelude_op(PreludeOp::None, []);
        let missing = origin::Fragment::plain(format!("{slot} < 0"));
        self.optional_subjects
            .insert(expression, (locate.clone(), missing));
        let mut source = origin::Fragment::plain("{ ");
        source.push(locate);
        source.push_text(format!("if {slot} < 0 {{ "));
        source.push(none);
        source.push_text(" } else { ");
        source.push(some);
        source.push_text(" } }");
        let ty = Ty::Option(Box::new(Ty::Reference {
            mutable: true,
            target: Box::new(element.clone()),
        }));
        let mut parsed = self.operator_expr(source.surrounded("(", ")"), ty, expression);
        parsed.lvalue = Some(lvalue);
        Ok(parsed)
    }

    /// `v.iter_mut().find(f)`: `Some` of the first element's place `f`
    /// holds for, or `None` (`optional_element`).
    fn parse_iter_mut_find(
        &mut self,
        value: &ParsedExpr,
        element: Ty,
    ) -> Result<ParsedExpr, Error> {
        for _ in 0..3 {
            self.next();
        }
        let item = Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Reference {
                mutable: true,
                target: Box::new(element.clone()),
            }),
        };
        self.closure_hints = vec![Some(Box::new([item.clone()]) as Box<[Ty]>)];
        let arguments = self.parse_call_arguments("`find` arguments");
        self.closure_hints = Vec::new();
        let (mut arguments, end) = arguments?;
        let expression = span(value.span.start(), end);
        let [predicate] = arguments.as_mut_slice() else {
            return Err(Error::InvalidMethodCall {
                owner: "Iterator".into(),
                method: "find".into(),
                reason: format!("expects 1 argument(s), found {}", arguments.len()).into(),
                call: expression,
            });
        };
        let slot = self.temporary("index");
        let at = self.temporary("at");
        let receiver = value.rendered_source().surrounded("(", ")");
        let mut candidate = receiver.clone();
        candidate.push_text(format!("[{at}]"));
        let call = self.callable_call(predicate, &[item], &Ty::Bool, vec![candidate])?;
        let mut locate = origin::Fragment::plain(format!("{slot} = -1; for {at} in 0.."));
        locate.push(receiver);
        locate.push_text(".len() { if ");
        locate.push(call);
        locate.push_text(format!(" {{ {slot} = {at}; break; }} }} "));
        self.optional_element(value, &element, &slot, locate, expression)
    }

    /// `.for_each(path)` after `iter_mut()`, a function named by a path as
    /// Rust accepts one (`Counter::add`, `bump`), reads as
    /// `.for_each(|x| path(x))`.
    fn function_path_for_each(&mut self) {
        let mut ahead = self.input.clone();
        let head: Vec<Token> = (0..4)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        if !(head[0] == Token::Unit
            && head[1] == Token::Period
            && matches!(&head[2], Token::Identifier(name) if name.as_str() == "for_each")
            && head[3] == Token::LeftParen)
        {
            return;
        }
        let mut path = Vec::new();
        let mut angle = 0_usize;
        loop {
            let Some((token, position)) = ahead.next() else {
                return;
            };
            match token {
                Token::RightParen if angle == 0 => break,
                Token::Identifier(_) | Token::DoubleColon | Token::Comma => {}
                Token::LessThan => angle += 1,
                Token::GreaterThan if angle > 0 => angle -= 1,
                _ => return,
            }
            path.push((token, position));
        }
        if !matches!(path.first(), Some((Token::Identifier(_), _))) {
            return;
        }
        let at = path[0].1;
        let mut tokens = Vec::new();
        for _ in 0..4 {
            tokens.push(self.next());
        }
        for _ in 0..=path.len() {
            self.next();
        }
        let parameter = Token::Identifier(Box::new("__rawr_each".into()));
        tokens.extend([
            (Token::Pipe, at),
            (parameter.clone(), at),
            (Token::Pipe, at),
        ]);
        tokens.extend(path);
        tokens.extend([
            (Token::LeftParen, at),
            (parameter, at),
            (Token::RightParen, at),
            (Token::RightParen, at),
        ]);
        self.splice_tokens(tokens);
    }

    /// `v.iter_mut()`: followed by `.for_each(|x| body)`, the body run on
    /// each element's place; a mutable loop is `for x in v.iter_mut()`.
    pub(super) fn parse_iter_mut(
        &mut self,
        value: &ParsedExpr,
        element: Ty,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        {
            let mut ahead = self.input.clone();
            let tokens: Vec<Token> = (0..4)
                .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
                .collect();
            if tokens[0] == Token::Unit
                && tokens[1] == Token::Period
                && matches!(&tokens[2], Token::Identifier(name) if name.as_str() == "find")
                && tokens[3] == Token::LeftParen
            {
                return self.parse_iter_mut_find(value, element);
            }
        }
        let (root, owner) = match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => (binding.to_string(), binding.clone()),
            Some(Lvalue::Place { binding, place, .. }) => {
                (place.as_str().to_string(), binding.clone())
            }
            // A field or a nested place: its fields copied out around the
            // statement and stored back, as a mutable loop over it is.
            Some(Lvalue::Field(field)) => {
                let nested = NestedLvalue {
                    binding: field.binding.clone(),
                    base: origin::Fragment::plain(field.binding.to_string()),
                    steps: vec![PlaceStep::Field { slot: field.slot }],
                    ty: field.ty.clone(),
                };
                (
                    self.nested_place(&nested).as_str().to_string(),
                    field.binding.clone(),
                )
            }
            Some(Lvalue::Nested(nested)) => {
                let nested = (**nested).clone();
                (
                    self.nested_place(&nested).as_str().to_string(),
                    nested.binding.clone(),
                )
            }
            _ => {
                return Err(self.unsupported("`iter_mut` on a value that is not a place"));
            }
        };
        self.function_path_for_each();
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        if !(tokens[0] == Token::Unit
            && tokens[1] == Token::Period
            && matches!(&tokens[2], Token::Identifier(name) if name.as_str() == "for_each")
            && tokens[3] == Token::LeftParen
            && tokens[4] == Token::Pipe)
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`iter_mut()` other than `for x in v.iter_mut()` or \
                            `v.iter_mut().for_each(|x| ..)`"
                    .into(),
                source: member_span,
            });
        }
        self.require_mutable_binding(&owner, value.span)?;
        for _ in 0..5 {
            self.next();
        }
        let (name, declaration) = self.expect_name("`for_each` closure parameter")?;
        self.expect(Token::Pipe, "'|' after the `for_each` closure parameter")?;
        let index = self.temporary("index");
        let outer = self.bindings.clone();
        let outer_aliases = self.aliases.clone();
        self.alias_element(
            name,
            declaration,
            Ty::Reference {
                mutable: true,
                target: Box::new(element),
            },
            origin::Fragment::plain(format!("{root}[{index}]")),
            owner,
        );
        let body = self.parse_inline_closure_body();
        self.bindings = outer;
        self.aliases = outer_aliases;
        let body = body?;
        let end = self.expect(Token::RightParen, "')' after `for_each`'s closure")?;
        let mut source = origin::Fragment::plain(format!("{{ for {index} in 0..{root}.len() {{ "));
        source.push(body.rendered_source());
        source.push_text("; } }");
        Ok(self.operator_expr(source, Ty::Unit, span(value.span.start(), end)))
    }

    /// `if let Some(v) = map.get_mut(&k) { .. } else { .. }` (and
    /// `v.get_mut(i)` of a `Vec`): the body sees the value as its place in
    /// the collection; `None` when the condition is not that form.
    pub(super) fn parse_get_mut_if(&mut self, if_span: Span) -> Result<Option<ParsedExpr>, Error> {
        let mut ahead = self.input.clone();
        let tokens: Vec<(Token, Position)> = (0..10)
            .map(|_| ahead.next().unwrap_or((Token::EOF, Position::NONE)))
            .collect();
        let named = |token: &Token, text: &str| matches!(token, Token::Identifier(name) if name.as_str() == text);
        let (Token::Identifier(name), Token::Identifier(root)) = (&tokens[3].0, &tokens[6].0)
        else {
            return Ok(None);
        };
        if !(tokens[0].0 == Token::Let
            && named(&tokens[1].0, "Some")
            && tokens[2].0 == Token::LeftParen
            && tokens[4].0 == Token::RightParen
            && tokens[5].0 == Token::Equals
            && tokens[7].0 == Token::Period
            && named(&tokens[8].0, "get_mut")
            && tokens[9].0 == Token::LeftParen)
        {
            return Ok(None);
        }
        let (name, root): (ImmutableString, ImmutableString) =
            (name.as_str().into(), root.as_str().into());
        let declaration = point_span(tokens[3].1);
        let Some(binding) = self.bindings.get(&root) else {
            return Ok(None);
        };
        // A `Vec`'s element at an index, or a map's value at a key.
        let collection = binding.ty.clone();
        let (vec, arguments) = match self.inference.resolve(&binding.ty) {
            Ty::Keyed(kind, arguments) if kind.is_map() => (false, arguments),
            Ty::Array {
                element,
                length: None,
            } => (true, Box::from([Ty::Int(Int::Usize), *element])),
            _ => return Ok(None),
        };
        for _ in 0..10 {
            self.next();
        }
        self.require_mutable_binding(&root, point_span(tokens[6].1))?;
        let mut key = self.parse_expr()?;
        self.expect(Token::RightParen, "')' after `get_mut`'s key")?;
        let expected = if vec {
            arguments[0].clone()
        } else {
            Ty::Reference {
                mutable: false,
                target: Box::new(arguments[0].clone()),
            }
        };
        if !(self.inference.resolve(&arguments[0]).is_text()
            && self.inference.resolve(&key.ty).is_text())
        {
            self.coerce_expr(&expected, &mut key)?;
        }
        if !matches!(self.peek(), Token::LeftBrace) {
            return Err(self.unsupported("`if` consequent without a block"));
        }
        let index = self.temporary("index");
        let outer = self.bindings.clone();
        let outer_aliases = self.aliases.clone();
        self.alias_element(
            name,
            declaration,
            Ty::Reference {
                mutable: true,
                target: Box::new(arguments[1].clone()),
            },
            origin::Fragment::plain(if vec {
                format!("{root}[{index}]")
            } else {
                format!("{root}[{index}][1]")
            }),
            root.clone(),
        );
        let consequent = self.parse_match_action();
        self.bindings = outer.clone();
        self.aliases = outer_aliases;
        let consequent = consequent?;
        let alternative = if !matches!(self.peek(), Token::Else) {
            self.require_type(&Ty::Unit, &consequent.value.ty, consequent.value.span)?;
            Branch {
                value: self.literal("()", Ty::Unit, consequent.syntax),
                syntax: consequent.syntax,
            }
        } else if matches!(self.next_after_else(), Token::If) {
            let nested = self.next().1;
            let value = self.parse_if(point_span(nested))?;
            Branch {
                syntax: value.span,
                value,
            }
        } else {
            if !matches!(self.peek(), Token::LeftBrace) {
                return Err(self.unsupported("`if` alternative without a block"));
            }
            self.parse_match_action()?
        };
        self.bindings = outer;
        let mut branches = [consequent, alternative];
        let output = self.coerce_branches(
            &mut branches
                .iter_mut()
                .map(|branch| &mut branch.value)
                .collect::<Vec<_>>(),
        )?;
        let [consequent, alternative] = branches;
        let mut source = origin::Fragment::plain(format!("{{ let {index} = "));
        if vec {
            source.push(key.rendered_source());
            source.push_text(format!("; if {index} < {root}.len() "));
        } else {
            let slot = self.push_op(self.keyed_op_for(PreludeOp::MapSlot, &collection));
            source.push_text(format!("{root}.{slot}("));
            source.push(key.rendered_source());
            source.push_text(format!("); if {index} >= 0 "));
        }
        source.push(consequent.value.rendered_block());
        source.push_text(" else ");
        source.push(alternative.value.rendered_block());
        source.push_text(" }");
        let span = span(if_span.start(), alternative.syntax.end());
        Ok(Some(self.operator_expr(
            source.surrounded("(", ")"),
            output,
            span,
        )))
    }
}

/// A pattern with `&mut` bindings matched on a place: `match &mut p`,
/// `Some(ref mut v)`, `if let Some(x) = v.get_mut(i)`. Rawr has no
/// references at runtime, so the scrutinee holds the place's path
/// (`KEYWORD_LOCATE`), the switch matches the place where it is, and each
/// `&mut` binding holds the path of its part. A use of the binding is
/// `__rawr_through_` of that path, which the evaluator resolves to the part
/// itself, so nothing is copied out of the place or written back.
pub(super) struct PlacePattern {
    /// The scrutinee's value: the place's path, or for `Some(&mut place)`
    /// (`optional`), `()` for `None`.
    pub(super) locate: origin::Fragment,
    optional: bool,
    /// Each `&mut` binding's name and declaration, and the runtime local
    /// holding its part's path, which no `let` of the script shadows.
    parts: Vec<(ImmutableString, Span, ImmutableString)>,
    /// A place behind a borrow guard, which each use of a binding reaches
    /// through the guard again: the path is that of a copy of the guard's
    /// target, matched once.
    guard: Option<NestedLvalue>,
}

impl PlacePattern {
    /// Whether the subject is `Some(&mut place)`.
    pub(super) fn optional(&self) -> bool {
        self.optional
    }

    /// What the switch is on: the place, through the scrutinee's path.
    pub(super) fn operand(&self, scrutinee: &str) -> String {
        let through = if self.optional {
            crate::engine::KEYWORD_SOME_THROUGH
        } else {
            KEYWORD_THROUGH
        };
        format!("{through}{scrutinee}")
    }

    /// The runtime name of the binding `name` declared at `at`.
    pub(super) fn runtime_name(
        place: Option<&Self>,
        name: &ImmutableString,
        at: Span,
    ) -> ImmutableString {
        place
            .and_then(|place| {
                place
                    .parts
                    .iter()
                    .find(|(bound, declaration, _)| bound == name && *declaration == at)
            })
            .map_or_else(|| name.clone(), |(_, _, holder)| holder.clone())
    }
}

/// The local a place's chain starts at, as `__rawr_locate`'s root: a local
/// holding a path or a slot (`__rawr_through_x`), or a local by where it
/// is (`-1` for the receiver). An async body's locals move between scopes
/// at each `.await`, so there (`by_name`) a local is found by its name.
/// `None` for a chain that starts at something else.
fn located_root(base: &str, by_name: bool) -> Option<String> {
    let local = base.split('[').next()?;
    if let Some(holder) = local.strip_prefix(KEYWORD_THROUGH) {
        return Some(holder.to_string());
    }
    let named = !local.is_empty()
        && local
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !named {
        return None;
    }
    Some(if by_name && local != KEYWORD_THIS {
        format!("{local:?}")
    } else {
        format!("{}{local}", crate::engine::KEYWORD_SLOT_OF)
    })
}

impl Parser<'_> {
    /// For a pattern matched on a `&mut` place, or on a place whose pattern
    /// binds `ref mut`: the place's path and the bindings reaching parts of
    /// it, with each such binding of `proof` made a [`NominalPattern::Part`]
    /// held by a runtime local of its own. `None` for another subject, or
    /// no such binding.
    pub(super) fn pattern_place(
        &mut self,
        subject: &ParsedExpr,
        proof: &mut NominalPattern,
        bindings: &[PatternBinding],
    ) -> Result<Option<PlacePattern>, Error> {
        let borrowed = matches!(
            self.inference.resolve(&subject.ty),
            Ty::Reference { mutable: true, .. }
        );
        if !borrowed && subject.lvalue.is_none() {
            return Ok(None);
        }
        let mut parts = Vec::new();
        for binding in bindings {
            if matches!(
                self.inference.resolve(&binding.ty),
                Ty::Reference { mutable: true, .. }
            ) {
                let holder = self.temporary("part").into();
                parts.push((binding.name.clone(), binding.span, holder));
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        // `match self` in a `&mut self` method matches the receiver.
        let receiver = subject.lvalue.is_none()
            && self.closures.is_empty()
            && subject.source.as_str() == "this"
            && self.bindings.get("this").is_some_and(|binding| {
                binding.mutable && matches!(binding.kind, BindingKind::Receiver { .. })
            });
        let lvalue = if receiver {
            Lvalue::Binding {
                binding: "this".into(),
                ty: Ty::Unknown,
            }
        } else {
            match subject.lvalue.clone() {
                Some(lvalue) => lvalue,
                None => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: "binding parts of a `&mut` value that is not a local, a \
                                    field or an element"
                            .into(),
                        source: subject.span,
                    })
                }
            }
        };
        self.require_mutable_binding(&lvalue.binding().clone(), subject.span)?;
        // `Some(&mut place)` (`optional_element`): the pattern's `Some` is
        // matched on the place.
        let optional = !borrowed
            && matches!(
                self.inference.resolve(&subject.ty),
                Ty::Option(inner) if matches!(*inner, Ty::Reference { mutable: true, .. })
            );
        let unreached = |source: Span| Error::UnsupportedNominalSyntax {
            construct: "a `&mut` pattern binding of a place not reached from a local, or \
                        through a `RefCell` borrowed below its start: not supported in Rawr yet"
                .into(),
            source,
        };
        // A range of elements is matched by a slice pattern, not bound whole.
        if matches!(lvalue, Lvalue::Slice(_))
            && !matches!(proof, NominalPattern::Slice { .. } | NominalPattern::Or(_))
        {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a `&mut` binding of a whole range of elements: not supported in \
                            Rawr yet"
                    .into(),
                source: subject.span,
            });
        }
        // A place behind a borrow guard (a `RefCell` borrow, an imported
        // method's guard, a facade accessor) is reached only through the
        // guard, which rawr can't keep: each use of a binding goes through
        // the guard again, and the pattern is matched on a copy of the
        // guard's target.
        let mut guard = match &lvalue {
            Lvalue::Nested(nested)
                if nested.steps.iter().any(|step| {
                    matches!(step, PlaceStep::Cell { .. } | PlaceStep::Accessor { .. })
                }) =>
            {
                Some((**nested).clone())
            }
            _ => None,
        };
        let located = match &mut guard {
            Some(guarded) => self
                .guarded_located(guarded)
                .ok_or_else(|| unreached(subject.span))?,
            None => self
                .place_located(&lvalue)
                .ok_or_else(|| unreached(subject.span))?,
        };
        let locate = if optional {
            let (find, none) = self
                .optional_subjects
                .get(&subject.span)
                .cloned()
                .ok_or_else(|| unreached(subject.span))?;
            let mut locate = origin::Fragment::plain("{ ");
            locate.push(find);
            locate.push_text("if ");
            locate.push(none);
            locate.push_text(" { () } else { ");
            locate.push(located);
            locate.push_text(" } }");
            locate
        } else {
            located
        };
        if let Err(construct) = mark_parts(proof, &parts, false) {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!("{construct}: not supported in Rawr yet").into(),
                source: subject.span,
            });
        }
        Ok(Some(PlacePattern {
            locate,
            optional,
            parts,
            guard,
        }))
    }

    /// The path of a copy of the target behind a borrow guard (`nested`),
    /// read through the guard once, for a pattern to be matched on. A
    /// `RefCell` borrow the place starts with is made once and held to the
    /// statement's end, as Rust holds it while the pattern's bindings live:
    /// `nested` becomes the place through that held borrow, which each use
    /// of a binding reaches. `None` for a borrow made below the place's
    /// start, which each use would make again.
    fn guarded_located(&mut self, nested: &mut NestedLvalue) -> Option<origin::Fragment> {
        let copy = self.temporary("guarded");
        let mut before = format!("let {copy} = (); ");
        let mut after = String::new();
        let mut located = origin::Fragment::plain("{ ");
        if let Some(PlaceStep::Cell {
            release: release @ true,
            ..
        }) = nested.steps.first_mut()
        {
            let (held, made) = (self.temporary("held"), self.temporary("made"));
            let release_op = self.push_op(Op::Prelude(PreludeOp::CellRelease));
            before += &format!("let {held} = (); let {made} = false; ");
            after = format!("if {made} {{ {release_op}({held}); }} ");
            located.push_text(format!("{held} = "));
            located.push(std::mem::replace(
                &mut nested.base,
                origin::Fragment::plain(held.as_str()),
            ));
            located.push_text(format!("; {made} = true; "));
            *release = false;
        }
        if nested
            .steps
            .iter()
            .any(|step| matches!(step, PlaceStep::Cell { release: true, .. }))
        {
            return None;
        }
        self.field_entries.push((
            origin::Fragment::plain(before),
            origin::Fragment::plain(after),
        ));
        let (read, place, store) = self.nested_parts(nested);
        let root = located_root(&copy, self.in_async_body()).expect("a local's name");
        located.push(read);
        located.push_text(format!("{copy} = "));
        located.push(place);
        located.push_text("; ");
        located.push(store);
        located.push_text(format!("{KEYWORD_LOCATE}({copy}, {root}) }}"));
        Some(located)
    }

    /// The path of the place `lvalue` (`KEYWORD_LOCATE`): the chain of
    /// indexes reaching it below its local, a struct's field by its slot,
    /// a function's `&mut` output by the path the function gives, and a
    /// range of elements last. `None` for a place no path reaches.
    pub(super) fn place_located(&mut self, lvalue: &Lvalue) -> Option<origin::Fragment> {
        let by_name = self.in_async_body();
        self.place_located_by(lvalue, by_name)
    }

    /// `place_located`, with an async body's locals found `by_name`.
    fn place_located_by(&mut self, lvalue: &Lvalue, by_name: bool) -> Option<origin::Fragment> {
        let (mut chain, steps, range) = match lvalue {
            Lvalue::Binding { binding, .. } => {
                (origin::Fragment::plain(binding.as_str()), &[][..], None)
            }
            Lvalue::Place { place, .. } => (place.clone(), &[][..], None),
            Lvalue::Field(field) => (field.place(), &[][..], None),
            Lvalue::Nested(nested) => (nested.base.clone(), &nested.steps[..], None),
            Lvalue::Slice(slice) => (slice.base.clone(), &[][..], Some(&**slice)),
        };
        let mut root = located_root(chain.as_str(), by_name)?;
        let mut located = origin::Fragment::plain("{ ");
        let index = |chain: &mut origin::Fragment, index: origin::Fragment| {
            chain.push_text("[");
            chain.push(index);
            chain.push_text("]");
        };
        for step in steps {
            match step {
                PlaceStep::Field { slot, .. } | PlaceStep::Member(slot) => {
                    index(&mut chain, slot.to_string().into());
                }
                PlaceStep::Index(at)
                | PlaceStep::Path {
                    path: at,
                    call: None,
                } => {
                    index(&mut chain, at.clone());
                }
                // The call is made on the place it borrows, reached by its
                // path, and gives the path below it.
                PlaceStep::Path {
                    path,
                    call: Some((before, after)),
                } => {
                    let reached = self.temporary("reached");
                    located.push_text(format!("let {reached} = {KEYWORD_LOCATE}("));
                    located.push(chain);
                    located.push_text(format!(", {root}); let {} = ", path.as_str()));
                    located.push(before.clone());
                    located.push_text(format!("{KEYWORD_THROUGH}{reached}"));
                    located.push(after.clone());
                    located.push_text("; ");
                    chain = origin::Fragment::plain(format!("{KEYWORD_THROUGH}{reached}"));
                    index(&mut chain, path.clone());
                    root = reached;
                }
                PlaceStep::Setup(setup) => {
                    located.push(setup.clone());
                    located.push_text(" ");
                }
                PlaceStep::Accessor { .. } | PlaceStep::Cell { .. } => return None,
            }
        }
        if let Some(range) = range {
            let mut bounds = range.low.clone();
            bounds.push_text(if range.inclusive { "..=" } else { ".." });
            bounds.push(range.high.clone());
            index(&mut chain, bounds);
        }
        located.push_text(format!("{KEYWORD_LOCATE}("));
        located.push(chain);
        located.push_text(format!(", {root}) }}"));
        Some(located)
    }

    /// The runtime local through which the closure `closure` (by its depth)
    /// reaches the place of the `&mut` borrow `name` (declared at
    /// `declaration` outside it, the place `lvalue`): a `&mut` local, a
    /// function's `&mut` parameter, a `&mut` pattern binding or a mutable
    /// loop's element. A closure's body runs in a scope of its own, so it
    /// can't name the place's local or its slot. Where the closure is made,
    /// the place's path is taken and its local made shared, as a closure's
    /// capture makes a local it uses; the closure captures that handle and
    /// the path, and each use reaches the place itself below the handle.
    pub(super) fn held(
        &mut self,
        closure: usize,
        name: &ImmutableString,
        declaration: Span,
        lvalue: &Lvalue,
        span: Span,
    ) -> Result<ImmutableString, Error> {
        if let Some(held) = self.closures[closure]
            .held
            .iter()
            .find(|held| held.name == *name && held.declaration == declaration)
        {
            return Ok(held.holder.clone());
        }
        let unreached = |construct: &str| Error::UnsupportedNominalSyntax {
            construct: format!("a closure using `{name}`, {construct}: not supported in Rawr yet")
                .into(),
            source: span,
        };
        if self.closures[closure..]
            .iter()
            .any(|scope| scope.asynchronous)
        {
            return Err(unreached("a `&mut` borrow of a place, in an async block"));
        }
        if matches!(lvalue, Lvalue::Slice(_)) {
            return Err(unreached("a `&mut` borrow of a range of elements"));
        }
        let by_name = self.closures[..closure]
            .last()
            .is_some_and(|scope| scope.asynchronous);
        let located = self
            .place_located_by(lvalue, by_name)
            .ok_or_else(|| unreached("a `&mut` borrow of a place behind a borrow guard"))?;
        let holder: ImmutableString = self.temporary("held").into();
        self.closures[closure].held.push(Held {
            name: name.clone(),
            declaration,
            holder: holder.clone(),
            located,
        });
        Ok(holder)
    }

    /// The closure `source`, made where each place its body reaches
    /// (`held`) is taken: `{ let h_at = path; let h = __rawr_share(h_at);
    /// source }`.
    pub(super) fn with_held(held: &[Held], source: origin::Fragment) -> origin::Fragment {
        if held.is_empty() {
            return source;
        }
        let mut made = origin::Fragment::plain("{ ");
        for held in held {
            let holder = &held.holder;
            made.push_text(format!("let {holder}{HELD_STEPS} = "));
            made.push(held.located.clone());
            made.push_text(format!(
                "; let {holder} = {KEYWORD_SHARE}({holder}{HELD_STEPS}); "
            ));
        }
        made.push(source);
        made.push_text(" }");
        made
    }

    /// Each `&mut` binding of a place pattern is a `&mut` local that is
    /// its part: a use reaches the part through the path its holder holds.
    pub(super) fn bind_parts(&mut self, place: &PlacePattern) {
        for (name, declaration, holder) in &place.parts {
            let Some(guard) = &place.guard else {
                self.alias_through(name.clone(), *declaration, holder);
                continue;
            };
            // Below the guard's target: the path past the copy's local.
            let step = PlaceStep::Path {
                path: origin::Fragment::plain(format!("{holder}.extract(1)")),
                call: None,
            };
            let guarded = Lvalue::Nested(Box::new(guard.clone()));
            let lvalue = place_step(&guarded, step, Ty::Unknown);
            self.note_alias_root(*declaration, lvalue.binding());
            self.aliases.insert(name.clone(), (*declaration, lvalue));
        }
    }

    /// A match's catch-all `&mut` binding `name` (declared at
    /// `declaration`) of the whole place, whose path `scrutinee` holds.
    pub(super) fn bind_whole(
        &mut self,
        place: &PlacePattern,
        name: ImmutableString,
        declaration: Span,
        scrutinee: &str,
    ) {
        match &place.guard {
            Some(guard) => {
                self.note_alias_root(declaration, &guard.binding);
                let lvalue = Lvalue::Nested(Box::new(guard.clone()));
                self.aliases.insert(name, (declaration, lvalue));
            }
            None => self.alias_through(name, declaration, scrutinee),
        }
    }
}

/// Make each binding of `pattern` that `parts` names a `Part` held by its
/// runtime local, or what refuses that: a binding in a slice's rest (`rest`
/// is whether `pattern` is in one), a copy no path reaches.
fn mark_parts(
    pattern: &mut NominalPattern,
    parts: &[(ImmutableString, Span, ImmutableString)],
    rest: bool,
) -> Result<(), &'static str> {
    let holder = |name: &ImmutableString| {
        parts
            .iter()
            .find(|(bound, ..)| bound == name)
            .map(|(.., holder)| holder.clone())
    };
    match pattern {
        NominalPattern::Binding { name, expected } => {
            let Some(holder) = holder(name) else {
                return Ok(());
            };
            if rest {
                return Err("a `&mut` binding in a slice pattern's `..` rest");
            }
            *pattern = NominalPattern::Part {
                name: holder,
                expected: expected.clone(),
            };
            Ok(())
        }
        NominalPattern::At {
            name,
            pattern,
            part,
        } => {
            if let Some(holder) = holder(name) {
                if rest {
                    return Err("a `&mut` binding in a slice pattern's `..` rest");
                }
                *name = holder;
                *part = true;
            }
            mark_parts(pattern, parts, rest)
        }
        NominalPattern::Part { .. }
        | NominalPattern::Discard { .. }
        | NominalPattern::Literal(_)
        | NominalPattern::Range { .. }
        | NominalPattern::Option(None) => Ok(()),
        NominalPattern::Option(Some(child)) | NominalPattern::Result(Ok(child) | Err(child)) => {
            mark_parts(child, parts, rest)
        }
        NominalPattern::Enum(child) => child
            .payload
            .iter_mut()
            .try_for_each(|child| mark_parts(child, parts, rest)),
        NominalPattern::Tuple(members) | NominalPattern::Struct(members) => members
            .iter_mut()
            .try_for_each(|member| mark_parts(member, parts, rest)),
        NominalPattern::Slice {
            before,
            rest: middle,
            after,
        } => {
            before
                .iter_mut()
                .chain(after.iter_mut())
                .try_for_each(|member| mark_parts(member, parts, rest))?;
            match middle {
                Some(middle) => mark_parts(middle, parts, true),
                None => Ok(()),
            }
        }
        NominalPattern::Or(alternatives) => alternatives
            .iter_mut()
            .try_for_each(|alternative| mark_parts(alternative, parts, rest)),
    }
}

/// `place.get(path)` (a copy of the value at `path` below the place) and
/// `place.set(path, value)` (the value stored there). Each step of a path
/// is an index into an array (a `Vec`, a tuple, a map's `[key, value]`
/// entry), a struct's field slot, an enum's payload slot, or `0` for an
/// option's `Some` value: what a script function's `&mut` output names
/// below the place it was called on.
pub(super) fn path_invoke(op: PreludeOp, args: &mut FnCallArgs) -> RawrResult {
    let path = args[1]
        .as_array_ref()
        .map_err(|_| invariant("a place's path is not an array at runtime"))?
        .clone();
    if matches!(op, PreludeOp::PathSet) {
        let value = args[2].take();
        // A native place is stored into where it is (D651): its own
        // handle, given back after the place was used, stores nothing.
        return at_path(args[0], &path, |leaf| match crate::place::get(leaf) {
            Some(place)
                if crate::place::get(&value).is_some_and(|other| place.same_storage(&other)) =>
            {
                Ok(Dynamic::UNIT)
            }
            Some(place) => place.invoke(crate::place::Operation::Store, &mut [value]),
            None => {
                *leaf = value;
                Ok(Dynamic::UNIT)
            }
        })?;
    }
    at_path(args[0], &path, |leaf| match crate::place::get(leaf) {
        Some(place) if place.copies() => place.invoke(crate::place::Operation::Copy, &mut []),
        _ => Ok(leaf.clone()),
    })?
}

fn at_path<R>(
    value: &mut Dynamic,
    path: &[Dynamic],
    visit: impl FnOnce(&mut Dynamic) -> R,
) -> Result<R, Box<EvalAltResult>> {
    let Some((step, rest)) = path.split_first() else {
        return Ok(visit(value));
    };
    let index = step
        .as_int()
        .map_err(|_| invariant("a place's path step is not an integer"))?;
    // Below a native sequence, the path names a native element's own
    // place, which a read copies only when it is `Copy` (D651).
    #[cfg(feature = "rust-import")]
    if let Some(place) = crate::place::get(value) {
        let mut element = place.project(
            usize::try_from(index).map_err(|_| invariant("a place's path step is negative"))?,
        )?;
        for step in rest {
            let at = step
                .as_int()
                .ok()
                .and_then(|at| usize::try_from(at).ok())
                .ok_or_else(|| invariant("a place's path step is not an index"))?;
            element = element.project(at)?;
        }
        return Ok(visit(&mut Dynamic::from(element)));
    }
    let slot = |len: usize| usize::try_from(index).ok().filter(|slot| *slot < len);
    if value.is_array() {
        let mut array = value
            .as_array_mut()
            .map_err(|_| invariant("an array place changed type"))?;
        let len = array.len();
        let Some(slot) = slot(len) else {
            return Err(EvalAltResult::ErrorArrayBounds(len, index, Position::NONE).into());
        };
        return at_path(&mut array[slot], rest, visit);
    }
    match &mut value.0 {
        Union::Struct(value, ..) => {
            let fields = value.fields_mut();
            let slot = slot(fields.len())
                .ok_or_else(|| invariant("a field slot is outside its schema"))?;
            at_path(&mut fields[slot], rest, visit)
        }
        Union::Enum(value, ..) => {
            let payload = value.payload_mut();
            let slot = slot(payload.len())
                .ok_or_else(|| invariant("a payload slot is outside its variant"))?;
            at_path(&mut payload[slot], rest, visit)
        }
        _ => {
            let mut option = value
                .write_lock::<crate::option::Value>()
                .ok_or_else(|| invariant("a place's path steps into a value without parts"))?;
            let inner = option
                .as_mut()
                .ok_or_else(|| invariant("a place's path steps into `None`"))?;
            at_path(inner, rest, visit)
        }
    }
}

/// What a script function's output borrows of the place it is called on:
/// `&mut T`, or `Option<&mut T>`, with `T`.
#[derive(Clone)]
pub(super) enum PlaceOutput {
    Reference(Ty),
    Optional(Ty),
}

/// The text standing for the receiver's place in a call whose output
/// borrows it, replaced by the place wherever the place is reached.
pub(super) const RECEIVER_PLACE: &str = "__rawr_receiver_place";

impl Parser<'_> {
    /// `value`, returned from a function whose `&mut` output borrows its
    /// receiver (`place_output`), as the path to its place below the
    /// receiver: `&mut self.counts[i]` gives `[0, i]`, a struct field by
    /// its slot. An `Option<&mut T>` gives `None`, or `Some` of the path.
    /// A block-like value (`block_like`: `if`, `match`, a block) gave its
    /// paths at each of its own tails and arms.
    pub(super) fn returned_place(
        &mut self,
        mut value: ParsedExpr,
        block_like: bool,
    ) -> Result<ParsedExpr, Error> {
        if !self.place_output || !self.closures.is_empty() {
            return Ok(value);
        }
        let Some(output) = self.place_output(&value.ty) else {
            return Ok(value);
        };
        let place = value.lvalue.clone().or_else(|| {
            (value.source.as_str() == "this").then(|| Lvalue::Binding {
                binding: "this".into(),
                ty: value.ty.clone(),
            })
        });
        let Some(place) = place else {
            if block_like || self.is_none_value(&value) {
                return Ok(value);
            }
            return Err(Error::UnsupportedNominalSyntax {
                construct: "returning a `&mut` that is not a place below `self` (or a free \
                            function's first `&mut` parameter)"
                    .into(),
                source: value.span,
            });
        };
        let path = self.place_path(&place, value.span)?;
        value.source = match output {
            PlaceOutput::Reference(_) => path,
            PlaceOutput::Optional(_) => {
                let option = self.temporary("option");
                let test = self.prelude_op(PreludeOp::IsSome, [option.as_str().into()]);
                let some = self.prelude_op(PreludeOp::Some, [path]);
                let none = self.prelude_op(PreludeOp::None, []);
                let mut source = origin::Fragment::plain(format!("({{ let {option} = "));
                source.push(value.rendered_source());
                source.push_text("; if ");
                source.push(test);
                source.push_text(" { ");
                source.push(some);
                source.push_text(" } else { ");
                source.push(none);
                source.push_text(" } })");
                source
            }
        };
        value.prelude = origin::Fragment::default();
        value.lvalue = None;
        Ok(value)
    }

    /// The statement making the call of `nested`'s step `index`, a
    /// `PlaceStep::Path` with its call, on the place before it: `let path =
    /// ..;`, with the places on the way read and stored back around it.
    fn call_statement(&mut self, nested: &NestedLvalue, index: usize) -> origin::Fragment {
        let PlaceStep::Path {
            path,
            call: Some((before, after)),
        } = &nested.steps[index]
        else {
            unreachable!("a call statement is made for a call's step")
        };
        let (path, before, after) = (path.clone(), before.clone(), after.clone());
        let prefix = NestedLvalue {
            steps: nested.steps[..index].to_vec(),
            ..nested.clone()
        };
        let (read, place, store) = self.nested_parts(&prefix);
        let given = self.temporary("given");
        let mut statement = origin::Fragment::plain(format!("let {} = {{ ", path.as_str()));
        statement.push(read);
        statement.push_text(format!("let {given} = "));
        statement.push(before);
        statement.push(place);
        statement.push(after);
        statement.push_text("; ");
        statement.push(store);
        statement.push_text(format!("{given} }}; "));
        statement
    }

    /// `let r = c.slot(0);`, `let x = v.get_mut(0).unwrap();`: the
    /// statement making the calls that reach `nested`'s place, and running
    /// its setup, once; and the place they reached, which `r` names after
    /// it.
    pub(super) fn hold_place(&mut self, nested: NestedLvalue) -> (origin::Fragment, NestedLvalue) {
        let mut statement = origin::Fragment::default();
        let mut held = NestedLvalue {
            steps: Vec::new(),
            ..nested.clone()
        };
        for step in nested.steps {
            match step {
                PlaceStep::Setup(setup) => {
                    statement.push(setup);
                    statement.push_text(" ");
                }
                PlaceStep::Path {
                    path,
                    call: Some(call),
                } => {
                    held.steps.push(PlaceStep::Path {
                        path: path.clone(),
                        call: Some(call),
                    });
                    statement.push(self.call_statement(&held, held.steps.len() - 1));
                    held.steps.pop();
                    held.steps.push(PlaceStep::Path { path, call: None });
                }
                step => held.steps.push(step),
            }
        }
        (statement, held)
    }

    /// Whether `nested` is reached by a call or a setup, which a held
    /// borrow of it makes once (`hold_place`).
    pub(super) fn reached_once(nested: &NestedLvalue) -> bool {
        nested.steps.iter().any(|step| {
            matches!(
                step,
                PlaceStep::Path { call: Some(_), .. } | PlaceStep::Setup(_)
            )
        })
    }

    /// Whether `value` is `None` as written.
    fn is_none_value(&self, value: &ParsedExpr) -> bool {
        let text = value.source.as_str();
        text.strip_prefix(SENTINEL)
            .and_then(|rest| rest.strip_suffix("()"))
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| index.checked_sub(self.op_offset))
            .is_some_and(|index| matches!(self.ops.get(index), Some(Op::Prelude(PreludeOp::None))))
    }

    /// The runtime path of `place` below the receiver `this`: an array of
    /// its steps, after any statements its steps run (a call giving a path
    /// of its own, an `unwrap()`'s `Option`).
    fn place_path(&mut self, place: &Lvalue, at: Span) -> Result<origin::Fragment, Error> {
        let unsupported = |construct: &str| Error::UnsupportedNominalSyntax {
            construct: construct.to_string().into(),
            source: at,
        };
        if place.binding().as_str() != "this" {
            return Err(unsupported(
                "returning a `&mut` borrowed from a value other than `self` (or a free \
                 function's first `&mut` parameter)",
            ));
        }
        if matches!(place, Lvalue::Slice(_)) {
            return Err(unsupported("returning a `&mut` slice range"));
        }
        let nested = as_nested(place);
        let Some(groups) = place_groups(nested.base.as_str()) else {
            return Err(unsupported(
                "returning a `&mut` to this place below `self`, such as a map's `entry(k).or_insert(v)`",
            ));
        };
        let mut statements = origin::Fragment::default();
        let mut pieces: Vec<origin::Fragment> = Vec::new();
        let mut current: Vec<origin::Fragment> = Vec::new();
        for (start, end) in groups {
            let mut group = nested.base.clone();
            let mut inner = group.split_off(start);
            inner.truncate(end - start);
            current.push(inner);
        }
        for (index, step) in nested.steps.iter().enumerate() {
            match step {
                PlaceStep::Field { slot, .. } => current.push(slot.to_string().into()),
                PlaceStep::Index(index) => current.push(index.clone()),
                PlaceStep::Member(member) => current.push(member.to_string().into()),
                PlaceStep::Path { path, call } => {
                    if call.is_some() {
                        statements.push(self.call_statement(&nested, index));
                    }
                    let elements = std::mem::take(&mut current);
                    pieces.push(origin::Fragment::join(elements, ", ").surrounded("[", "]"));
                    pieces.push(path.clone());
                }
                PlaceStep::Setup(setup) => {
                    statements.push(setup.clone());
                    statements.push_text(" ");
                }
                PlaceStep::Cell { .. } | PlaceStep::Accessor { .. } => {
                    return Err(unsupported(
                        "returning a `&mut` through a `RefCell` borrow or a library type's \
                         accessor",
                    ))
                }
            }
        }
        if !current.is_empty() || pieces.is_empty() {
            pieces.push(origin::Fragment::join(current, ", ").surrounded("[", "]"));
        }
        let mut path = statements.surrounded("({ ", "");
        path.push(origin::Fragment::join(pieces, " + "));
        path.push_text(" })");
        Ok(path)
    }
}

/// The index groups of a place's text below `this`: `this[a][b]` (or
/// `(this)[a][b]`) gives the ranges of `a` and `b`; `None` for another
/// place.
fn place_groups(text: &str) -> Option<Vec<(usize, usize)>> {
    let mut at = if text.starts_with("(this)") {
        "(this)".len()
    } else if text.starts_with("this") {
        "this".len()
    } else {
        return None;
    };
    let bytes = text.as_bytes();
    let mut groups = Vec::new();
    while at < bytes.len() {
        if bytes[at] != b'[' {
            return None;
        }
        let start = at + 1;
        let mut depth = 0usize;
        let mut index = start;
        loop {
            match *bytes.get(index)? {
                b'"' | b'`' | b'\'' => {
                    let quote = bytes[index];
                    index += 1;
                    while *bytes.get(index)? != quote {
                        if bytes[index] == b'\\' {
                            index += 1;
                        }
                        index += 1;
                    }
                }
                b'[' | b'(' | b'{' => depth += 1,
                b']' if depth == 0 => break,
                b']' | b')' | b'}' => depth = depth.checked_sub(1)?,
                _ => {}
            }
            index += 1;
        }
        groups.push((start, index));
        at = index + 1;
    }
    Some(groups)
}

impl Parser<'_> {
    /// Whether a function with the output `output` borrows its receiver.
    pub(super) fn place_output(&self, output: &Ty) -> Option<PlaceOutput> {
        match self.inference.resolve(output) {
            Ty::Reference {
                mutable: true,
                target,
            } => Some(PlaceOutput::Reference(*target)),
            Ty::Option(inner) => match self.inference.resolve(&inner) {
                Ty::Reference {
                    mutable: true,
                    target,
                } => Some(PlaceOutput::Optional(*target)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The place a call whose output borrows it is made on: `value`'s,
    /// or the receiver's for `self` in a `&mut self` method.
    pub(super) fn borrowed_receiver(
        &mut self,
        value: &ParsedExpr,
        call: Span,
    ) -> Result<Lvalue, Error> {
        let lvalue = match &value.lvalue {
            Some(Lvalue::Slice(_)) => None,
            Some(lvalue) => Some(lvalue.clone()),
            None if value.source.as_str() == "this" => Some(Lvalue::Binding {
                binding: "this".into(),
                ty: value.ty.clone(),
            }),
            None => None,
        };
        let Some(lvalue) = lvalue else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a call returning `&mut` on a value that is not a local, a field \
                            or an element"
                    .into(),
                source: call,
            });
        };
        self.require_mutable_binding(lvalue.binding(), value.span)?;
        Ok(lvalue)
    }

    /// The value of `call`, a call on the place `receiver` (written as
    /// `RECEIVER_PLACE`) whose output borrows that place (`output`). The
    /// value has the borrowed place, which the call reaches each time the
    /// place is read or stored: `*c.slot(1) += 5` calls `slot` once, reads
    /// the path it gives and stores through it. An `Option<&mut T>`
    /// output's place is its `Some` value's, the call made where the value
    /// is evaluated (`if let Some(l) = s.last()`).
    pub(super) fn borrowed_call(
        &mut self,
        receiver: &Lvalue,
        call: origin::Fragment,
        output: PlaceOutput,
        expression: Span,
    ) -> ParsedExpr {
        let at = call
            .as_str()
            .find(RECEIVER_PLACE)
            .expect("a borrowing call is made on its receiver's place");
        let mut before = call;
        let mut after = before.split_off(at);
        let after = after.split_off(RECEIVER_PLACE.len());
        let path = self.temporary("path");
        let (lvalue, ty, source) = match output {
            PlaceOutput::Reference(target) => {
                let step = PlaceStep::Path {
                    path: origin::Fragment::plain(path),
                    call: Some((before, after)),
                };
                let lvalue = place_step(receiver, step, target.clone());
                let Lvalue::Nested(nested) = &lvalue else {
                    unreachable!("a borrowed place is nested in its receiver")
                };
                let (read, place, store) = self.nested_parts(nested);
                let value = self.temporary("value");
                let mut source = origin::Fragment::plain("({ ");
                source.push(read);
                source.push_text(format!("let {value} = "));
                source.push(place);
                source.push_text("; ");
                source.push(store);
                source.push_text(format!("{value} }})"));
                let ty = Ty::Reference {
                    mutable: true,
                    target: Box::new(target),
                };
                (lvalue, ty, source)
            }
            PlaceOutput::Optional(target) => {
                self.field_entries.push((
                    origin::Fragment::plain(format!("let {path} = (); ")),
                    origin::Fragment::default(),
                ));
                let (read, place, store) = self.nested_parts(&as_nested(receiver));
                let unwrapped = self.prelude_op(PreludeOp::Unwrap, [path.as_str().into()]);
                let test = self.prelude_op(PreludeOp::IsSome, [path.as_str().into()]);
                // A pattern matching the value calls the function on the
                // receiver where it is, and reaches the path it gives.
                if let Some(located) = self.place_located(receiver) {
                    let reached = self.temporary("reached");
                    let mut find = origin::Fragment::plain(format!("let {reached} = "));
                    find.push(located);
                    find.push_text(format!("; {path} = "));
                    find.push(before.clone());
                    find.push_text(format!("{KEYWORD_THROUGH}{reached}"));
                    find.push(after.clone());
                    find.push_text("; ");
                    let missing = test.clone().surrounded("!(", ")");
                    self.optional_subjects.insert(expression, (find, missing));
                }
                let getter = self.push_op(Op::Prelude(PreludeOp::PathGet));
                let mut element = place.clone();
                element.push_text(format!(".{getter}("));
                element.push(unwrapped.clone());
                element.push_text(")");
                let some = self.prelude_op(PreludeOp::Some, [element]);
                let none = self.prelude_op(PreludeOp::None, []);
                let value = self.temporary("value");
                let mut source = origin::Fragment::plain("({ ");
                source.push(read);
                source.push_text(format!("{path} = "));
                source.push(before);
                source.push(place);
                source.push(after);
                source.push_text(format!("; let {value} = if "));
                source.push(test);
                source.push_text(" { ");
                source.push(some);
                source.push_text(" } else { ");
                source.push(none);
                source.push_text(" }; ");
                source.push(store);
                source.push_text(format!("{value} }})"));
                let step = PlaceStep::Path {
                    path: unwrapped,
                    call: None,
                };
                let lvalue = place_step(receiver, step, target.clone());
                let ty = Ty::Option(Box::new(Ty::Reference {
                    mutable: true,
                    target: Box::new(target),
                }));
                (lvalue, ty, source)
            }
        };
        let mut value = self.operator_expr(source, ty, expression);
        value.lvalue = Some(lvalue);
        value
    }
}

/// `lvalue`'s place with `step` below it, of type `ty`.
pub(super) fn place_step(lvalue: &Lvalue, step: PlaceStep, ty: Ty) -> Lvalue {
    let mut nested = as_nested(lvalue);
    nested.steps.push(step);
    nested.ty = ty;
    Lvalue::Nested(Box::new(nested))
}

/// `lvalue` as a nested place: its root binding, the place the runtime
/// assigns directly, and the steps below that.
pub(super) fn as_nested(lvalue: &Lvalue) -> NestedLvalue {
    match lvalue {
        Lvalue::Binding { binding, ty } => NestedLvalue {
            binding: binding.clone(),
            base: origin::Fragment::plain(binding.to_string()),
            steps: Vec::new(),
            ty: ty.clone(),
        },
        Lvalue::Place { binding, ty, place } => NestedLvalue {
            binding: binding.clone(),
            base: place.clone(),
            steps: Vec::new(),
            ty: ty.clone(),
        },
        Lvalue::Field(field) => NestedLvalue {
            binding: field.binding.clone(),
            base: origin::Fragment::plain(field.binding.to_string()),
            steps: vec![PlaceStep::Field { slot: field.slot }],
            ty: field.ty.clone(),
        },
        Lvalue::Nested(nested) => (**nested).clone(),
        Lvalue::Slice(_) => unreachable!("a slice range is not borrowed by a call"),
    }
}
