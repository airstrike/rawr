//! Rust's `PartialEq`, `PartialOrd` and `Ord` of runtime values, as a
//! derive or a builtin compares them, with a crate type's hand-written
//! impl wherever its values sit: in a tuple, an `Option`, a vector, a map
//! key or a derived type's field (D419, D423). A [`Shape`] says where a
//! `u64` or `usize` sits, whose bits order as an unsigned number (D425).

use super::*;

/// Where a value's static type reads integers as unsigned: a `u64` or a
/// `usize` is its bits in the runtime's `i64`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Shape {
    /// Compared as the values say.
    Plain,
    /// A `u64` or a `usize`.
    Unsigned,
    /// Each element of a vector, array, slice or set.
    Each(Box<Shape>),
    /// A tuple's members; a map's `[key, value]` entry.
    Tuple(Box<[Shape]>),
    Option(Box<Shape>),
    Result(Box<Shape>, Box<Shape>),
}

impl Shape {
    pub(super) fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }

    /// The shape of `ty`, each part read through `resolve`. A crate type
    /// is plain: its fields carry their own shapes (`field_shape`).
    pub(super) fn of(ty: &Ty, resolve: &dyn Fn(&Ty) -> Ty) -> Self {
        let of = |ty: &Ty| Self::of(ty, resolve);
        let shape = match resolve(ty) {
            Ty::Int(Int::U64 | Int::Usize) => Self::Unsigned,
            Ty::Reference { target, .. } | Ty::Boxed(target) | Ty::Counted(_, target) => {
                of(&target)
            }
            Ty::Option(element) => Self::Option(Box::new(of(&element))),
            Ty::Result(ok, err) => Self::Result(Box::new(of(&ok)), Box::new(of(&err))),
            Ty::Array { element, .. } => Self::Each(Box::new(of(&element))),
            Ty::Tuple(members) => Self::Tuple(members.iter().map(of).collect()),
            Ty::Keyed(kind, arguments) if kind.is_map() && arguments.len() == 2 => {
                Self::Each(Box::new(Self::Tuple(arguments.iter().map(of).collect())))
            }
            Ty::Keyed(_, arguments) if arguments.len() == 1 => {
                Self::Each(Box::new(of(&arguments[0])))
            }
            _ => Self::Plain,
        };
        shape.simplified()
    }

    /// `Plain` where nothing inside is unsigned.
    fn simplified(self) -> Self {
        let plain = match &self {
            Self::Plain | Self::Unsigned => return self,
            Self::Each(inner) | Self::Option(inner) => inner.is_plain(),
            Self::Tuple(members) => members.iter().all(Self::is_plain),
            Self::Result(ok, err) => ok.is_plain() && err.is_plain(),
        };
        if plain {
            Self::Plain
        } else {
            self
        }
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Plain => formatter.write_str("p"),
            Self::Unsigned => formatter.write_str("u"),
            Self::Each(inner) => write!(formatter, "e({inner})"),
            Self::Option(inner) => write!(formatter, "o({inner})"),
            Self::Result(ok, err) => write!(formatter, "r({ok},{err})"),
            Self::Tuple(members) => {
                formatter.write_str("t(")?;
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{member}")?;
                }
                formatter.write_str(")")
            }
        }
    }
}

/// The shape of a derived type's field of declared type `ty`.
fn field_shape(ty: &type_ref::Type) -> Shape {
    if mentions_unsigned(ty) {
        Shape::of(&Ty::from_resolved(ty), &Ty::clone)
    } else {
        Shape::Plain
    }
}

fn mentions_unsigned(ty: &type_ref::Type) -> bool {
    match ty {
        type_ref::Type::Unit(_) => false,
        type_ref::Type::Tuple { members, .. } => members.iter().any(mentions_unsigned),
        type_ref::Type::Reference { target, .. } => mentions_unsigned(target),
        type_ref::Type::Path(path) => {
            path.target == type_ref::Target::Builtin
                && matches!(path.name.as_str(), "u64" | "usize")
                || path.arguments.iter().any(|argument| match argument {
                    type_ref::Argument::Type(ty) => mentions_unsigned(ty),
                    type_ref::Argument::Lifetime(_) => false,
                })
        }
    }
}

/// A comparison of runtime values, which calls a crate type's own
/// `eq`, `partial_cmp` or `cmp` through `context`. Without a context (a
/// constant folded at compile time) a type compares as derived.
pub(super) struct Comparison<'a, 'c> {
    pub(super) models: &'a Models,
    pub(super) context: Option<&'a NativeCallContext<'c>>,
    /// `Ord` (`cmp`), as a sort or a `BTreeMap` orders, rather than
    /// `PartialOrd` (`partial_cmp`), as `<` does.
    pub(super) total: bool,
}

type Compared<T> = Result<T, crate::RawrError>;

impl Comparison<'_, '_> {
    /// Rust's `PartialOrd` (or `Ord`, when `total`) of `lhs` and `rhs`:
    /// `None` when they are unordered, as a NaN is.
    pub(super) fn compare(
        &self,
        lhs: &Dynamic,
        rhs: &Dynamic,
        shape: &Shape,
    ) -> Compared<Option<std::cmp::Ordering>> {
        use std::cmp::Ordering;
        if let Some((place, other, reversed)) = crate::place::get(lhs)
            .map(|place| (place, rhs, false))
            .or_else(|| crate::place::get(rhs).map(|place| (place, lhs, true)))
        {
            let result = place.invoke(
                crate::place::Operation::Compare {
                    total: self.total,
                    reversed,
                },
                &mut [other.clone()],
            )?;
            return crate::option::read(&result)
                .ok_or_else(|| super::invariant("a native comparison lost its Option"))?
                .map(|order| {
                    order
                        .as_int()
                        .map(|order| order.cmp(&0))
                        .map_err(|_| super::invariant("a native comparison lost its Ordering"))
                })
                .transpose();
        }
        // An `Rc` or `Arc` compares its value, as std's impls do.
        if let (Some(lhs), Some(rhs)) = (super::counted::value(lhs), super::counted::value(rhs)) {
            return self.compare(&lhs, &rhs, shape);
        }
        match shape {
            Shape::Unsigned => {
                if let (Ok(lhs), Ok(rhs)) = (lhs.as_int(), rhs.as_int()) {
                    return Ok(Some((lhs as u64).cmp(&(rhs as u64))));
                }
            }
            Shape::Option(inner) => {
                if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs))
                {
                    return match (lhs, rhs) {
                        (Some(lhs), Some(rhs)) => self.compare(&lhs, &rhs, inner),
                        (lhs, rhs) => Ok(Some(lhs.is_some().cmp(&rhs.is_some()))),
                    };
                }
            }
            Shape::Result(ok, err) => {
                if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs))
                {
                    return match (lhs, rhs) {
                        (Ok(lhs), Ok(rhs)) => self.compare(&lhs, &rhs, ok),
                        (Err(lhs), Err(rhs)) => self.compare(&lhs, &rhs, err),
                        (Ok(_), Err(_)) => Ok(Some(Ordering::Less)),
                        (Err(_), Ok(_)) => Ok(Some(Ordering::Greater)),
                    };
                }
            }
            Shape::Each(inner) => {
                if let (Ok(lhs), Ok(rhs)) = (lhs.as_array_ref(), rhs.as_array_ref()) {
                    return self.lexicographic(&lhs, &rhs, |_| inner);
                }
            }
            Shape::Tuple(members) => {
                if let (Ok(lhs), Ok(rhs)) = (lhs.as_array_ref(), rhs.as_array_ref()) {
                    return self.lexicographic(&lhs, &rhs, |index| {
                        members.get(index).unwrap_or(&Shape::Plain)
                    });
                }
            }
            _ => {}
        }
        self.compare_plain(lhs, rhs)
    }

    fn lexicographic<'s>(
        &self,
        lhs: &[Dynamic],
        rhs: &[Dynamic],
        shape: impl Fn(usize) -> &'s Shape,
    ) -> Compared<Option<std::cmp::Ordering>> {
        for (index, (lhs, rhs)) in lhs.iter().zip(rhs).enumerate() {
            match self.compare(lhs, rhs, shape(index))? {
                Some(std::cmp::Ordering::Equal) => {}
                ordering => return Ok(ordering),
            }
        }
        Ok(Some(lhs.len().cmp(&rhs.len())))
    }

    fn compare_plain(&self, lhs: &Dynamic, rhs: &Dynamic) -> Compared<Option<std::cmp::Ordering>> {
        let identities = match (&lhs.0, &rhs.0) {
            (Union::Struct(lhs, ..), Union::Struct(rhs, ..)) => {
                Some((lhs.identity().clone(), rhs.identity().clone()))
            }
            (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) => {
                Some((lhs.identity().clone(), rhs.identity().clone()))
            }
            _ => None,
        };
        let Some((identity, other)) = identities else {
            return self.compare_builtin(lhs, rhs);
        };
        if identity != other {
            return Ok(None);
        }
        let Some(nominal) = self.models.by_identity.get(&identity) else {
            return Ok(None);
        };
        // A hand-written impl: `cmp` for `Ord`, `partial_cmp` for `<`, or
        // whichever the type has.
        let owner = nominal.definition().id();
        let preferred = if self.total {
            [("Ord", "cmp"), ("PartialOrd", "partial_cmp")]
        } else {
            [("PartialOrd", "partial_cmp"), ("Ord", "cmp")]
        };
        if let Some(context) = self.context
            && let Some((function, definition)) =
                preferred.iter().find_map(|(trait_name, function)| {
                    Some((
                        *function,
                        self.models.std_impl(owner, trait_name, function)?,
                    ))
                })
        {
            let ordering = call(context, definition, lhs, rhs)?;
            let ordering = if function == "cmp" {
                Some(ordering)
            } else {
                crate::option::read(&ordering)
                    .ok_or_else(|| invariant("`partial_cmp` did not give an `Option`"))?
            };
            return Ok(ordering
                .and_then(|ordering| ordering.as_int().ok())
                .map(|code| code.cmp(&0)));
        }
        // A derive: fields in declaration order; an enum's variants by
        // declaration order, then their payloads.
        match (nominal, &lhs.0, &rhs.0) {
            (Nominal::Struct { item, .. }, Union::Struct(lhs, ..), Union::Struct(rhs, ..)) => {
                let shapes = item
                    .fields
                    .iter()
                    .map(|field| field_shape(&field.ty))
                    .collect::<Vec<_>>();
                self.lexicographic(lhs.fields(), rhs.fields(), |index| {
                    shapes.get(index).unwrap_or(&Shape::Plain)
                })
            }
            (Nominal::Enum { item, .. }, Union::Enum(lhs, ..), Union::Enum(rhs, ..)) => {
                let index = |name: &str| {
                    item.variants
                        .iter()
                        .position(|variant| variant.name.text == name)
                };
                let (Some(variant), Some(other)) =
                    (index(lhs.variant().name()), index(rhs.variant().name()))
                else {
                    return Ok(None);
                };
                if variant != other {
                    return Ok(Some(variant.cmp(&other)));
                }
                let shapes = item.variants[variant]
                    .payload
                    .iter()
                    .map(field_shape)
                    .collect::<Vec<_>>();
                self.lexicographic(lhs.payload(), rhs.payload(), |index| {
                    shapes.get(index).unwrap_or(&Shape::Plain)
                })
            }
            _ => Ok(None),
        }
    }

    fn compare_builtin(
        &self,
        lhs: &Dynamic,
        rhs: &Dynamic,
    ) -> Compared<Option<std::cmp::Ordering>> {
        if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
            return match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => self.compare_plain(&lhs, &rhs),
                (lhs, rhs) => Ok(Some(lhs.is_some().cmp(&rhs.is_some()))),
            };
        }
        if let (Some(lhs), Some(rhs)) = (
            lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
            rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        ) {
            return self.compare_plain(&rhs.0, &lhs.0);
        }
        // A `Cow` orders as what it derefs to.
        if let (Some(lhs), Some(rhs)) = (super::cow::payload(lhs), super::cow::payload(rhs)) {
            return self.compare_plain(&lhs, &rhs);
        }
        if let (Ok(lhs), Ok(rhs)) = (lhs.as_array_ref(), rhs.as_array_ref()) {
            return self.lexicographic(&lhs, &rhs, |_| &Shape::Plain);
        }
        Ok(compare_value(lhs, rhs, self.models))
    }

    /// Rust's `PartialEq` of `lhs` and `rhs`.
    pub(super) fn equal(&self, lhs: &Dynamic, rhs: &Dynamic) -> Compared<bool> {
        if let Some((place, other, reversed)) = crate::place::get(lhs)
            .map(|place| (place, rhs, false))
            .or_else(|| crate::place::get(rhs).map(|place| (place, lhs, true)))
        {
            return place
                .invoke(
                    crate::place::Operation::Equal { reversed },
                    &mut [other.clone()],
                )?
                .as_bool()
                .map_err(|_| super::invariant("a native equality lost its bool"));
        }
        if let (Some(lhs), Some(rhs)) = (super::counted::value(lhs), super::counted::value(rhs)) {
            return self.equal(&lhs, &rhs);
        }
        if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
            return match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => self.equal(&lhs, &rhs),
                (lhs, rhs) => Ok(lhs.is_none() && rhs.is_none()),
            };
        }

        if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
            return match (lhs, rhs) {
                (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => self.equal(&lhs, &rhs),
                _ => Ok(false),
            };
        }
        if let (Some(lhs), Some(rhs)) = (
            lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
            rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        ) {
            return self.equal(&lhs.0, &rhs.0);
        }
        // A `Cow` compares what it derefs to, `Borrowed` or `Owned`.
        if let (Some(lhs), Some(rhs)) = (super::cow::payload(lhs), super::cow::payload(rhs)) {
            return self.equal(&lhs, &rhs);
        }
        match (&lhs.0, &rhs.0) {
            (Union::Array(left, ..), Union::Array(right, ..)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                for (lhs, rhs) in left.iter().zip(right.iter()) {
                    if !self.equal(lhs, rhs)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Union::Struct(left, ..), Union::Struct(right, ..)) => {
                if left.identity() != right.identity() || left.schema() != right.schema() {
                    return Ok(false);
                }
                if let Some(equal) = self.hand_written_eq(left.identity(), lhs, rhs)? {
                    return Ok(equal);
                }
                self.all_equal(left.fields(), right.fields())
            }
            (Union::Enum(left, ..), Union::Enum(right, ..)) => {
                if left.identity() != right.identity() || left.schema() != right.schema() {
                    return Ok(false);
                }
                if let Some(equal) = self.hand_written_eq(left.identity(), lhs, rhs)? {
                    return Ok(equal);
                }
                if left.variant() != right.variant() {
                    return Ok(false);
                }
                self.all_equal(left.payload(), right.payload())
            }
            _ => Ok(equal_value(lhs, rhs)),
        }
    }

    fn all_equal(&self, lhs: &[Dynamic], rhs: &[Dynamic]) -> Compared<bool> {
        if lhs.len() != rhs.len() {
            return Ok(false);
        }
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            if !self.equal(lhs, rhs)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The type's own `eq` of `lhs` and `rhs`, when it has one.
    fn hand_written_eq(
        &self,
        identity: &type_def::Identity,
        lhs: &Dynamic,
        rhs: &Dynamic,
    ) -> Compared<Option<bool>> {
        let Some(context) = self.context else {
            return Ok(None);
        };
        let Some(nominal) = self.models.by_identity.get(identity) else {
            return Ok(None);
        };
        let Some(definition) = self
            .models
            .std_impl(nominal.definition().id(), "PartialEq", "eq")
        else {
            return Ok(None);
        };
        let equal = call(context, definition, lhs, rhs)?;
        Ok(Some(equal.as_bool().unwrap_or(false)))
    }
}

/// `value` with each integer its shape says is unsigned tagged as one
/// (`UNSIGNED_TAG`), for a generic body that reads it without its type.
pub(super) fn tag_unsigned(mut value: Dynamic, shape: &Shape) -> Dynamic {
    match shape {
        Shape::Plain => {}
        Shape::Unsigned => {
            if value.is_int() {
                value.set_tag(crate::types::dynamic::UNSIGNED_TAG);
            }
        }
        Shape::Option(inner) => {
            if let Some(Some(element)) = crate::option::read(&value) {
                return crate::option::some(tag_unsigned(element, inner));
            }
        }
        Shape::Result(ok, err) => match crate::result::read(&value) {
            Some(Ok(element)) => return crate::result::ok(tag_unsigned(element, ok)),
            Some(Err(element)) => return crate::result::err(tag_unsigned(element, err)),
            None => {}
        },
        Shape::Each(_) | Shape::Tuple(_) => {
            if let Ok(mut array) = value.as_array_mut() {
                for (index, element) in array.iter_mut().enumerate() {
                    let inner = match shape {
                        Shape::Each(inner) => &**inner,
                        Shape::Tuple(members) => members.get(index).unwrap_or(&Shape::Plain),
                        _ => &Shape::Plain,
                    };
                    *element = tag_unsigned(std::mem::take(element), inner);
                }
            }
        }
    }
    value
}

/// The crate's method `definition` on `lhs` with `rhs` as its argument.
fn call(
    context: &NativeCallContext<'_>,
    definition: &method::Definition,
    lhs: &Dynamic,
    rhs: &Dynamic,
) -> RawrResult {
    let (mut receiver, mut argument) = (lhs.clone(), rhs.clone());
    context.call_script_method(
        definition.symbol().as_str(),
        &mut [&mut receiver, &mut argument],
    )
}

impl<'a> Parser<'a> {
    /// `a < b` through `PartialOrd`, as Rust has it, for operands the
    /// operator does not compare itself: a crate type's hand-written
    /// `partial_cmp`, a generic bounded by `PartialOrd` or `Ord`, or builtin
    /// values compared by value (`bool`, `Option`, tuples, `Vec`).
    /// `operator_at` is the operator's span where the program wrote one.
    pub(super) fn partial_ord_operator(
        &mut self,
        symbol: &str,
        operator_at: Option<Span>,
        lhs: &ParsedExpr,
        rhs: &mut ParsedExpr,
    ) -> Result<Option<ParsedExpr>, Error> {
        let method = match symbol {
            "<" => "lt",
            "<=" => "le",
            ">" => "gt",
            _ => "ge",
        };
        if let Some(value) = self.host_comparison(method, lhs, rhs)? {
            return Ok(Some(value));
        }
        let mut peeled = self.inference.resolve(&lhs.ty);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) = peeled {
            peeled = self.inference.resolve(&target);
        }
        let partial_cmp = match &peeled {
            Ty::Int(_) | Ty::Float(_) | Ty::String | Ty::Str | Ty::Char | Ty::Ordering => {
                return Ok(None)
            }
            Ty::Script(definition, ..)
                if let Some(definition) =
                    self.models
                        .std_impl(*definition, "PartialOrd", "partial_cmp") =>
            {
                Some((Some(definition.clone()), true))
            }
            Ty::Param(_) | Ty::Projection(..) if self.type_implements(&peeled, "PartialOrd") => {
                Some((None, true))
            }
            Ty::Param(_) | Ty::Projection(..) if self.type_implements(&peeled, "Ord") => {
                Some((None, false))
            }
            ty if self.std_ordered(ty, true) => Some((None, true)),
            _ => None,
        };
        let Some((definition, partial)) = partial_cmp else {
            // A crate type without `PartialOrd`, or a std type std doesn't
            // order (rustc's E0369).
            if matches!(peeled, Ty::Script(..))
                || self.std_declared(&peeled)
                    && !matches!(peeled, Ty::Bool | Ty::Char | Ty::Int(_) | Ty::Float(_))
            {
                return Err(Error::StdPath {
                    code: "E0369",
                    message: format!(
                        "binary operation `{symbol}` cannot be applied to type `{}`",
                        self.inference.resolve(&lhs.ty).name(&self.models)
                    )
                    .into(),
                    label: None,
                    note: None,
                    reference: operator_at
                        .unwrap_or_else(|| span(lhs.span.start(), rhs.span.end())),
                });
            }
            return Ok(None);
        };
        self.coerce_expr(&lhs.ty, rhs)?;
        let (value, other) = (lhs.rendered_source(), rhs.rendered_source());
        let compare = match (definition, &peeled) {
            (Some(definition), _) => Self::std_call(&definition, value, &[other]),
            (None, Ty::Param(_) | Ty::Projection(..)) if partial => {
                Self::std_dispatch("PartialOrd", "partial_cmp", value, &[other])
            }
            (None, Ty::Param(_) | Ty::Projection(..)) => {
                Self::std_dispatch("Ord", "cmp", value, &[other])
            }
            (None, _) => self.compare_op(&peeled, false, [value, other]),
        };
        // `Some(ordering)` compared to `Equal`'s 0, as Rust's `lt` matches.
        let ordering = self.temporary("ordering");
        let mut source = origin::Fragment::plain(format!("{{ let {ordering} = "));
        source.push(compare);
        source.push_text("; ");
        if partial {
            let is_some = self.prelude_op(PreludeOp::IsSome, [ordering.as_str().into()]);
            let unwrapped = self.prelude_op(PreludeOp::Unwrap, [ordering.as_str().into()]);
            source.push(is_some);
            source.push_text(" && ");
            source.push(unwrapped);
        } else {
            source.push_text(ordering.as_str());
        }
        source.push_text(format!(" {symbol} 0 }}"));
        Ok(Some(self.operator_expr(
            source.surrounded("(", ")"),
            Ty::Bool,
            span(lhs.span.start(), rhs.span.end()),
        )))
    }

    /// A library type's comparison by the facade method named for
    /// `PartialEq`'s or `PartialOrd`'s (`Degrees::lt`), which iced's
    /// `impl PartialOrd<f32> for Degrees`, log's `impl
    /// PartialOrd<LevelFilter> for Level` and their kin declare. `None` when
    /// `lhs` is no library type with that method, or when an `eq` would
    /// compare two values of the same library type, which its `PartialEq`
    /// capability does.
    pub(super) fn host_comparison(
        &mut self,
        method: &str,
        lhs: &ParsedExpr,
        rhs: &mut ParsedExpr,
    ) -> Result<Option<ParsedExpr>, Error> {
        let mut host = self.inference.resolve(&lhs.ty);
        while let Ty::Reference { target, .. } = host {
            host = self.inference.resolve(&target);
        }
        let Ty::Host { proof, .. } = &host else {
            return Ok(None);
        };
        let path = format!("{}::{method}", proof.definition.path());
        let mut other = self.inference.resolve(&rhs.ty);
        while let Ty::Reference { target, .. } = other {
            other = self.inference.resolve(&target);
        }
        // The right operand is another type, so the comparison is the left
        // type's `PartialEq<Rhs>` or `PartialOrd<Rhs>`, as rustc selects it:
        // a number, text, or another library type.
        let across = match &other {
            Ty::Int(_) | Ty::Float(_) | Ty::Str | Ty::String => true,
            Ty::Host { .. } => !same_head(&host, &other),
            _ => false,
        };
        if !self.models.host_items(&path).is_some_and(|items| {
            items
                .iter()
                .any(|item| item.kind() == crate::host::item::Kind::Method)
        }) {
            // An imported type compared with another type, as clock's
            // `rotation < 180.0` of `Degrees` or log's `level <=
            // STATIC_MAX_LEVEL`: a miss the importer answers with the type's
            // `PartialOrd<Rhs>` or `PartialEq<Rhs>`.
            #[cfg(feature = "rust-import")]
            {
                if proof.definition.identity().imported().is_some()
                    && across
                    && self.models.host.entry(&path).is_none()
                {
                    let error = Error::UnknownMember {
                        owner: host_type_name(proof, &self.models),
                        member: method.into(),
                        reference: span(lhs.span.start(), rhs.span.end()),
                    };
                    return Err(self.native_miss(true, error));
                }
            }
            return Ok(None);
        }
        if method == "eq" && !across {
            return Ok(None);
        }
        let expression = span(lhs.span.start(), rhs.span.end());
        let receiver = self.operator_expr(lhs.rendered_source(), host.clone(), lhs.span);
        // Rust's operator lends its right-hand side, as `rotation < 180.0`
        // is `PartialOrd::lt(&rotation, &180.0)` (T412): a crate's
        // comparison takes `&Rhs`.
        let mut other = rhs.clone();
        let lent = self.models.host_items(&path).is_some_and(|items| {
            items.iter().any(|item| {
                matches!(
                    item.signature().parameters().first().map(|ty| ty.shape()),
                    Some(crate::host::type_ref::Shape::Reference { mutable: false, .. })
                )
            })
        });
        if lent && !matches!(self.inference.resolve(&other.ty), Ty::Reference { .. }) {
            other.ty = Ty::Reference {
                mutable: false,
                target: Box::new(self.inference.resolve(&other.ty)),
            };
        }
        self.pending_arguments = Some((vec![other], expression.end()));
        let mut value = self.parse_host_method(receiver, method.into(), expression)?;
        value.span = expression;
        Ok(Some(value))
    }

    /// A compared type as rustc names it.
    pub(super) fn compared_name(&self, side: &Ty) -> ImmutableString {
        self.inference.resolve(side).name(&self.models)
    }

    /// rustc's E0369 for `lhs == rhs` (`!=` when `negate`) whose left
    /// operand's type has no `PartialEq`, at the operator.
    fn not_equatable(
        &self,
        negate: bool,
        operator_at: Option<Span>,
        lhs: &ParsedExpr,
        rhs: &ParsedExpr,
    ) -> Error {
        Error::StdPath {
            code: "E0369",
            message: format!(
                "binary operation `{}` cannot be applied to type `{}`",
                if negate { "!=" } else { "==" },
                self.compared_name(&lhs.ty)
            )
            .into(),
            label: None,
            note: None,
            reference: operator_at.unwrap_or_else(|| span(lhs.span.start(), rhs.span.end())),
        }
    }

    pub(super) fn equality(
        &mut self,
        operator: Token,
        operator_at: Option<Span>,
        mut lhs: ParsedExpr,
        mut rhs: ParsedExpr,
    ) -> Result<ParsedExpr, Error> {
        let negate = matches!(operator, Token::NotEqualsTo);
        // `degrees == 90.0` through iced's `impl PartialEq<f32> for Degrees`.
        if let Some(mut value) = self.host_comparison("eq", &lhs, &mut rhs)? {
            if negate {
                value.source = value.rendered_source().surrounded("!(", ")");
                value.prelude = origin::Fragment::default();
            }
            return Ok(value);
        }
        // The `PartialEq<Rhs>` impl of the left operand's type that takes
        // the right operand's: one between two types (`&String == &str`)
        // compares their values as they are; `Self`'s takes the right
        // operand as the left's type.
        let (left, right) = (
            self.inference.resolve(&lhs.ty),
            self.inference.resolve(&rhs.ty),
        );
        match self.std_partial_eq(&left, &right) {
            Ok(PartialEqImpl::Own) => {
                self.coerce_expr(&lhs.ty, &mut rhs)?;
            }
            Ok(PartialEqImpl::Between) => {}
            Err((inner_lhs, inner_rhs)) => {
                let required = (inner_lhs != left || inner_rhs != right)
                    .then(|| (self.compared_name(&left), self.compared_name(&right)));
                return Err(Error::CannotCompare {
                    lhs: self.compared_name(&inner_lhs),
                    rhs: self.compared_name(&inner_rhs),
                    required,
                    at: operator_at.unwrap_or_else(|| span(lhs.span.start(), rhs.span.end())),
                });
            }
        }
        // A `Cow` compares what it derefs to, as std's impls for it do.
        self.cow_compared(&mut lhs);
        self.cow_compared(&mut rhs);
        // `&T == &T` compares the referents, as Rust's `PartialEq for &A` does,
        // and `Box<T> == Box<T>` their contents.
        let mut peeled = self.inference.resolve(&lhs.ty);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) = peeled {
            peeled = self.inference.resolve(&target);
        }
        if let Ty::Option(_)
        | Ty::Result(..)
        | Ty::Param(_)
        | Ty::Projection(..)
        | Ty::Keyed(..)
        | Ty::Reverse(_)
        | Ty::Time(_)
        | Ty::Host { .. } = peeled
        {
            let expression = span(lhs.span.start(), rhs.span.end());
            let ty = peeled.clone();
            if !self.type_provides(&ty, capability::Kind::PartialEq) {
                return Err(self.not_equatable(negate, operator_at, &lhs, &rhs));
            }
            let equal = self.equal_source(
                &ty,
                lhs.rendered_source(),
                rhs.rendered_source(),
                expression,
            )?;
            let source = if negate {
                equal.surrounded("!(", ")")
            } else {
                equal.surrounded("(", ")")
            };
            return Ok(self.operator_expr(source, Ty::Bool, expression));
        }
        match peeled {
            Ty::Script(definition, ..)
                if self
                    .models
                    .std_impl(definition, "PartialEq", "eq")
                    .is_some() =>
            {
                let eq = self
                    .models
                    .std_impl(definition, "PartialEq", "eq")
                    .expect("guarded by the match arm");
                let call = Self::std_call(eq, lhs.rendered_source(), &[rhs.rendered_source()]);
                let source = if negate {
                    call.surrounded("!(", ")")
                } else {
                    call
                };
                Ok(self.operator_expr(source, Ty::Bool, span(lhs.span.start(), rhs.span.end())))
            }
            Ty::Script(definition, ..) => {
                if !self.type_provides(&peeled, capability::Kind::PartialEq) {
                    return Err(self.not_equatable(negate, operator_at, &lhs, &rhs));
                }
                let model = self.models.definition(definition);
                let helper = self.push_op(Op::Equal {
                    definition: model.definition().clone(),
                });
                let call =
                    origin::Fragment::call(&helper, [lhs.rendered_source(), rhs.rendered_source()]);
                let source = if negate {
                    call.surrounded("!", "")
                } else {
                    call
                };
                Ok(self.operator_expr(source, Ty::Bool, span(lhs.span.start(), rhs.span.end())))
            }
            Ty::Unknown => Err(self.unsupported("equality of dynamic values")),
            Ty::Dyn(..) => Err(self.not_equatable(negate, operator_at, &lhs, &rhs)),
            // Tuples and arrays of crate types: element by element, each by
            // its type's own `eq` where it has one.
            ty if !self.plain_equality(&ty) => {
                let expression = span(lhs.span.start(), rhs.span.end());
                if !self.type_provides(&ty, capability::Kind::PartialEq) {
                    return Err(self.not_equatable(negate, operator_at, &lhs, &rhs));
                }
                let equal = self.prelude_op(
                    PreludeOp::GenericEqual,
                    [lhs.rendered_source(), rhs.rendered_source()],
                );
                let source = if negate {
                    equal.surrounded("!(", ")")
                } else {
                    equal
                };
                Ok(self.operator_expr(source, Ty::Bool, expression))
            }
            _ => Ok(self.binary(lhs, if negate { "!=" } else { "==" }, rhs, Ty::Bool)),
        }
    }

    /// `Ord::cmp` of `lhs` and `rhs`, values of type `ty`, as `Ordering`'s
    /// discriminant: a hand-written impl, a generic's bound through the std
    /// dispatcher, or `compare_value` for builtins and derives. `None` when
    /// `ty` is not `Ord`.
    pub(super) fn ord_compare(
        &mut self,
        ty: &Ty,
        lhs: origin::Fragment,
        rhs: origin::Fragment,
    ) -> Option<origin::Fragment> {
        let ty = self.ord_type(ty)?;
        if let Ty::Script(id, _) = &ty
            && let Some(cmp) = self.models.std_impl(*id, "Ord", "cmp")
        {
            return Some(Self::std_call(cmp, lhs, &[rhs]));
        }
        if matches!(ty, Ty::Param(_) | Ty::Projection(..)) {
            return Some(Self::std_dispatch("Ord", "cmp", lhs, &[rhs]));
        }
        let compare = match ty {
            Ty::Int(Int::U64 | Int::Usize) => self.prelude_op(
                PreludeOp::Compare,
                [unsigned_order(lhs), unsigned_order(rhs)],
            ),
            ty => self.compare_op(&ty, true, [lhs, rhs]),
        };
        Some(self.prelude_op(PreludeOp::Unwrap, [compare]))
    }

    /// Whether the runtime's `==` compares values of `ty` as Rust does:
    /// numbers, strings, chars, bools, and tuples and arrays of them.
    pub(super) fn plain_equality(&self, ty: &Ty) -> bool {
        match self.inference.resolve(ty) {
            Ty::Tuple(members) => members.iter().all(|member| self.plain_equality(member)),
            Ty::Array { .. } | Ty::String | Ty::Str | Ty::Reference { .. } => false,
            Ty::Script(..)
            | Ty::Host { .. }
            | Ty::Option(_)
            | Ty::Result(..)
            | Ty::Keyed(..)
            | Ty::Reverse(_)
            | Ty::Cow(_)
            | Ty::Cell(..)
            | Ty::Time(_)
            | Ty::Boxed(..)
            | Ty::Counted(..)
            | Ty::Weak(..)
            | Ty::Param(_)
            | Ty::Projection(..) => false,

            // An `i128` or a `u128` past `i64` is boxed.
            Ty::Int(kind) => !kind.wide(),
            _ => true,
        }
    }

    /// A comparison (`PreludeOp::Compare`) of `lhs` and `rhs`, values of
    /// type `ty`, as `Ord` orders them when `total`.
    pub(super) fn compare_op(
        &mut self,
        ty: &Ty,
        total: bool,
        operands: [origin::Fragment; 2],
    ) -> origin::Fragment {
        let helper = self.push_op(Op::Shaped {
            op: PreludeOp::Compare,
            ty: ty.clone(),
            flags: if total { SHAPED_TOTAL } else { 0 },
        });
        origin::Fragment::call(&helper, operands)
    }

    /// Whether a value of `ty` compares through a hand-written
    /// `PartialEq` somewhere inside it: its own, an element's or a derived
    /// type's field's.
    pub(super) fn hand_written_eq_inside(&self, ty: &Ty) -> bool {
        fn inside(this: &Parser<'_>, ty: &Ty, seen: &mut BTreeSet<type_def::Id>) -> bool {
            match this.inference.resolve(ty) {
                Ty::Reference { target, .. }
                | Ty::Boxed(target)
                | Ty::Counted(_, target)
                | Ty::Option(target)
                | Ty::Reverse(target)
                | Ty::Cow(target) => inside(this, &target, seen),
                Ty::Array { element, .. } => inside(this, &element, seen),
                Ty::Result(ok, err) => inside(this, &ok, seen) || inside(this, &err, seen),
                Ty::Tuple(members) | Ty::Keyed(_, members) => {
                    members.iter().any(|member| inside(this, member, seen))
                }
                Ty::Script(id, _) => {
                    if !seen.insert(id) {
                        return false;
                    }
                    if this.models.std_impl(id, "PartialEq", "eq").is_some() {
                        return true;
                    }
                    let types: Vec<Ty> = match this.models.definition(id) {
                        Nominal::Struct { item, .. } => item
                            .fields
                            .iter()
                            .map(|field| Ty::from_resolved(&field.ty))
                            .collect(),
                        Nominal::Enum { item, .. } => item
                            .variants
                            .iter()
                            .flat_map(|variant| variant.payload.iter())
                            .map(Ty::from_resolved)
                            .collect(),
                    };
                    types.iter().any(|ty| inside(this, ty, seen))
                }
                _ => false,
            }
        }
        inside(self, ty, &mut BTreeSet::new())
    }

    /// `ty` without its references and boxes, when it is `Ord`.
    pub(super) fn ord_type(&self, ty: &Ty) -> Option<Ty> {
        let mut ty = self.inference.resolve(ty);
        while let Ty::Reference { target, .. } | Ty::Boxed(target) = ty {
            ty = self.inference.resolve(&target);
        }
        let ord = match &ty {
            Ty::Script(id, _) if self.models.std_impl(*id, "Ord", "cmp").is_some() => true,
            Ty::Param(_) | Ty::Projection(..) => self.type_implements(&ty, "Ord"),
            // An imported type orders by the crate's `Ord`.
            Ty::Host { proof, .. } => proof
                .definition
                .capabilities()
                .contains(capability::Kind::Ord),
            ty => self.std_ordered(ty, false),
        };
        ord.then_some(ty)
    }

    /// `a.min(b)`, `a.max(b)` and `a.clamp(lo, hi)`: `Ord`'s, which keep
    /// `a` when `min` finds them equal and `b` when `max` does, or `f64`'s
    /// own, which ignore a NaN.
    pub(super) fn parse_ord_method(
        &mut self,
        value: ParsedExpr,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("`Ord` method arguments")?;
        let call = span(value.span.start(), end);
        let ty = self.inference.resolve(&value.ty);
        let count = if member.as_str() == "clamp" { 2 } else { 1 };
        if arguments.len() != count {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified(&ty.name(&self.models), &member).to_string(),
                count,
                arguments.len(),
                span(member_span.start(), end),
            ));
        }
        for argument in &mut arguments {
            self.coerce_expr(&ty, argument)?;
        }
        let names = [
            self.temporary("ord"),
            self.temporary("ord"),
            self.temporary("ord"),
        ];
        let [x, y, z] = names.clone().map(origin::Fragment::plain);
        let float = matches!(ty, Ty::Float(_));
        let compare = |this: &mut Self, lhs: &origin::Fragment, rhs: &origin::Fragment| {
            if float {
                let mut source = lhs.clone().surrounded("(if ", " < ");
                source.push(rhs.clone());
                source.push_text(" { -1 } else if ");
                source.push(lhs.clone());
                source.push_text(" > ");
                source.push(rhs.clone());
                source.push_text(" { 1 } else { 0 })");
                Some(source)
            } else {
                this.ord_compare(&ty, lhs.clone(), rhs.clone())
            }
        };
        let Some(ordered) = compare(self, &x, &y) else {
            return Err(Error::MissingCapability {
                owner: ty.name(&self.models),
                capability: "Ord".into(),
                operation: member_span,
                derive: matches!(ty, Ty::Script(..)),
                required_by: None,
            });
        };
        let [first, second, third] = names;
        let mut source = origin::Fragment::plain(format!("{{ let {first} = "));
        source.push(value.rendered_source());
        for (name, argument) in [second.clone(), third.clone()].iter().zip(&arguments) {
            source.push_text(format!("; let {name} = "));
            source.push(argument.rendered_source());
        }
        source.push_text("; ");
        match member.as_str() {
            // A NaN loses to the other operand of `f64::min` and `max`.
            "min" | "max" if float => {
                let operator = if member.as_str() == "min" { "<" } else { ">" };
                source.push_text(format!(
                    "if {second} != {second} || {first} {operator} {second} {{ {first} }} else {{ {second} }}"
                ));
            }
            "min" | "max" => {
                let (greater, otherwise) = if member.as_str() == "min" {
                    (&second, &first)
                } else {
                    (&first, &second)
                };
                source.push_text("if ");
                source.push(ordered);
                source.push_text(format!(" > 0 {{ {greater} }} else {{ {otherwise} }}"));
            }
            _ => {
                let bounds = compare(self, &y, &z).expect("the type is ordered");
                let above = compare(self, &x, &z).expect("the type is ordered");
                // An integer's `clamp` names its bounds; `Ord`'s asserts.
                let failed = match self.inference.resolve(&ty) {
                    Ty::Int(kind) if !kind.wide() => {
                        format!("\"min > max. min = \" + {second} + \", max = \" + {third}")
                    }
                    _ => "\"assertion failed: min <= max\"".to_string(),
                };
                source.push_text("if !(");
                source.push(bounds);
                source.push_text(format!(" <= 0) {{ __rawr_throw {failed}; }} if "));
                source.push(ordered);
                source.push_text(format!(" < 0 {{ {second} }} else if "));
                source.push(above);
                source.push_text(format!(" > 0 {{ {third} }} else {{ {first} }}"));
            }
        }
        source.push_text(" }");
        Ok(self.operator_expr(source.surrounded("(", ")"), ty, call))
    }

    /// Whether `path` names `std::cmp::Ordering`: `Ordering` only when the
    /// crate declares no type of that name.
    pub(super) fn ordering_path<'p>(&self, path: impl Iterator<Item = &'p str>) -> bool {
        let path = path.collect::<Vec<_>>().join("::");
        crate::resolve::std_ordering(&path)
            && (path != "Ordering" || self.models.get("Ordering").is_none())
    }

    /// `Ordering::Less` after its first segment `first`: the variant's
    /// discriminant, the number of tokens up to its end, and that end.
    pub(super) fn ordering_variant(&self, first: &str) -> Option<(crate::INT, usize, Position)> {
        let mut ahead = self.input.clone();
        let mut segments = vec![ImmutableString::from(first)];
        let mut end = Position::NONE;
        while matches!(ahead.peek(), Some((Token::DoubleColon, _))) {
            ahead.next();
            match ahead.next() {
                Some((Token::Identifier(name), position)) => {
                    end = text_end(position, &name);
                    segments.push((*name).as_str().into());
                }
                _ => return None,
            }
        }
        let [path @ .., variant] = &segments[..] else {
            return None;
        };
        let value = ordering_discriminant(variant)?;
        self.ordering_path(path.iter().map(ImmutableString::as_str))
            .then_some((value, 2 * path.len(), end))
    }

    /// `a.partial_cmp(&b)` and `a.cmp(&b)` on builtin values, compared by
    /// their runtime values: an `Option<Ordering>`, unwrapped for `cmp`.
    pub(super) fn parse_compare_method(
        &mut self,
        value: ParsedExpr,
        compared: Ty,
        member: ImmutableString,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("comparison arguments")?;
        let call = span(value.span.start(), end);
        if arguments.len() != 1 {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified(&compared.name(&self.models), &member).to_string(),
                1,
                arguments.len(),
                call,
            ));
        }
        let mut other = arguments.remove(0);
        let target = match self.inference.resolve(&compared) {
            Ty::Reference { target, .. } => *target,
            ty => ty,
        };
        let unsigned = matches!(target, Ty::Int(Int::U64 | Int::Usize));
        // `other: &Self`, `Self` the receiver's type before it is
        // borrowed: a `&str` receiver is `str`'s `&self`, so `other` is a
        // `&str` too, as `a.cmp(b)` takes two string slices.
        let expected = match self.inference.resolve(&value.ty) {
            receiver @ (Ty::Str | Ty::Reference { mutable: false, .. }) => receiver,
            _ => Ty::Reference {
                mutable: false,
                target: Box::new(target),
            },
        };
        self.coerce_expr(&expected, &mut other)?;
        let operands = [value.rendered_source(), other.rendered_source()];
        let compare = if unsigned {
            self.prelude_op(PreludeOp::Compare, operands.map(unsigned_order))
        } else {
            let total = member.as_str() == "cmp";
            let ty = match self.inference.resolve(&compared) {
                Ty::Reference { target, .. } => *target,
                ty => ty,
            };
            self.compare_op(&ty, total, operands)
        };
        let (source, ty) = if member.as_str() == "cmp" {
            (self.prelude_op(PreludeOp::Unwrap, [compare]), Ty::Ordering)
        } else {
            (compare, Ty::Option(Box::new(Ty::Ordering)))
        };
        Ok(self.operator_expr(source, ty, call))
    }

    /// `Ordering`'s methods: `reverse`, `then`, `is_lt` and the like, on
    /// its discriminant.
    pub(super) fn parse_ordering_method(
        &mut self,
        value: ParsedExpr,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) =
            self.parse_std_arguments(&Ty::Ordering, &member, "`Ordering` method arguments")?;
        let receiver = value.rendered_source().surrounded("(", ")");
        let comparison = match member.as_str() {
            "is_eq" => Some("== 0"),
            "is_ne" => Some("!= 0"),
            "is_lt" => Some("< 0"),
            "is_gt" => Some("> 0"),
            "is_le" => Some("<= 0"),
            "is_ge" => Some(">= 0"),
            _ => None,
        };
        let (source, ty) = match (member.as_str(), arguments.as_mut_slice()) {
            (_, []) if comparison.is_some() => (
                receiver.surrounded("(", &format!(" {})", comparison.unwrap_or_default())),
                Ty::Bool,
            ),
            ("reverse", []) => (receiver.surrounded("(-", ")"), Ty::Ordering),
            ("clone", []) => (receiver, Ty::Ordering),
            // `self.then(other)`: `other` when `self` is `Equal`.
            ("then", [other]) => {
                self.require_type(&Ty::Ordering, &other.ty, other.span)?;
                let first = self.temporary("ordering");
                let second = self.temporary("ordering");
                let mut source = origin::Fragment::plain(format!("{{ let {first} = "));
                source.push(receiver);
                source.push_text(format!("; let {second} = "));
                source.push(other.rendered_source());
                source.push_text(format!(
                    "; if {first} == 0 {{ {second} }} else {{ {first} }} }}"
                ));
                (source.surrounded("(", ")"), Ty::Ordering)
            }
            // `self.then_with(f)`: `f()` only when `self` is `Equal`.
            ("then_with", [f]) => {
                let first = self.temporary("ordering");
                let call = self.callable_call(f, &[], &Ty::Ordering, Vec::new())?;
                let mut source = origin::Fragment::plain(format!("{{ let {first} = "));
                source.push(receiver);
                source.push_text(format!("; if {first} == 0 {{ "));
                source.push(call);
                source.push_text(format!(" }} else {{ {first} }} }}"));
                (source.surrounded("(", ")"), Ty::Ordering)
            }
            _ => {
                return Err(Error::UnknownMember {
                    owner: "Ordering".into(),
                    member,
                    reference: member_span,
                })
            }
        };
        Ok(self.operator_expr(source, ty, span(value.span.start(), end)))
    }

    /// A `bool` expression comparing `lhs` and `rhs` of type `ty` as Rust's
    /// `PartialEq` does; options compare their `Some` values.
    pub(super) fn equal_source(
        &mut self,
        ty: &Ty,
        lhs: origin::Fragment,
        rhs: origin::Fragment,
        span: Span,
    ) -> Result<origin::Fragment, Error> {
        let ty = self.inference.resolve(ty);
        Ok(match &ty {
            Ty::Reference { target, .. } | Ty::Boxed(target) => {
                self.equal_source(target, lhs, rhs, span)?
            }
            // Two `Rc`s or `Arc`s are equal when their values are, as
            // std's `PartialEq` has it.
            Ty::Counted(_, target) => {
                let lhs = self.counted_call(PreludeOp::CountedGet, lhs, []);
                let rhs = self.counted_call(PreludeOp::CountedGet, rhs, []);
                self.equal_source(target, lhs, rhs, span)?
            }
            Ty::Option(element) => {
                let left = self.temporary("left");
                let right = self.temporary("right");
                let mut source = origin::Fragment::plain(format!("{{ let {left} = "));
                source.push(lhs);
                source.push_text(format!("; let {right} = "));
                source.push(rhs);
                source.push_text("; if ");
                let left_some = self.prelude_op(PreludeOp::IsSome, [left.as_str().into()]);
                let right_some = self.prelude_op(PreludeOp::IsSome, [right.as_str().into()]);
                let left_value = self.prelude_op(PreludeOp::Unwrap, [left.as_str().into()]);
                let right_value = self.prelude_op(PreludeOp::Unwrap, [right.as_str().into()]);
                let inner = self.equal_source(element, left_value, right_value, span)?;
                source.push(left_some);
                source.push_text(" { ");
                source.push(right_some.clone());
                source.push_text(" && ");
                source.push(inner);
                source.push_text(" } else { !");
                source.push(right_some);
                source.push_text(" } }");
                source
            }
            // Equal variants compare their values, as `#[derive(PartialEq)]` does.
            Ty::Result(ok, err) => {
                let left = self.temporary("left");
                let right = self.temporary("right");
                let mut source = origin::Fragment::plain(format!("{{ let {left} = "));
                source.push(lhs);
                source.push_text(format!("; let {right} = "));
                source.push(rhs);
                source.push_text("; let __rawr_ok = ");
                let left_ok = self.prelude_op(PreludeOp::IsOk, [left.as_str().into()]);
                let right_ok = self.prelude_op(PreludeOp::IsOk, [right.as_str().into()]);
                source.push(left_ok);
                source.push_text("; if __rawr_ok != ");
                source.push(right_ok);
                source.push_text(" { false } else if __rawr_ok { ");
                let left_value = self.prelude_op(PreludeOp::UnwrapOk, [left.as_str().into()]);
                let right_value = self.prelude_op(PreludeOp::UnwrapOk, [right.as_str().into()]);
                let inner = self.equal_source(ok, left_value, right_value, span)?;
                source.push(inner);
                source.push_text(" } else { ");
                let left_error = self.prelude_op(PreludeOp::UnwrapErr, [left.as_str().into()]);
                let right_error = self.prelude_op(PreludeOp::UnwrapErr, [right.as_str().into()]);
                let inner = self.equal_source(err, left_error, right_error, span)?;
                source.push(inner);
                source.push_text(" } }");
                source
            }
            Ty::Script(id, ..) if self.models.std_impl(*id, "PartialEq", "eq").is_some() => {
                let definition = self
                    .models
                    .std_impl(*id, "PartialEq", "eq")
                    .expect("guarded by the match arm");
                Self::std_call(definition, lhs, &[rhs])
            }
            Ty::Script(id, ..) => {
                let model = self.models.definition(*id).clone();
                self.require_capability(&model, capability::Kind::PartialEq, span)?;
                let helper = self.push_op(Op::Equal {
                    definition: model.definition().clone(),
                });
                origin::Fragment::call(&helper, [lhs, rhs])
            }
            // Maps and sets compare their sorted entries; `Reverse` and a
            // cell their value; a host value by its type's own `PartialEq`.
            Ty::Keyed(..) | Ty::Reverse(_) | Ty::Cell(..) | Ty::Time(_) | Ty::Host { .. } => {
                if !self.type_provides(&ty, capability::Kind::PartialEq) {
                    return Err(Error::MissingCapability {
                        owner: ty.name(&self.models),
                        capability: "PartialEq".into(),
                        operation: span,
                        derive: matches!(ty, Ty::Script(..)),
                        required_by: None,
                    });
                }
                self.prelude_op(PreludeOp::GenericEqual, [lhs, rhs])
            }
            // A generic value, or an associated type's by its bounds.
            Ty::Param(_) | Ty::Projection(..) => {
                if !self.type_provides(&ty, capability::Kind::PartialEq) {
                    return Err(Error::MissingCapability {
                        owner: ty.name(&self.models),
                        capability: "PartialEq".into(),
                        operation: span,
                        derive: matches!(ty, Ty::Script(..)),
                        required_by: None,
                    });
                }
                if self.models.std_traits.contains("PartialEq") {
                    Self::std_dispatch("PartialEq", "eq", lhs, &[rhs])
                } else {
                    self.prelude_op(PreludeOp::GenericEqual, [lhs, rhs])
                }
            }
            Ty::Unknown | Ty::Infer(_) => {
                return Err(self.unsupported("equality of dynamic values"));
            }
            Ty::Array { .. } | Ty::String | Ty::Str => {
                self.prelude_op(PreludeOp::GenericEqual, [lhs, rhs])
            }
            ty if !self.plain_equality(ty) => {
                if !self.type_provides(ty, capability::Kind::PartialEq) {
                    return Err(Error::MissingCapability {
                        owner: ty.name(&self.models),
                        capability: "PartialEq".into(),
                        operation: span,
                        derive: matches!(ty, Ty::Script(..)),
                        required_by: None,
                    });
                }
                self.prelude_op(PreludeOp::GenericEqual, [lhs, rhs])
            }
            _ => {
                let mut source = lhs.surrounded("(", ")");
                source.push_text(" == ");
                source.push(rhs.surrounded("(", ")"));
                source.surrounded("(", ")")
            }
        })
    }
}

/// Rust's `PartialOrd` of builtin values: numbers, strings, chars, bools
/// and `()`, `Option` (`None` first) and arrays and tuples element by
/// element, and of script types deriving it. `None` when unordered: a NaN, or values Rawr's checks never
/// let reach here.
pub(super) fn compare_value(
    lhs: &Dynamic,
    rhs: &Dynamic,
    models: &Models,
) -> Option<std::cmp::Ordering> {
    // Element by element, as Rust's derive and slices compare.
    let lexicographic = |lhs: &[Dynamic], rhs: &[Dynamic]| {
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            match compare_value(lhs, rhs, models)? {
                std::cmp::Ordering::Equal => {}
                ordering => return Some(ordering),
            }
        }
        Some(lhs.len().cmp(&rhs.len()))
    };
    if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
        return match (lhs, rhs) {
            (Some(lhs), Some(rhs)) => compare_value(&lhs, &rhs, models),
            (lhs, rhs) => Some(lhs.is_some().cmp(&rhs.is_some())),
        };
    }
    // `Ok` before `Err`, then their values, as std's derive orders them.
    if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
        return match (lhs, rhs) {
            (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => compare_value(&lhs, &rhs, models),
            (lhs, rhs) => Some(lhs.is_err().cmp(&rhs.is_err())),
        };
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::time::Duration>(),
        rhs.read_lock::<std::time::Duration>(),
    ) {
        return Some(lhs.cmp(&rhs));
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<crate::Instant>(),
        rhs.read_lock::<crate::Instant>(),
    ) {
        return Some(lhs.cmp(&rhs));
    }
    if let (Some(lhs), Some(rhs)) = (cell::value(lhs), cell::value(rhs)) {
        return compare_value(&lhs, &rhs, models);
    }
    if let (Some(lhs), Some(rhs)) = (counted::value(lhs), counted::value(rhs)) {
        return compare_value(&lhs, &rhs, models);
    }
    // `Reverse(a) < Reverse(b)` when `b < a`.
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
    ) {
        return compare_value(&rhs.0, &lhs.0, models);
    }
    // A `Cow` orders as what it derefs to.
    if let (Some(lhs), Some(rhs)) = (cow::payload(lhs), cow::payload(rhs)) {
        return compare_value(&lhs, &rhs, models);
    }
    // An `i128` or a `u128` past `i64`, boxed.
    if (matches!(lhs.0, Union::Variant(..)) || matches!(rhs.0, Union::Variant(..)))
        && let Some(ordering) = wide_order(lhs, rhs)
    {
        return Some(ordering);
    }
    match (&lhs.0, &rhs.0) {
        (Union::Unit(..), Union::Unit(..)) => Some(std::cmp::Ordering::Equal),
        (Union::Bool(lhs, ..), Union::Bool(rhs, ..)) => Some(lhs.cmp(rhs)),
        (Union::Char(lhs, ..), Union::Char(rhs, ..)) => Some(lhs.cmp(rhs)),
        // A `u64` a generic parameter holds is tagged as one.
        (Union::Int(lhs, tag, ..), Union::Int(rhs, other, ..))
            if *tag == crate::types::dynamic::UNSIGNED_TAG
                || *other == crate::types::dynamic::UNSIGNED_TAG =>
        {
            Some((*lhs as u64).cmp(&(*rhs as u64)))
        }
        (Union::Int(lhs, ..), Union::Int(rhs, ..)) => Some(lhs.cmp(rhs)),
        (Union::Str(lhs, ..), Union::Str(rhs, ..)) => Some(lhs.as_str().cmp(rhs.as_str())),
        (Union::Float(lhs, ..), Union::Float(rhs, ..)) => (**lhs).partial_cmp(&**rhs),
        (Union::Array(lhs, ..), Union::Array(rhs, ..)) => lexicographic(lhs, rhs),
        // An imported value keying a hashed collection.
        (Union::Variant(..), Union::Variant(..)) => crate::host::value::host_compare(lhs, rhs),
        // A derived `PartialOrd`: fields in declaration order; an enum's
        // variants by declaration order, then their payloads.
        (Union::Struct(lhs, ..), Union::Struct(rhs, ..)) if lhs.identity() == rhs.identity() => {
            match models.by_identity.get(lhs.identity()) {
                Some(Nominal::Struct { item, .. }) => typed_lexicographic(
                    lhs.fields(),
                    rhs.fields(),
                    item.fields.iter().map(|field| &field.ty),
                    models,
                ),
                _ => lexicographic(lhs.fields(), rhs.fields()),
            }
        }
        (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) if lhs.identity() == rhs.identity() => {
            let Some(Nominal::Enum { item, .. }) = models.by_identity.get(lhs.identity()) else {
                return None;
            };
            let index = |name: &str| {
                item.variants
                    .iter()
                    .position(|variant| variant.name.text == name)
            };
            let variant = index(lhs.variant().name())?;
            match variant.cmp(&index(rhs.variant().name())?) {
                std::cmp::Ordering::Equal => typed_lexicographic(
                    lhs.payload(),
                    rhs.payload(),
                    item.variants[variant].payload.iter(),
                    models,
                ),
                ordering => Some(ordering),
            }
        }
        _ => None,
    }
}

/// Fields or payloads compared in order, a `u64` or `usize` one as the
/// unsigned number its bits are.
pub(super) fn typed_lexicographic<'t>(
    lhs: &[Dynamic],
    rhs: &[Dynamic],
    types: impl Iterator<Item = &'t type_ref::Type>,
    models: &Models,
) -> Option<std::cmp::Ordering> {
    for ((lhs, rhs), ty) in lhs.iter().zip(rhs).zip(types) {
        let ordering = match (ty, &lhs.0, &rhs.0) {
            (type_ref::Type::Path(path), Union::Int(lhs, ..), Union::Int(rhs, ..))
                if path.target == type_ref::Target::Builtin
                    && matches!(path.name.as_str(), "u64" | "usize") =>
            {
                (*lhs as u64).cmp(&(*rhs as u64))
            }
            _ => compare_value(lhs, rhs, models)?,
        };
        if ordering != std::cmp::Ordering::Equal {
            return Some(ordering);
        }
    }
    Some(lhs.len().cmp(&rhs.len()))
}

pub(crate) fn equal_value(lhs: &Dynamic, rhs: &Dynamic) -> bool {
    if let (Some(lhs), Some(rhs)) = (cell::value(lhs), cell::value(rhs)) {
        return equal_value(&lhs, &rhs);
    }
    if let (Some(lhs), Some(rhs)) = (counted::value(lhs), counted::value(rhs)) {
        return equal_value(&lhs, &rhs);
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::time::Duration>(),
        rhs.read_lock::<std::time::Duration>(),
    ) {
        return *lhs == *rhs;
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<crate::Instant>(),
        rhs.read_lock::<crate::Instant>(),
    ) {
        return *lhs == *rhs;
    }
    if let (Some(lhs), Some(rhs)) = (
        lhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
        rhs.read_lock::<std::cmp::Reverse<Dynamic>>(),
    ) {
        return equal_value(&lhs.0, &rhs.0);
    }
    // A `Cow` compares what it derefs to, `Borrowed` or `Owned`.
    if let (Some(lhs), Some(rhs)) = (cow::payload(lhs), cow::payload(rhs)) {
        return equal_value(&lhs, &rhs);
    }
    if let (Some(lhs), Some(rhs)) = (crate::option::read(lhs), crate::option::read(rhs)) {
        return match (lhs, rhs) {
            (Some(lhs), Some(rhs)) => equal_value(&lhs, &rhs),
            (lhs, rhs) => lhs.is_none() && rhs.is_none(),
        };
    }
    if let (Some(lhs), Some(rhs)) = (crate::result::read(lhs), crate::result::read(rhs)) {
        return match (lhs, rhs) {
            (Ok(lhs), Ok(rhs)) | (Err(lhs), Err(rhs)) => equal_value(&lhs, &rhs),
            _ => false,
        };
    }
    match (&lhs.0, &rhs.0) {
        (Union::Unit(..), Union::Unit(..)) => true,
        (Union::Bool(lhs, ..), Union::Bool(rhs, ..)) => lhs == rhs,
        (Union::Char(lhs, ..), Union::Char(rhs, ..)) => lhs == rhs,
        (Union::Int(lhs, ..), Union::Int(rhs, ..)) => lhs == rhs,
        (Union::Str(lhs, ..), Union::Str(rhs, ..)) => lhs == rhs,
        (Union::Float(lhs, ..), Union::Float(rhs, ..)) => lhs == rhs,
        (Union::Struct(lhs, ..), Union::Struct(rhs, ..)) => {
            lhs.identity() == rhs.identity()
                && lhs.schema() == rhs.schema()
                && lhs.fields().len() == rhs.fields().len()
                && lhs
                    .fields()
                    .iter()
                    .zip(rhs.fields())
                    .all(|(lhs, rhs)| equal_value(lhs, rhs))
        }
        (Union::Enum(lhs, ..), Union::Enum(rhs, ..)) => {
            lhs.identity() == rhs.identity()
                && lhs.schema() == rhs.schema()
                && lhs.variant() == rhs.variant()
                && lhs.payload().len() == rhs.payload().len()
                && lhs
                    .payload()
                    .iter()
                    .zip(rhs.payload())
                    .all(|(lhs, rhs)| equal_value(lhs, rhs))
        }
        // Vectors, arrays and tuples compare element by element.
        (Union::Array(lhs, ..), Union::Array(rhs, ..)) => {
            lhs.len() == rhs.len()
                && lhs
                    .iter()
                    .zip(rhs.iter())
                    .all(|(lhs, rhs)| equal_value(lhs, rhs))
        }
        // An `i128` or a `u128` past `i64`, boxed.
        (Union::Variant(..), Union::Variant(..) | Union::Int(..))
        | (Union::Int(..), Union::Variant(..))
            if let Some(ordering) = wide_order(lhs, rhs) =>
        {
            ordering == std::cmp::Ordering::Equal
        }
        (Union::Variant(..), Union::Variant(..)) => {
            crate::host::value::host_equal(lhs, rhs).unwrap_or(false)
        }
        _ => false,
    }
}

pub(super) const COMPARISON: u8 = 3;

/// Whether `ty` is one of `allowed`, any integer or float kind standing for
/// every kind of its class.
pub(super) fn allowed_type(allowed: &[Ty], ty: &Ty) -> bool {
    allowed.iter().any(|allowed| match (allowed, ty) {
        (Ty::Int(_), Ty::Int(_)) | (Ty::Float(_), Ty::Float(_)) => true,
        (allowed, ty) => allowed == ty,
    })
}
