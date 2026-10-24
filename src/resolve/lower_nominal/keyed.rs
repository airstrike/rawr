//! Rust's keyed collections (`Ty::Keyed`): `HashMap`, `BTreeMap`, `HashSet`
//! and `BTreeSet`. At runtime a collection is an array sorted by key, as
//! Rust's `Ord` orders the keys (`compare_value`): `[key, value]` pairs for
//! a map, the keys for a set. Every kind therefore iterates in key order;
//! Rust leaves a hashed collection's order unspecified, and a fixed one
//! keeps previews and screenshots stable. A lookup is a binary search
//! (`keyed_operation`) on the collection passed by reference, so reading or
//! changing one entry copies nothing, and iterating is walking the array.

use super::*;

/// A map or set type and how a value reaches it: by value, or through `&`
/// (`Some(false)`) or `&mut` (`Some(true)`).
pub(super) struct KeyedType {
    pub(super) kind: Keyed,
    pub(super) arguments: Box<[Ty]>,
    pub(super) through: Option<bool>,
}

/// What iterating a collection gives: `(K, V)` or `K`, or through a
/// reference `(&K, &V)` or `&K`.
pub(super) fn keyed_item(kind: Keyed, arguments: &[Ty], through: Option<bool>) -> Ty {
    let wrap = |ty: &Ty, mutable: bool| match through {
        Some(_) => Ty::Reference {
            mutable,
            target: Box::new(ty.clone()),
        },
        None => ty.clone(),
    };
    if kind.is_map() {
        Ty::Tuple(Box::new([
            wrap(&arguments[0], false),
            wrap(&arguments[1], through == Some(true)),
        ]))
    } else {
        wrap(&arguments[0], false)
    }
}

/// The methods of `Entry` this lowering reads after `map.entry(key)`.
const ENTRY_METHODS: &[&str] = &["and_modify", "or_insert", "or_insert_with", "or_default"];

impl Parser<'_> {
    /// The map or set a value of type `ty` is, or refers to.
    pub(super) fn keyed_type(&self, ty: &Ty) -> Option<KeyedType> {
        match self.inference.resolve(ty) {
            Ty::Keyed(kind, arguments) => Some(KeyedType {
                kind,
                arguments,
                through: None,
            }),
            Ty::Reference { mutable, target } => match self.inference.resolve(&target) {
                Ty::Keyed(kind, arguments) => Some(KeyedType {
                    kind,
                    arguments,
                    through: Some(mutable),
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// The collection a body path starting with `name` (already read)
    /// names before its `::function`: `HashMap`, unless the crate declares
    /// a type of that name, `collections::HashMap` or
    /// `std::collections::HashMap`. Reads the rest of the path.
    pub(super) fn keyed_path(&mut self, name: &str) -> Option<Keyed> {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        let identifier = |token: &Token| match token {
            Token::Identifier(text) => Some(text.as_str().to_string()),
            _ => None,
        };
        let (collection, consumed) = match name {
            "std"
                if tokens[0] == Token::DoubleColon
                    && identifier(&tokens[1]).as_deref() == Some("collections")
                    && tokens[2] == Token::DoubleColon =>
            {
                (identifier(&tokens[3])?, 4)
            }
            "collections" if tokens[0] == Token::DoubleColon => (identifier(&tokens[1])?, 2),
            _ if self.models.get(name).is_some() => return None,
            name => (name.to_string(), 0),
        };
        let kind = Keyed::from_name(crate::resolve::std_collection(&collection)?)?;
        if tokens[consumed] != Token::DoubleColon {
            return None;
        }
        for _ in 0..consumed {
            self.next();
        }
        Some(kind)
    }

    /// `HashMap::new()`, `with_capacity(n)`, `default()`, `from([(k, v),
    /// ..])` and `from_iter(items)`, with an optional turbofish
    /// (`HashSet::<i64>::new()`); the path is read up to its `::`.
    pub(super) fn parse_keyed_constructor(
        &mut self,
        kind: Keyed,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        self.expect(Token::DoubleColon, "'::' after a map or set type")?;
        let arity = if kind.is_map() { 2 } else { 1 };
        let mut arguments: Vec<Ty> = (0..arity).map(|_| self.inference.fresh()).collect();
        if matches!(self.peek(), Token::LessThan) {
            self.next();
            let mut written = Vec::new();
            while !matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                written.push(self.parse_local_type()?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect_close_angle("'>' after a map or set's type arguments")?;
            self.expect(Token::DoubleColon, "'::' after a map or set type")?;
            if written.len() != arity {
                return Err(Error::InvalidTypeArguments {
                    ty: kind.name().into(),
                    expected: arity,
                    actual: written.len(),
                    reference: name_span,
                });
            }
            arguments = written;
        }
        let (member, member_span) = self.expect_member_name()?;
        let (mut values, end) = self.parse_call_arguments("map or set constructor arguments")?;
        let call = span(name_span.start(), end);
        let ty = Ty::Keyed(kind, arguments.clone().into_boxed_slice());
        let source = match (member.as_str(), values.as_mut_slice()) {
            ("new" | "default", []) => origin::Fragment::plain("[]"),
            // The capacity is a hint; the sorted array grows as it needs.
            ("with_capacity", [capacity]) => {
                self.coerce_expr(&Ty::Int(Int::Usize), capacity)?;
                let mut source = origin::Fragment::plain("{ let __rawr_capacity = ");
                source.push(capacity.rendered_source());
                source.push_text("; [] }");
                source
            }
            ("from" | "from_iter", [items]) => {
                let Some((items, item)) = self.collected_items(items) else {
                    return Err(Error::TypeMismatch {
                        expected: "an array or an iterator of a map's or set's items".into(),
                        actual: items.ty.name(&self.models),
                        expression: items.span,
                    });
                };
                let expected = keyed_item(kind, &arguments, None);
                self.require_type(&expected, &item, call)?;
                self.require_key(kind, &arguments[0], call)?;
                let op = if kind.is_map() {
                    PreludeOp::MapFrom
                } else {
                    PreludeOp::SetFrom
                };
                self.prelude_op(op, [items])
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: kind.name().into(),
                    member,
                    reference: member_span,
                })
            }
        };
        Ok(self.operator_expr(source, ty, call))
    }

    /// `HashMap<K, V>` and its kin in a body, after the name.
    pub(super) fn parse_keyed_type(&mut self, name: &str, name_span: Span) -> Result<Ty, Error> {
        let kind = crate::resolve::std_collection(name)
            .and_then(Keyed::from_name)
            .expect("a std collection's name");
        let arity = if kind.is_map() { 2 } else { 1 };
        let mut arguments = Vec::new();
        if matches!(self.peek(), Token::LessThan) {
            self.next();
            while !matches!(self.peek(), Token::GreaterThan | Token::RightShift) {
                arguments.push(self.parse_local_type()?);
                if matches!(self.peek(), Token::Comma) {
                    self.next();
                } else {
                    break;
                }
            }
            self.expect_close_angle("'>' after a map or set's type arguments")?;
        }
        if arguments.len() != arity {
            return Err(Error::InvalidTypeArguments {
                ty: kind.name().into(),
                expected: arity,
                actual: arguments.len(),
                reference: name_span,
            });
        }
        Ok(Ty::Keyed(kind, arguments.into_boxed_slice()))
    }

    /// A map's keys need `Hash` and `Eq` (or, for the B-tree kinds, `Ord`),
    /// as `insert` and its kin require; through references, the referent's.
    pub(super) fn require_key(&mut self, kind: Keyed, key: &Ty, span: Span) -> Result<(), Error> {
        let mut key = self.inference.resolve(key);
        while let Ty::Reference { target, .. } = key {
            key = self.inference.resolve(&target);
        }
        if contains_inference(&key) {
            return Ok(());
        }
        let (holds, capability) = if kind.hashed() {
            (
                self.type_provides(&key, capability::Kind::Hash)
                    && self.type_provides(&key, capability::Kind::Eq),
                "Hash + Eq",
            )
        } else {
            (self.ord_type(&key).is_some(), "Ord")
        };
        if holds {
            Ok(())
        } else {
            Err(Error::MissingCapability {
                owner: key.name(&self.models),
                capability: capability.into(),
                operation: span,
            })
        }
    }

    /// A lookup's `&key` (`&str` reads as Rawr's `String`).
    fn coerce_key(&mut self, key: &Ty, argument: &mut ParsedExpr) -> Result<(), Error> {
        if self.inference.resolve(key) == Ty::String
            && self.inference.resolve(&argument.ty) == Ty::String
        {
            return Ok(());
        }
        let expected = Ty::Reference {
            mutable: false,
            target: Box::new(key.clone()),
        };
        self.coerce_expr(&expected, argument).map(|_| ())
    }

    pub(super) fn require_mutable_binding(
        &mut self,
        name: &ImmutableString,
        mutation: Span,
    ) -> Result<(), Error> {
        let Some(binding) = self.bindings.get(name) else {
            return Err(self.unsupported("assignment through an unknown binding"));
        };
        if binding.mutable {
            return Ok(());
        }
        Err(match &binding.kind {
            BindingKind::Receiver { owner } => Error::ImmutableReceiver {
                owner: owner.clone(),
                mutation,
                receiver: binding.declaration,
            },
            BindingKind::Local | BindingKind::Invocation => Error::ImmutableBinding {
                binding: name.clone(),
                mutation,
                declaration: binding.declaration,
            },
        })
    }

    /// The Rawr place a collection is changed through: a mutable local, an
    /// element of one (`v[i]`, a map's entry), or a field (written back).
    /// `None` for a field, which `keyed_mutation` copies out and back.
    pub(super) fn keyed_place(
        &mut self,
        value: &ParsedExpr,
    ) -> Result<Option<origin::Fragment>, Error> {
        if let Ty::Reference { mutable: false, .. } = self.inference.resolve(&value.ty) {
            return Err(Error::TypeMismatch {
                expected: "a mutable collection (`&mut`, or a `mut` binding)".into(),
                actual: value.ty.name(&self.models),
                expression: value.span,
            });
        }
        match &value.lvalue {
            Some(Lvalue::Binding { binding, .. }) => {
                self.require_mutable_binding(binding, value.span)?;
                Ok(Some(origin::Fragment::plain(binding.to_string())))
            }
            Some(Lvalue::Place { binding, place, .. }) => {
                self.require_mutable_binding(binding, value.span)?;
                Ok(Some(place.clone()))
            }
            Some(Lvalue::Field(field)) => {
                self.require_mutable_binding(&field.binding, value.span)?;
                Ok(None)
            }
            Some(Lvalue::Slice(_)) => {
                Err(self.unsupported("changing a slice range other than by its methods"))
            }
            Some(Lvalue::Nested(nested)) => {
                self.require_mutable_binding(&nested.binding, value.span)?;
                let nested = (**nested).clone();
                Ok(Some(self.nested_place(&nested)))
            }
            None => Err(self
                .unsupported("changing a collection that is not a local, an element or a field")),
        }
    }

    /// `op` called on the collection `value` by reference, changing it.
    fn keyed_mutation(
        &mut self,
        value: &ParsedExpr,
        op: PreludeOp,
        arguments: Vec<origin::Fragment>,
    ) -> Result<origin::Fragment, Error> {
        let helper = self.push_op(Op::Prelude(op));
        self.mutation_call(value, &helper, arguments)
    }

    /// The method `helper` called on the collection `value` (a local, an
    /// element or a field, which is stored back) by reference, changing it.
    pub(super) fn mutation_call(
        &mut self,
        value: &ParsedExpr,
        helper: &str,
        arguments: Vec<origin::Fragment>,
    ) -> Result<origin::Fragment, Error> {
        // `v[a..b]`: its copy changed, then stored back over the range.
        if let Some(Lvalue::Slice(slice)) = &value.lvalue {
            let slice = (**slice).clone();
            let (copy, result) = (self.temporary("slice"), self.temporary("result"));
            let mut source = origin::Fragment::plain(format!("{{ let {copy} = "));
            source.push(value.rendered_source());
            source.push_text(format!("; let {result} = {copy}.{helper}("));
            source.push(origin::Fragment::join(arguments, ", "));
            source.push_text("); ");
            let mut store = value.clone();
            store.lvalue = Some(Lvalue::Slice(Box::new(slice)));
            source.push(self.store_into(&store, &copy)?);
            source.push_text(format!(" {result} }}"));
            return Ok(source);
        }
        let place = self.keyed_place(value)?;
        let arguments = origin::Fragment::join(arguments, ", ");
        if let Some(mut source) = place {
            source.push_text(format!(".{helper}("));
            source.push(arguments);
            source.push_text(")");
            return Ok(source);
        }
        let Some(Lvalue::Field(field)) = &value.lvalue else {
            unreachable!("a collection without a place is a field");
        };
        let setter = self.push_op(Op::SetField {
            definition: field.definition.clone(),
            slot: field.slot,
        });
        let (receiver, result) = (self.temporary("receiver"), self.temporary("result"));
        let owner = &field.binding;
        let mut source = origin::Fragment::plain(format!("{{ let {receiver} = "));
        source.push(value.rendered_source());
        source.push_text(format!("; let {result} = {receiver}.{helper}("));
        source.push(arguments);
        source.push_text(format!(
            "); {owner} = {setter}({owner}, {receiver}); {result} }}"
        ));
        Ok(source)
    }

    /// A map's or set's method.
    pub(super) fn parse_keyed_method(
        &mut self,
        value: ParsedExpr,
        keyed: KeyedType,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let KeyedType {
            kind,
            arguments: types,
            through,
        } = keyed;
        let map = kind.is_map();
        if map && member.as_str() == "entry" {
            return self.parse_keyed_entry(value, kind, types, member_span);
        }
        let owner = |this: &Self| Ty::Keyed(kind, types.clone()).name(&this.models);
        if map && matches!(member.as_str(), "get_mut" | "iter_mut" | "values_mut") {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!(
                    "`{}::{member}` other than `if let Some(v) = map.get_mut(&k)` or \
                     `for (k, v) in map.iter_mut()` (and `values_mut`)",
                    kind.name()
                )
                .into(),
                source: member_span,
            });
        }
        // `range(a..b)` of a B-tree map or set: its entries in that key
        // range, as an iterator.
        if matches!(kind, Keyed::BTreeMap | Keyed::BTreeSet) && member.as_str() == "range" {
            self.expect(Token::LeftParen, "'(' after `range`")?;
            let bounds = self.parse_range_or_expr()?;
            let end = self.expect(Token::RightParen, "')' after `range`'s range")?;
            let Err((low, high, inclusive)) = bounds else {
                return Err(self.unsupported("`range` of something other than a range"));
            };
            let mut flags = if map { 8 } else { 0 } | if inclusive { 4 } else { 0 };
            let mut operands = Vec::new();
            for (bound, flag) in [(low, 1), (high, 2)] {
                operands.push(match bound {
                    Some(mut bound) => {
                        flags |= flag;
                        self.coerce_expr(&types[0], &mut bound)?;
                        bound.rendered_source()
                    }
                    None => "()".into(),
                });
            }
            operands.push(flags.to_string().into());
            let helper = self.push_op(Op::Prelude(PreludeOp::KeyedRange));
            let mut source = value.rendered_source().surrounded("(", ")");
            source.push_text(format!(".{helper}("));
            source.push(origin::Fragment::join(operands, ", "));
            source.push_text(")");
            let item = keyed_item(kind, &types, Some(false));
            let ty = Ty::Iter(Box::new(item));
            return Ok(self.operator_expr(source, ty, span(value.span.start(), end)));
        }
        let (mut arguments, end) = self.parse_call_arguments("map or set method arguments")?;
        let expression = span(value.span.start(), end);
        let arity = |expected: usize, arguments: &[ParsedExpr]| {
            if arguments.len() == expected {
                Ok(())
            } else {
                Err(Error::InvalidMethodCall {
                    owner: kind.name().into(),
                    method: member.clone(),
                    reason: format!("expects {expected} argument(s), found {}", arguments.len())
                        .into(),
                    call: expression,
                })
            }
        };
        let receiver = value.rendered_source().surrounded("(", ")");
        let read = |this: &mut Self, op: PreludeOp, arguments: &[ParsedExpr]| {
            let helper = this.push_op(Op::Prelude(op));
            let mut source = receiver.clone();
            source.push_text(format!(".{helper}("));
            source.push(origin::Fragment::join(
                arguments.iter().map(ParsedExpr::rendered_source),
                ", ",
            ));
            source.push_text(")");
            source
        };
        let (key, entry) = (types[0].clone(), types.get(1).cloned());
        // Set algebra with another set of the same type.
        if !map && let Some(code) = SET_ALGEBRA.iter().position(|name| *name == member.as_str()) {
            arity(1, &arguments)?;
            let expected = Ty::Reference {
                mutable: false,
                target: Box::new(Ty::Keyed(kind, types.clone())),
            };
            self.coerce_expr(&expected, &mut arguments[0])?;
            arguments.push(self.literal(code.to_string(), Ty::Int(Int::Usize), expression));
            let source = read(self, PreludeOp::SetAlgebra, &arguments);
            let ty = if code < 4 {
                Ty::Iter(Box::new(Ty::Reference {
                    mutable: false,
                    target: Box::new(key),
                }))
            } else {
                Ty::Bool
            };
            return Ok(self.operator_expr(source, ty, expression));
        }
        let (source, ty) = match member.as_str() {
            "len" => {
                arity(0, &arguments)?;
                (receiver.surrounded("", ".len()"), Ty::Int(Int::Usize))
            }
            "is_empty" => {
                arity(0, &arguments)?;
                (receiver.surrounded("(", ".len() == 0)"), Ty::Bool)
            }
            "clone" => {
                arity(0, &arguments)?;
                let ty = Ty::Keyed(kind, types.clone());
                if !self.type_provides(&ty, capability::Kind::Clone) {
                    return Err(Error::MissingCapability {
                        owner: owner(self),
                        capability: "Clone".into(),
                        operation: expression,
                    });
                }
                (receiver, ty)
            }
            "insert" if map => {
                arity(2, &arguments)?;
                self.coerce_expr(&key, &mut arguments[0])?;
                let value_ty = entry.clone().expect("a map has a value type");
                self.coerce_expr(&value_ty, &mut arguments[1])?;
                self.require_key(kind, &key, expression)?;
                let rendered = arguments.iter().map(ParsedExpr::rendered_source).collect();
                let source = self.keyed_mutation(&value, PreludeOp::MapInsert, rendered)?;
                (source, Ty::Option(Box::new(value_ty)))
            }
            "insert" => {
                arity(1, &arguments)?;
                self.coerce_expr(&key, &mut arguments[0])?;
                self.require_key(kind, &key, expression)?;
                let rendered = vec![arguments[0].rendered_source()];
                let source = self.keyed_mutation(&value, PreludeOp::SetInsert, rendered)?;
                (source, Ty::Bool)
            }
            "remove" => {
                arity(1, &arguments)?;
                self.coerce_key(&key, &mut arguments[0])?;
                let rendered = vec![arguments[0].rendered_source()];
                if map {
                    let source = self.keyed_mutation(&value, PreludeOp::MapRemove, rendered)?;
                    let value_ty = entry.clone().expect("a map has a value type");
                    (source, Ty::Option(Box::new(value_ty)))
                } else {
                    let source = self.keyed_mutation(&value, PreludeOp::SetRemove, rendered)?;
                    (source, Ty::Bool)
                }
            }
            "get" if map => {
                arity(1, &arguments)?;
                self.coerce_key(&key, &mut arguments[0])?;
                let value_ty = Ty::Reference {
                    mutable: false,
                    target: Box::new(entry.clone().expect("a map has a value type")),
                };
                (
                    read(self, PreludeOp::MapGet, &arguments),
                    Ty::Option(Box::new(value_ty)),
                )
            }
            "contains_key" if map => {
                arity(1, &arguments)?;
                self.coerce_key(&key, &mut arguments[0])?;
                (read(self, PreludeOp::MapContainsKey, &arguments), Ty::Bool)
            }
            "contains" if !map => {
                arity(1, &arguments)?;
                self.coerce_key(&key, &mut arguments[0])?;
                (read(self, PreludeOp::SetContains, &arguments), Ty::Bool)
            }
            // Iterators over the sorted entries (the keyed chains).
            "iter" | "into_iter" | "keys" | "values" | "into_keys" | "into_values" => {
                arity(0, &arguments)?;
                let through = match member.as_str() {
                    "iter" | "keys" | "values" => Some(false),
                    "into_iter" => through,
                    _ => None,
                };
                let (item, project) = match member.as_str() {
                    "keys" | "into_keys" if map => {
                        (keyed_item(Keyed::HashSet, &types, through), Some(0))
                    }
                    "values" | "into_values" if map => {
                        (keyed_item(Keyed::HashSet, &types[1..], through), Some(1))
                    }
                    "iter" | "into_iter" => (keyed_item(kind, &types, through), None),
                    _ => {
                        return Err(Error::UnknownMember {
                            owner: owner(self),
                            member,
                            reference: member_span,
                        })
                    }
                };
                return Ok(self.keyed_iterator(&value, item, project, end));
            }
            // Each item inserted in turn, as Rust's `Extend` does.
            "extend" => {
                arity(1, &arguments)?;
                let Some((items, item)) = self.collected_items(&arguments[0]) else {
                    return Err(Error::TypeMismatch {
                        expected: "an array or an iterator to `extend` with".into(),
                        actual: arguments[0].ty.name(&self.models),
                        expression: arguments[0].span,
                    });
                };
                self.require_type(&keyed_item(kind, &types, None), &item, expression)?;
                self.require_key(kind, &key, expression)?;
                let element = self.temporary("item");
                let (op, parts) = if map {
                    (
                        PreludeOp::MapInsert,
                        vec![
                            origin::Fragment::plain(format!("{element}[0]")),
                            origin::Fragment::plain(format!("{element}[1]")),
                        ],
                    )
                } else {
                    (
                        PreludeOp::SetInsert,
                        vec![origin::Fragment::plain(element.clone())],
                    )
                };
                let insert = self.keyed_mutation(&value, op, parts)?;
                let mut source = origin::Fragment::plain(format!("{{ for {element} in ("));
                source.push(items);
                source.push_text(") { ");
                source.push(insert);
                source.push_text("; } }");
                (source, Ty::Unit)
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: owner(self),
                    member,
                    reference: member_span,
                })
            }
        };
        Ok(self.operator_expr(source, ty, expression))
    }

    /// `map[&key]`: the key's value, or Rust's panic.
    pub(super) fn keyed_index(
        &mut self,
        value: ParsedExpr,
        keyed: KeyedType,
        mut index: ParsedExpr,
        end: Position,
    ) -> Result<ParsedExpr, Error> {
        if !keyed.kind.is_map() {
            return Err(Error::TypeMismatch {
                expected: "an array, Vec, slice or map".into(),
                actual: value.ty.name(&self.models),
                expression: value.span,
            });
        }
        self.coerce_key(&keyed.arguments[0], &mut index)?;
        let helper = self.push_op(Op::Prelude(PreludeOp::MapIndex));
        let mut source = value.rendered_source().surrounded("(", ")");
        source.push_text(format!(".{helper}("));
        source.push(index.rendered_source());
        source.push_text(")");
        Ok(self.operator_expr(
            source,
            keyed.arguments[1].clone(),
            span(value.span.start(), end),
        ))
    }

    /// `map.entry(key)` with `and_modify(|v| ..)` and then `or_insert(v)`,
    /// `or_insert_with(f)` or `or_default()`: the entry's index, inserting
    /// or changing it first. Without `and_modify` the result is the value's
    /// place, so `*map.entry(k).or_insert(0) += 1` and
    /// `map.entry(k).or_default().push(x)` change it in the map.
    fn parse_keyed_entry(
        &mut self,
        value: ParsedExpr,
        kind: Keyed,
        types: Box<[Ty]>,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, mut end) = self.parse_call_arguments("`entry` arguments")?;
        let [key] = arguments.as_mut_slice() else {
            return Err(Error::InvalidMethodCall {
                owner: kind.name().into(),
                method: "entry".into(),
                reason: format!("expects 1 argument(s), found {}", arguments.len()).into(),
                call: span(value.span.start(), end),
            });
        };
        self.coerce_expr(&types[0], key)?;
        self.require_key(kind, &types[0], key.span)?;
        let key = key.rendered_source();
        let value_ty = types[1].clone();
        let root = match self.keyed_place(&value)? {
            Some(root) => root,
            // A field's map: copied into a local before the statement and
            // stored back after it (`with_field_entries`).
            None => {
                let Some(Lvalue::Field(field)) = &value.lvalue else {
                    unreachable!("a collection without a place is a field");
                };
                let setter = self.push_op(Op::SetField {
                    definition: field.definition.clone(),
                    slot: field.slot,
                });
                let local = self.temporary("field");
                let owner = &field.binding;
                let mut read = origin::Fragment::plain(format!("let {local} = "));
                read.push(value.rendered_source());
                read.push_text("; ");
                let store =
                    origin::Fragment::plain(format!(" {owner} = {setter}({owner}, {local});"));
                self.field_entries.push((read, store));
                origin::Fragment::plain(local)
            }
        };
        let root_binding = value
            .lvalue
            .as_ref()
            .map(|lvalue| lvalue.binding().clone())
            .expect("a place has a binding");
        let slot = self.push_op(Op::Prelude(PreludeOp::MapSlot));
        let insert = self.push_op(Op::Prelude(PreludeOp::MapSlotInsert));
        let at = self.temporary("at");
        let mut modify: Option<origin::Fragment> = None;
        // The value a vacant entry gets, and whether it is made lazily.
        let mut vacant: Option<(origin::Fragment, bool)> = None;
        while vacant.is_none()
            && matches!(self.peek(), Token::Period)
            && matches!(self.peek_second(), Token::Identifier(ref name)
                if ENTRY_METHODS.contains(&name.as_str()))
        {
            self.next();
            let (method, method_span) = self.expect_member_name()?;
            match method.as_str() {
                "and_modify" if modify.is_none() => {
                    let mut entry_place = root.clone();
                    entry_place.push_text(format!("[{at}][1]"));
                    let (body, close) = self.parse_entry_modifier(&value_ty, entry_place)?;
                    modify = Some(body);
                    end = close;
                }
                "or_insert" => {
                    let (mut values, close) = self.parse_call_arguments("`or_insert` arguments")?;
                    let [inserted] = values.as_mut_slice() else {
                        return Err(self.unsupported("`or_insert` without exactly one value"));
                    };
                    self.coerce_expr(&value_ty, inserted)?;
                    vacant = Some((inserted.rendered_source(), false));
                    end = close;
                }
                "or_default" => {
                    let (values, close) = self.parse_call_arguments("`or_default` arguments")?;
                    if !values.is_empty() {
                        return Err(self.unsupported("`or_default` with arguments"));
                    }
                    vacant = Some((self.default_source(&value_ty, method_span)?, false));
                    end = close;
                }
                "or_insert_with" => {
                    let (made, close) = self.parse_entry_maker(&value_ty, method_span)?;
                    vacant = Some(made);
                    end = close;
                }
                _ => {
                    return Err(Error::UnknownMember {
                        owner: "Entry".into(),
                        member: method,
                        reference: method_span,
                    })
                }
            }
        }
        let expression = span(value.span.start(), end);
        if modify.is_none() && vacant.is_none() {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "`entry` without `or_insert`, `or_insert_with`, `or_default` or \
                            `and_modify`"
                    .into(),
                source: member_span,
            });
        }
        let call = |name: &str, arguments: &[&origin::Fragment]| {
            let mut source = root.clone();
            source.push_text(format!(".{name}("));
            source.push(origin::Fragment::join(
                arguments.iter().map(|argument| (*argument).clone()),
                ", ",
            ));
            source.push_text(")");
            source
        };
        // The entry's index: found, else inserted; `and_modify` changes a
        // found one in place.
        let index = match (&modify, &vacant) {
            (None, Some((vacant, false))) => call(&insert, &[&key, vacant]),
            (modify, vacant) => {
                let mut source = origin::Fragment::plain(format!("{{ let {at} = "));
                source.push(call(&slot, &[&key]));
                source.push_text(format!("; if {at} < 0 {{ "));
                if let Some((vacant, _)) = vacant {
                    source.push_text(format!("{at} = "));
                    source.push(call(&insert, &[&key, vacant]));
                    source.push_text("; ");
                }
                source.push_text("}");
                if let Some(modify) = modify {
                    source.push_text(" else { ");
                    source.push(modify.clone());
                    source.push_text(" }");
                }
                source.push_text(format!(" {at} }}"));
                source
            }
        };
        if vacant.is_none() {
            // `entry(k).and_modify(f);`: the entry itself is dropped.
            let source = index.surrounded("{ ", "; }");
            return Ok(self.operator_expr(source, Ty::Unit, expression));
        }
        let mut place = root;
        place.push_text("[");
        place.push(index);
        place.push_text("][1]");
        let mut entry = self.operator_expr(
            place.clone(),
            Ty::Reference {
                mutable: true,
                target: Box::new(value_ty.clone()),
            },
            expression,
        );
        // Its place, re-read by a compound assignment: only without
        // `and_modify`, whose change must run once.
        if modify.is_none() {
            entry.lvalue = Some(Lvalue::Place {
                binding: root_binding,
                ty: value_ty,
                place,
            });
        }
        Ok(entry)
    }

    /// `and_modify(|v| body)`: `body` with `v` a mutable local holding the
    /// entry's value, which is stored back after it.
    fn parse_entry_modifier(
        &mut self,
        value_ty: &Ty,
        entry_place: origin::Fragment,
    ) -> Result<(origin::Fragment, Position), Error> {
        self.expect(Token::LeftParen, "'(' after `and_modify`")?;
        // `and_modify(bump)` of a function taking `&mut V`: the entry's
        // value is its receiver (`this_parameter`).
        if let Token::Identifier(name) = self.peek()
            && let Some(function) = self.models.function(&name).cloned()
            && this_parameter(&function.definition) == Some(0)
            && function.definition.parameters.len() == 1
            && function.definition.generics.is_empty()
        {
            let (_, name_span) = self.expect_name("`and_modify` function")?;
            let expected = Ty::Reference {
                mutable: true,
                target: Box::new(value_ty.clone()),
            };
            let parameter = Ty::from_resolved(&function.definition.parameters[0].ty);
            self.require_type(&parameter, &expected, name_span)?;
            if matches!(self.peek(), Token::Comma) {
                self.next();
            }
            let end = self.expect(Token::RightParen, "')' after `and_modify`'s function")?;
            let mut source = entry_place;
            source.push_text(format!(".{}();", function.generated));
            self.used_nominal = true;
            return Ok((source, end));
        }
        self.expect(Token::Pipe, "a closure `|v| ..` in `and_modify`")?;
        let (name, declaration) = self.expect_name("`and_modify` closure parameter")?;
        if matches!(self.peek(), Token::Colon) {
            self.next();
            let written = self.parse_local_type()?;
            let expected = Ty::Reference {
                mutable: true,
                target: Box::new(value_ty.clone()),
            };
            self.require_type(&expected, &written, declaration)?;
        }
        self.expect(Token::Pipe, "'|' after the `and_modify` closure parameter")?;
        let outer = self.bindings.clone();
        // The parameter is read and written through its own name.
        self.bindings.insert(
            name.clone(),
            Binding {
                ty: Ty::Reference {
                    mutable: true,
                    target: Box::new(value_ty.clone()),
                },
                mutable: true,
                available: true,
                moved_fields: Default::default(),
                declaration,
                kind: BindingKind::Local,
            },
        );
        let body = self.parse_inline_closure_body();
        self.bindings = outer;
        let body = body?;
        if matches!(self.peek(), Token::Comma) {
            self.next();
        }
        let end = self.expect(Token::RightParen, "')' after `and_modify`'s closure")?;
        let mut source = origin::Fragment::plain(format!("let {name} = "));
        source.push(entry_place.clone());
        source.push_text("; ");
        source.push(body.rendered_source());
        source.push_text("; ");
        source.push(entry_place);
        source.push_text(format!(" = {name};"));
        Ok((source, end))
    }

    /// A closure's body, parsed where it is written: a block, an
    /// expression, or an assignment.
    pub(super) fn parse_inline_closure_body(&mut self) -> Result<ParsedExpr, Error> {
        if matches!(self.peek(), Token::LeftBrace) {
            return self.parse_match_action().map(|branch| branch.value);
        }
        let value = self.parse_expr()?;
        if !super::assignment_token(&self.peek()) {
            return Ok(value);
        }
        let start = value.span;
        let (rendered, end) = self.parse_assignment(value)?;
        Ok(self.operator_expr(
            rendered.surrounded("{ ", " }"),
            Ty::Unit,
            span(start.start(), end.end()),
        ))
    }

    /// `or_insert_with(f)`: `f()`, called only for a vacant entry; a
    /// constructor path (`Vec::new`, `String::new`, `HashMap::new`) is the
    /// value type's default.
    fn parse_entry_maker(
        &mut self,
        value_ty: &Ty,
        method_span: Span,
    ) -> Result<((origin::Fragment, bool), Position), Error> {
        let mut ahead = self.input.clone();
        let tokens: Vec<Token> = (0..5)
            .map(|_| ahead.next().map_or(Token::EOF, |(token, _)| token))
            .collect();
        if tokens[0] == Token::LeftParen
            && matches!(&tokens[1], Token::Identifier(owner)
                if matches!(owner.as_str(), "Vec" | "String" | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet"))
            && tokens[2] == Token::DoubleColon
            && matches!(&tokens[3], Token::Identifier(name) if matches!(name.as_str(), "new" | "default"))
            && tokens[4] == Token::RightParen
        {
            for _ in 0..4 {
                self.next();
            }
            let end = self.next().1;
            return Ok(((self.default_source(value_ty, method_span)?, false), end));
        }
        let (mut values, end) = self.parse_call_arguments("`or_insert_with` arguments")?;
        let [maker] = values.as_mut_slice() else {
            return Err(self.unsupported("`or_insert_with` without exactly one function"));
        };
        let expected = Ty::Fn {
            inputs: Box::new([]),
            output: Box::new(value_ty.clone()),
        };
        self.coerce_expr(&expected, maker)?;
        Ok((
            (maker.rendered_source().surrounded("(", ").call()"), true),
            end,
        ))
    }
}
