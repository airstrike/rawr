//! What one classified path adds to its root's facade (#1618).
//!
//! An outcome's contribution is a function of the outcome alone, read with
//! every plan it names: the items and types it declares at its paths, the
//! trait or module it names, the builtin or crate type alias it is, and the
//! variant it re-exports. A plan applies each outcome's contribution in
//! order, so one kept from an earlier plan of the same outcome is the one
//! it would generate again.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    builtin_alias, copy, declared, defaults, item, parameters, path, ty, type_definition, Facade,
    Item, ItemKind, Type, TypeAlias, TypeDefinition,
};
use crate::classify::representability::{self, Representability};

/// What one outcome adds to its root's facade, in order.
#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Contribution {
    ops: Box<[Op]>,
}

/// Each outcome's contribution one plan applied, by the digest of the
/// outcome with every plan it names in place.
#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Contributions {
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    by_digest: BTreeMap<Digest, Kept>,
    /// How many the plan that gave these generated anew.
    #[serde(skip)]
    fresh: usize,
}

/// An outcome's digest with its plans in place.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Digest(#[serde(with = "crate::hex")] [u8; 32]);

/// One outcome's contribution, and the definition its path names.
#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Kept {
    contribution: Arc<Contribution>,
    target: Option<crate::surface::identity::Definition>,
    /// Whether the plan that gave it generated it.
    #[serde(skip)]
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    fresh: bool,
}

impl Contributions {
    /// The contribution kept for the outcome whose digest is `digest`.
    pub(super) fn get(&self, digest: [u8; 32]) -> Option<&Arc<Contribution>> {
        self.by_digest
            .get(&Digest(digest))
            .map(|kept| &kept.contribution)
    }

    /// Keep `contribution`, of the outcome whose digest is `digest` and
    /// whose path names `target`; `fresh` when this plan generated it.
    pub(super) fn keep(
        &mut self,
        digest: [u8; 32],
        contribution: Arc<Contribution>,
        target: Option<crate::surface::identity::Definition>,
        fresh: bool,
    ) {
        self.fresh += usize::from(fresh);
        self.by_digest.insert(
            Digest(digest),
            Kept {
                contribution,
                target,
                fresh,
            },
        );
    }

    /// How many the plan that gave these generated anew.
    pub(crate) const fn fresh(&self) -> usize {
        self.fresh
    }

    /// The contributions `keep` keeps, by the definition each one's path
    /// names.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn select(
        &self,
        mut keep: impl FnMut(Option<&crate::surface::identity::Definition>) -> bool,
    ) -> Self {
        Self {
            by_digest: self
                .by_digest
                .iter()
                .filter(|(_, kept)| keep(kept.target.as_ref()))
                .map(|(digest, kept)| (*digest, kept.clone()))
                .collect(),
            fresh: 0,
        }
    }

    /// These contributions with those of `later` beside them: of each
    /// definition's paths, `later`'s, then up to [`CANDIDATES`] in all, as
    /// another plan may read an outcome as an earlier one did.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn with(&self, later: &Self) -> Self {
        let mut by_digest = later.by_digest.clone();
        let mut per_target = BTreeMap::<Option<crate::surface::identity::Definition>, usize>::new();
        for kept in later.by_digest.values() {
            *per_target.entry(kept.target).or_default() += 1;
        }
        for (digest, kept) in &self.by_digest {
            let count = per_target.entry(kept.target).or_default();
            if *count < CANDIDATES && !by_digest.contains_key(digest) {
                *count += 1;
                by_digest.insert(*digest, kept.clone());
            }
        }
        Self {
            by_digest,
            fresh: 0,
        }
    }

    /// Whether the plan that gave these generated any of them.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn any_fresh(&self) -> bool {
        self.by_digest.values().any(|kept| kept.fresh)
    }

    /// Whether these hold no contribution.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_digest.is_empty()
    }
}

/// A map written as a sequence of its entries, as JSON keys are strings.
fn entries<K: Serialize, V: Serialize, S: serde::Serializer>(
    map: &BTreeMap<K, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(map.iter())
}

/// A map read from the sequence of entries [`entries`] writes.
fn from_entries<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Vec::<(K, V)>::deserialize(deserializer)?
        .into_iter()
        .collect())
}

/// The most contributions an entry keeps of one definition's paths.
#[cfg(feature = "rustc-driver")]
const CANDIDATES: usize = 4;

/// One addition to a root's facade.
#[derive(Clone, Deserialize, Serialize)]
enum Op {
    Item(Item),
    Type(TypeDefinition),
    /// A trait or module Rawr code names, by its path.
    Trait(Box<str>),
    /// A builtin's alias: its path and the builtin's Rust name.
    Builtin(Box<str>, Box<str>),
    TypeAlias(TypeAlias),
    /// A trait Rawr code names, whose header is written once every type
    /// is in (D604).
    NamedTrait {
        path: Box<str>,
        identity: [u8; 32],
        outputs: Option<representability::signature::Outputs>,
    },
    /// A re-exported variant, named through a path to its enum once every
    /// type is in.
    Variant {
        path: Box<str>,
        identity: [u8; 32],
        variant: Box<str>,
    },
}

impl Contribution {
    /// What `outcome` adds to its root's facade.
    pub(crate) fn of(outcome: crate::classify::Outcome<'_>) -> Result<Self, crate::surface::Error> {
        Ok(Self {
            ops: ops(outcome)?.into_boxed_slice(),
        })
    }
}

fn ops(outcome: crate::classify::Outcome<'_>) -> Result<Vec<Op>, crate::surface::Error> {
    let mut ops = Vec::new();
    let named_trait = matches!(
        outcome.representability(),
        Representability::Rejected(reason)
            if reason.kind() == representability::reject::Kind::Trait
    );
    // A module Rawr code names (`use crate::icon;`) is a module to Rawr,
    // as a trait is, so an import that nothing below it uses resolves, as
    // rustc's does (#1436).
    let named_module = matches!(
        outcome.representability(),
        Representability::Rejected(reason)
            if reason.kind() == representability::reject::Kind::Module
    );
    let left_out = !named_trait
        && !named_module
        && matches!(
            outcome.representability(),
            Representability::Excluded(_) | Representability::Rejected(_)
        );
    if left_out {
        return Ok(ops);
    }
    let relative = path(outcome.export().path());
    if named_trait {
        if let Some(definition) = outcome.identity().definition() {
            ops.push(Op::NamedTrait {
                path: copy(&relative)?,
                identity: *definition.as_bytes(),
                outputs: outcome.outputs().cloned(),
            });
        }
        // The methods of the trait's impls for `&str`, each a
        // function whose first input is Rawr's string
        // (`surface::text`).
        for associated in outcome
            .implementations()
            .flat_map(crate::classify::ImplementationOutcome::associated)
            .filter(|associated| crate::surface::text::is_member(associated.representability()))
        {
            let path = crate::surface::text::path(&relative, associated.name());
            let Some(mut item) = item(&path, associated)? else {
                continue;
            };
            if !matches!(item.body.kind, ItemKind::OwnedMethod) {
                continue;
            }
            let body = item.made_mut();
            body.kind = ItemKind::Function;
            let mut inputs = Vec::new();
            inputs
                .try_reserve_exact(body.signature.inputs.len() + 1)
                .map_err(|_| crate::surface::Error::Allocation)?;
            // The impl's `&str`, as rustc's `<&str as Trait>::
            // method(text)` takes it, apart from a `String` (#706).
            inputs.push(Type::Reference {
                mutable: false,
                target: Box::new(Type::String),
            });
            inputs.extend(std::mem::take(&mut body.signature.inputs).into_vec());
            body.signature.inputs = inputs.into_boxed_slice();
            ops.push(Op::Item(item));
        }
        // The trait's methods on its object, each a function of the
        // object beside the trait (`surface::object`).
        for associated in outcome
            .implementations()
            .flat_map(crate::classify::ImplementationOutcome::associated)
            .filter(|associated| crate::surface::object::of(associated.representability()))
        {
            let path = crate::surface::object::path(&relative, associated.name());
            let Some(mut item) = item(&path, associated)? else {
                continue;
            };
            item.made_mut().kind = ItemKind::Function;
            ops.push(Op::Item(item));
        }
        // The members of the trait's impls for a number, `bool` or
        // `char`, each a function of the scalar beside the trait
        // (`surface::scalar`).
        for associated in outcome
            .implementations()
            .flat_map(crate::classify::ImplementationOutcome::associated)
        {
            let Some(scalar) = crate::surface::scalar::of(associated.representability()) else {
                continue;
            };
            let path = crate::surface::scalar::path(&relative, scalar, associated.name());
            let Some(mut item) = item(&path, associated)? else {
                continue;
            };
            item.made_mut().kind = ItemKind::Function;
            ops.push(Op::Item(item));
        }
        // The members a blanket impl's application to slices gives,
        // each a function of the slice's borrow beside the trait
        // (`surface::slice`).
        for associated in outcome
            .implementations()
            .flat_map(crate::classify::ImplementationOutcome::associated)
            .filter(|associated| crate::surface::slice::is_member(associated.representability()))
        {
            let path = crate::surface::slice::path(&relative, associated.name());
            let Some(mut item) = item(&path, associated)? else {
                continue;
            };
            item.made_mut().kind = ItemKind::Function;
            ops.push(Op::Item(item));
        }
        // A provided method called on the program's own type is
        // the same function over `Self` in the retained runner
        // facade as it was during demand compilation.
        if let Some(definition) = outcome.identity().definition() {
            for associated in outcome
                .implementations()
                .filter(|implementation| {
                    crate::surface::member::is_members(implementation.identity(), &definition)
                })
                .flat_map(crate::classify::ImplementationOutcome::associated)
            {
                let path = crate::surface::member::path(&relative, associated.name());
                // The members of the opaque type it gives, as of a
                // function's (`surface::opaque`), as iced's
                // `Widget::explain` gives one whose `boxed` a
                // program calls.
                let owner = crate::surface::opaque::path(&path, 0);
                for member in associated
                    .implementations()
                    .flat_map(crate::classify::ImplementationOutcome::associated)
                {
                    let member_path = format!("{owner}::{}", member.name());
                    if let Some(item) = item(&member_path, member)? {
                        ops.push(Op::Item(item));
                    }
                }
                let Some(mut item) = item(&path, associated)? else {
                    continue;
                };
                item.made_mut().kind = ItemKind::Function;
                ops.push(Op::Item(item));
            }
        }
    }
    if named_trait || named_module {
        ops.push(Op::Trait(relative));
        return Ok(ops);
    }
    // A type alias of a builtin, as iced's `markdown::Uri` is
    // `String`, names the builtin.
    if let Representability::Value(plan) = outcome.representability()
        && plan.parameters().is_empty()
        && let Some(builtin) = builtin_alias(plan.ty())
    {
        ops.push(Op::Builtin(copy(&relative)?, copy(builtin)?));
        return Ok(ops);
    }
    // Any other type alias, as `pub type Result = std::result::Result<
    // (), Error>;`, names its target with its own parameters.
    if let Representability::Value(plan) = outcome.representability() {
        let (Some(defaults), Some(target)) = (
            declared(defaults(plan.parameters()))?,
            declared(ty(plan.ty(), plan.parameters()))?,
        ) else {
            return Ok(ops);
        };
        let alias = TypeAlias {
            path: copy(&relative)?,
            parameters: parameters(plan.parameters())?,
            defaults,
            target,
        };
        ops.push(Op::TypeAlias(alias));
        // An alias that passes its parameters on to a type has that
        // type's members (`classify`), as `widget::Text::new`.
        for associated in outcome
            .implementations()
            .flat_map(crate::classify::ImplementationOutcome::associated)
        {
            let associated_path = format!("{relative}::{}", associated.name());
            if let Some(item) = item(&associated_path, associated)?.filter(|item| {
                matches!(
                    item.body.kind,
                    ItemKind::AssociatedFunction | ItemKind::AssociatedConstant
                )
            }) {
                ops.push(Op::Item(item));
            }
        }
        return Ok(ops);
    }
    // A re-exported variant, as `pub use Alignment::Center;`, is its
    // enum's variant.
    if let Some(variant) = outcome.variant() {
        if let Some(definition) = outcome.identity().definition() {
            ops.push(Op::Variant {
                path: relative,
                identity: *definition.as_bytes(),
                variant: copy(variant)?,
            });
        }
        return Ok(ops);
    }
    if let Some(ty) = type_definition(&relative, outcome)? {
        ops.push(Op::Type(ty));
    }
    if let Some(item) = item(&relative, outcome)? {
        ops.push(Op::Item(item));
    }
    // The names of the type's associated items so far, by which an
    // opaque type a method gives is apart from its namesakes'.
    let mut seen = Vec::new();
    for associated in outcome
        .implementations()
        .flat_map(crate::classify::ImplementationOutcome::associated)
    {
        let sibling = seen
            .iter()
            .filter(|name| **name == associated.name())
            .count();
        seen.try_reserve(1)
            .map_err(|_| crate::surface::Error::Allocation)?;
        seen.push(associated.name());
        // An impl that fixes some of its type's arguments is
        // relevant to each type it names: iced's `impl TextEditor<
        // 'a, PlainText, ..>` is `TextEditor`'s, not `PlainText`'s.
        if let Representability::Invocation(plan) = associated.representability()
            && let Some(representability::convert::Type::Nominal { definition, .. }) =
                plan.signature().instance()
            && outcome.identity().definition() != Some(*definition)
        {
            continue;
        }
        // A function's members are those of the opaque type it
        // gives (`surface::opaque`).
        let owner = if matches!(
            outcome.representability(),
            Representability::Function(_)
                | Representability::FunctionItem(_)
                | Representability::Invocation(_)
        ) {
            crate::surface::opaque::path(&relative, 0)
        } else {
            relative.to_string()
        };
        let associated_path = format!("{owner}::{}", associated.name());
        if let Some(item) = item(&associated_path, associated)? {
            ops.push(Op::Item(item));
        }
        // The members of the opaque type an inherent method gives,
        // in its owner's module, as Rawr declares them, each impl's
        // apart (`opaque::inherent_owner`).
        let opaque = crate::surface::opaque::path(
            &crate::surface::opaque::inherent_owner(&owner, associated.name(), sibling),
            0,
        );
        for member in associated
            .implementations()
            .flat_map(crate::classify::ImplementationOutcome::associated)
        {
            let member_path = format!("{opaque}::{}", member.name());
            if let Some(item) = item(&member_path, member)? {
                ops.push(Op::Item(item));
            }
        }
    }
    Ok(ops)
}

/// One root's facade while each outcome's contribution is applied: lists
/// that grow, sealed once every outcome is in.
pub(super) struct Building {
    pub(super) root: Box<str>,
    types: Vec<TypeDefinition>,
    items: Vec<Item>,
    traits: Vec<Box<str>>,
    builtins: Vec<(Box<str>, Box<str>)>,
    type_aliases: Vec<TypeAlias>,
}

/// A re-exported variant: its root, its path, its enum and its name.
pub(super) type Variant = (Box<str>, Box<str>, [u8; 32], Box<str>);

/// A trait Rawr code names: its root, its path, its identity and its
/// methods' opaque outputs.
pub(super) type NamedTrait = (
    Box<str>,
    Box<str>,
    [u8; 32],
    Option<representability::signature::Outputs>,
);

impl Building {
    pub(super) fn new(root: Box<str>) -> Self {
        Self {
            root,
            types: Vec::new(),
            items: Vec::new(),
            traits: Vec::new(),
            builtins: Vec::new(),
            type_aliases: Vec::new(),
        }
    }

    /// Apply `contribution`, an outcome's of this root: a trait, builtin or
    /// type alias already in keeps its place.
    pub(super) fn apply(
        &mut self,
        contribution: &Contribution,
        variants: &mut Vec<Variant>,
        named_traits: &mut Vec<NamedTrait>,
    ) -> Result<(), crate::surface::Error> {
        let allocation = |_| crate::surface::Error::Allocation;
        for op in &contribution.ops {
            match op {
                Op::Item(item) => {
                    self.items.try_reserve(1).map_err(allocation)?;
                    self.items.push(item.clone());
                }
                Op::Type(ty) => {
                    self.types.try_reserve(1).map_err(allocation)?;
                    self.types.push(ty.clone());
                }
                Op::Trait(path) => {
                    if let Err(position) = self.traits.binary_search(path) {
                        self.traits.try_reserve(1).map_err(allocation)?;
                        self.traits.insert(position, path.clone());
                    }
                }
                Op::Builtin(path, builtin) => {
                    if let Err(position) = self
                        .builtins
                        .binary_search_by(|(known, _)| known.as_ref().cmp(path.as_ref()))
                    {
                        self.builtins.try_reserve(1).map_err(allocation)?;
                        self.builtins
                            .insert(position, (path.clone(), builtin.clone()));
                    }
                }
                Op::TypeAlias(alias) => {
                    if let Err(position) = self
                        .type_aliases
                        .binary_search_by(|known| known.path.as_ref().cmp(alias.path.as_ref()))
                    {
                        self.type_aliases.try_reserve(1).map_err(allocation)?;
                        self.type_aliases.insert(position, alias.clone());
                    }
                }
                Op::NamedTrait {
                    path,
                    identity,
                    outputs,
                } => {
                    named_traits.try_reserve(1).map_err(allocation)?;
                    named_traits.push((
                        copy(&self.root)?,
                        path.clone(),
                        *identity,
                        outputs.clone(),
                    ));
                }
                Op::Variant {
                    path,
                    identity,
                    variant,
                } => {
                    variants.try_reserve(1).map_err(allocation)?;
                    variants.push((copy(&self.root)?, path.clone(), *identity, variant.clone()));
                }
            }
        }
        Ok(())
    }

    /// The facade every outcome's contribution built.
    pub(super) fn into_facade(self) -> Facade {
        Facade {
            root: self.root,
            types: self.types.into_boxed_slice(),
            items: self.items.into_boxed_slice(),
            traits: self.traits.into_boxed_slice(),
            builtins: self.builtins.into_boxed_slice(),
            variants: Box::default(),
            type_aliases: self.type_aliases.into_boxed_slice(),
            macros: Box::default(),
            derives: Box::default(),
            trait_headers: Box::default(),
            impls: Box::default(),
            std_reexports: Box::default(),
            reexports: Box::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::classify::Policy;

    /// A plan of a graph that names more paths than one before it, as a
    /// later demand round's does, takes each path the other plan had from
    /// its contribution, generates only the new ones, and is the plan of
    /// the grown graph alone.
    #[test]
    fn a_grown_graphs_plan_generates_only_its_new_paths_to_the_same_bytes() {
        let limits = crate::limits::Limits::standard();
        for fixture in [
            crate::test_fixture::iced_profile(),
            crate::test_fixture::graph(),
            crate::test_fixture::native_shapes(),
            crate::test_fixture::opaque_left_out_functions(),
        ] {
            let exports = fixture.exports().len();
            let full = crate::test_fixture::with_exports(&fixture, |_| true);
            let contract = full
                .classify(&Policy::standard(), &limits)
                .expect("the grown graph classifies");
            let alone = super::super::Plan::encode(&contract).expect("the grown plan encodes");
            for dropped in 0..exports {
                let smaller = crate::test_fixture::with_exports(&fixture, |index| index != dropped)
                    .classify(&Policy::standard(), &limits)
                    .expect("the smaller graph classifies");
                let (_, kept) = super::super::Plan::encode_outcomes(
                    smaller.raw_graph(),
                    smaller.outcomes(),
                    Some(&[]),
                )
                .expect("the smaller plan encodes");
                let kept = kept.expect("a plan asked to keep its contributions keeps them");
                let (grown, generated) = super::super::Plan::encode_outcomes(
                    contract.raw_graph(),
                    contract.outcomes(),
                    Some(&[&kept]),
                )
                .expect("the grown plan encodes from the smaller plan's contributions");
                assert_eq!(alone, grown, "export {dropped} dropped");
                let generated =
                    generated.expect("a plan asked to keep its contributions keeps them");
                assert!(
                    generated.fresh() < exports || exports == 1,
                    "the paths both plans name are taken: {} of {exports}",
                    generated.fresh()
                );
            }
        }
    }
}
