//! Trait bounds proved through the program's impls, as rustc's obligation
//! solver proves them: an impl's own bounds are nested obligations on its
//! owner's arguments. A failed bound is reported at the innermost
//! obligation it fails on, with each impl that needed it.

use super::*;
use crate::resolve::{Requirement, Site};

/// A program's `impl Trait for Owner<..>`, as its bounds ask of the
/// owner's arguments: `T: Debug` of `impl<T: Debug> Summary for
/// Wrapper<T>`.
pub(super) struct Impl {
    source: crate::program::source::Id,
    generics: Box<[item::Generic]>,
    /// The impl's parameter each of the owner's type arguments is, when it
    /// is one: `T` of `Wrapper<T>`.
    parameters: Box<[Option<ImmutableString>]>,
    /// From `impl` through the `{` after its `where` clause.
    header: Span,
    /// The trait's name as the header writes it.
    trait_name: Span,
    /// The owner as the header writes it.
    owner: Span,
    /// The owner as rustc names it: `Wrapper<T>`.
    written: Ty,
}

impl Impl {
    pub(super) fn new(
        source: crate::program::source::Id,
        implementation: &item::Impl,
        trait_name: Span,
        parameters: Box<[Option<ImmutableString>]>,
    ) -> Self {
        Self {
            source,
            generics: implementation.generics.clone(),
            parameters,
            header: implementation.header.span,
            trait_name,
            owner: implementation.owner.span(),
            written: Ty::from_resolved(&implementation.owner),
        }
    }
}

/// What a bound asks of a type: a capability rawr reads by kind, or a
/// trait by name.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Obligation {
    Capability(capability::Kind),
    Trait(ImmutableString),
}

impl Obligation {
    /// The trait's name: `Debug`, `Summary`.
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Capability(kind) => kind.name(),
            Self::Trait(name) => name,
        }
    }
}

/// The innermost obligation a bound fails on: `Opaque: Debug` of
/// `Vec<Opaque>: Debug`, and each impl that needed it, innermost first.
pub(super) struct Unmet {
    pub(super) ty: Ty,
    pub(super) obligation: Obligation,
    pub(super) chain: Box<[Requirement]>,
    /// Each type the chain needed the obligation of, outermost first, with
    /// the argument of it the next one is, when it is that argument itself:
    /// `T` of `Wrapper<T>`, which a call of its constructor writes.
    steps: Box<[(Ty, Option<usize>)]>,
}

/// The bound of an impl a type's argument fails, and where it is written.
struct Failed {
    ty: Ty,
    obligation: Obligation,
    bound: Span,
    /// The owner's argument `ty` is, when the bound is on that argument.
    argument: Option<usize>,
}

impl Parser<'_> {
    /// Whether the program's impl of `trait_name` for its type `id` holds
    /// for the type's `arguments`: each bound the impl writes holds of the
    /// argument it bounds.
    pub(super) fn impl_holds(&self, trait_name: &str, id: type_def::Id, arguments: &[Ty]) -> bool {
        let Some(impls) = self.models.impl_bounds.get(&(trait_name.into(), id)) else {
            return true;
        };
        impls
            .iter()
            .any(|implementation| self.impl_fails(implementation, arguments).is_none())
    }

    /// Whether the program's impl of `trait_name` for a reference (or,
    /// `mutable`, a `&mut`) to `target` holds: `impl Describe for &Point`
    /// of `&Point`, with each bound it writes holding of the argument it
    /// bounds.
    pub(super) fn reference_impl_holds(
        &self,
        trait_name: &str,
        target: &Ty,
        mutable: bool,
    ) -> bool {
        let Some(id) = owner_id(target) else {
            return false;
        };
        let Some(impls) = self
            .models
            .reference_impls
            .get(&(trait_name.into(), id, mutable))
        else {
            return false;
        };
        let arguments = match target {
            Ty::Script(_, arguments) => &arguments[..],
            _ => &[],
        };
        impls
            .iter()
            .any(|implementation| self.impl_fails(implementation, arguments).is_none())
    }

    /// Whether the program writes an impl of `trait_name` for its type
    /// `id` by hand, whose own bounds then decide what it asks.
    pub(super) fn implemented_by_hand(&self, trait_name: &str, id: type_def::Id) -> bool {
        self.models
            .impl_bounds
            .contains_key(&(trait_name.into(), id))
    }

    /// The innermost obligation `ty`, which fails `obligation`, fails on,
    /// and the impls that needed it: `Opaque: Debug` of `Vec<Opaque>:
    /// Debug`, required for `Vec<Opaque>` to implement `Debug`.
    pub(super) fn unmet(&self, ty: &Ty, obligation: Obligation) -> Unmet {
        let mut ty = self.inference.resolve(ty);
        let mut obligation = obligation;
        let mut chain = Vec::new();
        let mut steps = Vec::new();
        while let Some((inner, site)) = self.cause(&ty, &obligation) {
            chain.push(Requirement {
                ty: ty.name(&self.models),
                trait_name: obligation.name().into(),
                site,
            });
            steps.push((ty, inner.argument));
            ty = self.inference.resolve(&inner.ty);
            obligation = inner.obligation;
        }
        chain.reverse();
        Unmet {
            ty,
            obligation,
            chain: chain.into_boxed_slice(),
            steps: steps.into_boxed_slice(),
        }
    }

    /// The part of the argument `at` whose type the innermost obligation
    /// of `unmet` is of, as rustc blames it: the field of a constructor's
    /// call, a tuple's member or a borrow's referent the impls needed it
    /// of, as far as each is the type's argument itself.
    fn blamed(&self, at: Span, unmet: &Unmet) -> Span {
        let mut at = at;
        for (ty, argument) in &unmet.steps {
            let Some(argument) = argument else {
                break;
            };
            match self
                .written(at)
                .and_then(|text| self.part(ty, *argument, at, &text))
            {
                Some(part) => at = part,
                None => break,
            }
        }
        at
    }

    /// The expression of the `argument`th argument of `ty` in `text`, the
    /// expression `at` building a `ty`.
    fn part(&self, ty: &Ty, argument: usize, at: Span, text: &str) -> Option<Span> {
        let (offset, part) = match ty {
            Ty::Reference { mutable, .. } => {
                let rest = text.strip_prefix('&')?;
                let rest = match mutable {
                    true => rest.strip_prefix("mut")?,
                    false => rest,
                };
                let inner = rest.trim_start();
                (text.len() - inner.len(), inner)
            }
            Ty::Tuple(types) => {
                let inner = text.strip_prefix('(')?.strip_suffix(')')?;
                let parts = members(inner);
                (parts.len() == types.len()).then_some(())?;
                let (start, part) = parts.into_iter().nth(argument)?;
                (1 + start, part)
            }
            Ty::Option(_) => called(text, "Some", argument)?,
            Ty::Result(..) => match argument {
                0 => called(text, "Ok", 0)?,
                _ => called(text, "Err", 0)?,
            },
            Ty::Script(id, _) => {
                let Nominal::Struct { item, .. } = self.models.definition(*id) else {
                    return None;
                };
                let parameter = &item.generics.get(argument)?.name.text;
                let field = item.fields.iter().position(|field| {
                    matches!(Ty::from_resolved(&field.ty), Ty::Param(name, _) if name == *parameter)
                })?;
                match item.form {
                    crate::item::StructForm::Tuple => called(text, &item.name.text, field)?,
                    crate::item::StructForm::Named => {
                        literal_field(text, &item.name.text, &item.fields[field].name.text)?
                    }
                    _ => return None,
                }
            }
            _ => return None,
        };
        let skipped = part.len() - part.trim_start().len();
        let part = part.trim();
        let start = text_end(at.start(), &text[..offset + skipped]);
        Some(Span::new(start, text_end(start, part)))
    }

    /// rustc's E0277 for a bound `site` writes, which `ty` fails at `span`:
    /// at the innermost obligation the impls proving it fail on.
    pub(super) fn unmet_bound(
        &self,
        ty: &Ty,
        obligation: Obligation,
        span: Span,
        site: Option<crate::resolve::Required>,
    ) -> Error {
        let unmet = self.unmet(ty, obligation);
        let mut site = site;
        if let Some(place) = site.as_mut().and_then(|site| site.place.as_mut()) {
            place.argument = self.blamed(place.argument, &unmet);
        }
        Error::MissingCapability {
            owner: unmet.ty.name(&self.models),
            capability: unmet.obligation.name().into(),
            operation: span,
            derive: matches!(unmet.ty, Ty::Script(..)),
            declared: self.declared_name(&unmet.ty),
            required_by: site,
            formatted: None,
            chain: crate::resolve::Chain::new(unmet.chain),
        }
    }

    /// Whether `ty` meets `obligation`.
    fn meets(&self, ty: &Ty, obligation: &Obligation) -> bool {
        match obligation {
            Obligation::Capability(capability::Kind::Send) => !self.not_send(ty),
            Obligation::Capability(kind) => self.type_provides(ty, *kind),
            Obligation::Trait(name) => self.type_implements(ty, name),
        }
    }

    /// The obligation of the impl that `ty`'s `obligation` is proved by
    /// that fails, with where the program writes the impl: the argument a
    /// std container, a derive or the program's impl needs it of.
    fn cause(&self, ty: &Ty, obligation: &Obligation) -> Option<(Failed, Option<Box<Site>>)> {
        if let Ty::Script(id, arguments) = ty {
            if let Some(impls) = self
                .models
                .impl_bounds
                .get(&(obligation.name().into(), *id))
            {
                // The impl whose bounds fail: one impl of the trait for the
                // type, as rustc names its candidate.
                let [implementation] = &impls[..] else {
                    return None;
                };
                let failed = self.impl_fails(implementation, arguments)?;
                let site = self.models.in_this_file(implementation.source).then(|| {
                    Box::new(Site::Impl {
                        header: implementation.header,
                        trait_path: implementation.trait_name,
                        owner: implementation.owner,
                        written: implementation.written.name(&self.models),
                        bound: failed.bound,
                    })
                });
                return Some((failed, site));
            }
            let Obligation::Capability(kind) = obligation else {
                return None;
            };
            return self.derive_fails(*id, arguments, *kind);
        }
        let Obligation::Capability(kind) = obligation else {
            return None;
        };
        let parts = self.std_parts(ty, *kind)?;
        let part = parts
            .iter()
            .find(|part| !self.meets(part, obligation))?
            .clone();
        Some((
            Failed {
                ty: part.clone(),
                obligation: obligation.clone(),
                bound: Span::NONE,
                argument: match ty {
                    Ty::Tuple(..) | Ty::Option(..) | Ty::Result(..) | Ty::Reference { .. } => {
                        parts.iter().position(|each| *each == part)
                    }
                    _ => None,
                },
            },
            None,
        ))
    }

    /// The argument of the program's type `id` that its derive of `kind`
    /// needs `kind` of and that lacks it, as the derive bounds each
    /// parameter.
    fn derive_fails(
        &self,
        id: type_def::Id,
        arguments: &[Ty],
        kind: capability::Kind,
    ) -> Option<(Failed, Option<Box<Site>>)> {
        let model = self.models.definition(id);
        if !model.capabilities().contains(kind) {
            return None;
        }
        let obligation = Obligation::Capability(kind);
        let (generic, argument) = model
            .generics()
            .iter()
            .zip(arguments)
            .find(|(_, argument)| !self.meets(argument, &obligation))?;
        let derive = model
            .header()
            .attributes
            .iter()
            .filter(|attribute| {
                matches!(&attribute.path.segments[..], [only] if only.name.text == "derive")
            })
            .flat_map(|attribute| attribute.arguments.iter())
            .find(|path| {
                path.segments
                    .last()
                    .is_some_and(|last| last.name.text == kind.name())
            })
            .map(|path| path.span);
        let site = derive
            .filter(|_| self.models.name_declared_here(model).is_some())
            .map(|derive| {
                Box::new(Site::Derive {
                    derive,
                    name: model.name_span(),
                    parameter: generic.name.span,
                    written: Ty::Script(
                        id,
                        model
                            .generics()
                            .iter()
                            .map(|generic| Ty::param(generic.name.text.clone()))
                            .collect(),
                    )
                    .name(&self.models),
                })
            });
        Some((
            Failed {
                ty: argument.clone(),
                obligation,
                bound: Span::NONE,
                argument: model
                    .generics()
                    .iter()
                    .position(|each| each.name.text == generic.name.text),
            },
            site,
        ))
    }

    /// The first bound `implementation` writes that the owner's `arguments`
    /// fail. A bound being proved already holds, as rustc proves a cycle
    /// of impls.
    fn impl_fails(&self, implementation: &Impl, arguments: &[Ty]) -> Option<Failed> {
        let mut substitution = BTreeMap::new();
        for (parameter, argument) in implementation.parameters.iter().zip(arguments) {
            if let Some(parameter) = parameter {
                substitution.insert(parameter.clone(), self.inference.resolve(argument));
            }
        }
        // `where Vec<T>: Debug`: the parameter lifted for the clause is
        // `Vec<T>` of the owner's argument.
        for generic in &implementation.generics {
            if let Some(fixed) = &generic.fixed {
                let fixed = instantiate(&Ty::from_resolved(fixed), &substitution);
                substitution.insert(generic.name.text.clone(), fixed);
            }
        }
        for generic in &implementation.generics {
            let Some(ty) = substitution.get(&generic.name.text) else {
                continue;
            };
            if contains_inference(ty) {
                continue;
            }
            let kinds = bound_kinds(generic.bounds).map(|kind| {
                let bound = generic
                    .written
                    .iter()
                    .find(|(written, _)| *written == kind)
                    .map_or(Span::NONE, |(_, span)| *span);
                (Obligation::Capability(kind), bound)
            });
            // A program's trait, or `Display`; rawr proves no other std
            // trait of an impl's bound here, and a trait with a blanket impl
            // where a call's bound selects it (`blanket_implements`).
            let blanket = type_def::Id::builtin(crate::resolve::BLANKET_OWNER);
            let traits = generic
                .traits
                .iter()
                .filter(|name| {
                    !generic
                        .trait_arguments
                        .iter()
                        .any(|(bounded, _)| *bounded == name.text)
                        && !self.models.implements(&name.text, blanket)
                        && (name.text == "Display"
                            || self.models.traits.contains_key(&name.text)
                                && !self.models.std_traits.contains(&name.text))
                })
                .map(|name| (Obligation::Trait(name.text.clone()), name.span));
            for (obligation, bound) in kinds.chain(traits).collect::<Vec<_>>() {
                if !self.proved(ty, &obligation) {
                    // A bound on a parameter the owner takes as its argument:
                    // `T` of `Wrapper<T>`; a `where Vec<T>:` bound is on
                    // another type.
                    let argument = match generic.fixed {
                        Some(_) => None,
                        None => implementation
                            .parameters
                            .iter()
                            .position(|parameter| parameter.as_ref() == Some(&generic.name.text)),
                    };
                    return Some(Failed {
                        ty: ty.clone(),
                        obligation,
                        bound,
                        argument,
                    });
                }
            }
        }
        None
    }

    /// Whether `ty` meets `obligation`, an impl's bound, holding it while
    /// it is proved.
    fn proved(&self, ty: &Ty, obligation: &Obligation) -> bool {
        let key = (ty.clone(), ImmutableString::from(obligation.name()));
        if self.proving.borrow().contains(&key) {
            return true;
        }
        self.proving.borrow_mut().push(key);
        let meets = self.meets(ty, obligation);
        self.proving.borrow_mut().pop();
        meets
    }

    /// The types std's impl of `kind` for `ty` asks `kind` of: `T` of
    /// `impl<T: Debug> Debug for Vec<T>`. None where std has no such impl
    /// or it asks nothing of what `ty` holds.
    fn std_parts(&self, ty: &Ty, kind: capability::Kind) -> Option<Vec<Ty>> {
        use capability::Kind;
        let parts = match ty {
            Ty::Array {
                element,
                kind: sequence,
            } => match kind {
                Kind::Default => return None,
                Kind::Copy if !sequence.is_array() => return None,
                _ => vec![(**element).clone()],
            },
            Ty::Tuple(members) => members.to_vec(),
            Ty::Option(element) if kind != Kind::Default => vec![(**element).clone()],
            Ty::Result(ok, err) if kind != Kind::Default => {
                vec![(**ok).clone(), (**err).clone()]
            }
            Ty::Reference { target, .. }
                if matches!(
                    kind,
                    Kind::PartialEq | Kind::Eq | Kind::PartialOrd | Kind::Ord | Kind::Hash
                ) || kind == Kind::Debug =>
            {
                vec![(**target).clone()]
            }
            Ty::Keyed(keyed, arguments)
                if !matches!(kind, Kind::Default | Kind::Copy)
                    && !(keyed.hashed()
                        && matches!(kind, Kind::PartialOrd | Kind::Ord | Kind::Hash)) =>
            {
                arguments.to_vec()
            }
            Ty::Reverse(inner) => vec![(**inner).clone()],
            Ty::Boxed(target) if kind != Kind::Copy => vec![(**target).clone()],
            Ty::Counted(_, target)
                if !matches!(kind, Kind::Copy | Kind::Clone | Kind::Send | Kind::Static) =>
            {
                vec![(**target).clone()]
            }
            _ => return None,
        };
        Some(parts)
    }
}

/// Each member of the list `text`, split at its commas outside brackets
/// and strings, with where it starts: `a` and ` (b, c)` of `a, (b, c)`.
fn members(text: &str) -> Vec<(usize, &str)> {
    let mut members = Vec::new();
    let (mut depth, mut quoted, mut escaped, mut start) = (0_usize, false, false, 0);
    for (index, character) in text.char_indices() {
        if quoted {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                members.push((start, &text[start..index]));
                start = index + 1;
            }
            _ => {}
        }
    }
    if !text[start..].trim().is_empty() {
        members.push((start, &text[start..]));
    }
    members
}

/// The path `head` names, without its turbofish: `Ok` of `Ok::<u8, E>`.
fn named(head: &str) -> &str {
    let head = head.trim();
    let head = head.split_once("::<").map_or(head, |(path, _)| path);
    head.rsplit("::").next().unwrap_or(head)
}

/// The `index`th argument of `text`, a call of the constructor `name`,
/// with where it starts in `text`: `Opaque` of `Wrapper(Opaque)`.
fn called<'a>(text: &'a str, name: &str, index: usize) -> Option<(usize, &'a str)> {
    let open = text.find('(')?;
    (named(&text[..open]) == name).then_some(())?;
    let inner = text[open + 1..].strip_suffix(')')?;
    let (start, part) = members(inner).into_iter().nth(index)?;
    Some((open + 1 + start, part))
}

/// The value of the field `field` in `text`, a literal of the struct
/// `name`, with where it starts in `text`: `Opaque` of `W { x: Opaque }`.
fn literal_field<'a>(text: &'a str, name: &str, field: &str) -> Option<(usize, &'a str)> {
    let open = text.find('{')?;
    (named(&text[..open]) == name).then_some(())?;
    let inner = text[open + 1..].strip_suffix('}')?;
    members(inner).into_iter().find_map(|(start, member)| {
        let rest = member.trim_start().strip_prefix(field)?;
        // `W { x }`: the field's own name is its value.
        if rest.trim().is_empty() {
            return Some((open + 1 + start, member));
        }
        let value = rest.trim_start().strip_prefix(':')?;
        Some((open + 1 + start + member.len() - value.len(), value))
    })
}
