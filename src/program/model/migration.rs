//! Live values a save's types replace (#2026): a save that changes the
//! fields of a script type a live value holds rewrites that value in place
//! at admission, where no call of the program is under way on the thread
//! that owns it (D553), so the program keeps its state and every handle to
//! the value stays valid.
//!
//! Fields match by name, so an order change is free. A field kept with
//! its type keeps its value, a script type's whatever its own fields are
//! now, as that value is rewritten too. A new field, or one whose type
//! changed, takes its type's `Default`. A removed field drops; one whose
//! type may have a program's `Drop` refuses while the program has one, as
//! the drop glue isn't run here. An enum's variants match by name, and a
//! live value in a variant the save removed, or whose payload changed,
//! refuses.
//!
//! The walk reaches the values from the roots it is given and counts what
//! it finds against each type's census: a value it can't reach, as one a
//! native type keeps inside it, refuses the save, so a value of an old
//! shape never meets the new code.

use std::collections::{HashMap, HashSet};
use std::fmt;

use super::{schema, Model, Shape};
use crate::resolve::type_ref;
use crate::types::dynamic::Union;
use crate::types::nominal;
use crate::{type_def, Dynamic, ImmutableString};

/// How this model's types replace the live values of the script types a
/// save changed.
pub(crate) struct Migration<'model> {
    model: &'model Model,
    plans: HashMap<type_def::Identity, Plan>,
}

/// How one type's values change.
struct Plan {
    /// The type's path, as reports name it.
    path: String,
    /// Its schema where the live values were made.
    old: schema::Fingerprint,
    /// Its schema in the new model.
    new: schema::Fingerprint,
    layout: Layout,
}

enum Layout {
    Struct {
        /// Where each new field's value comes from, in the new order.
        fields: Box<[Source]>,
        /// The old fields the new type has no place for.
        dropped: Box<[Dropped]>,
        /// Whether a kept field moved.
        reordered: bool,
    },
    Enum {
        /// The old variants a value may stay in, by their names: the new
        /// type has each with its payload's types.
        kept: HashSet<ImmutableString>,
        /// The old variants the new type has with another payload.
        changed: HashSet<ImmutableString>,
    },
}

enum Source {
    /// The old field at this index.
    Kept(usize),
    /// Its type's `Default`: the field is new, or its type changed.
    Made {
        name: ImmutableString,
        ty: Box<type_ref::Type>,
        changed: bool,
    },
}

struct Dropped {
    index: usize,
    name: ImmutableString,
    /// Whether its type names no script or crate type, so its drop runs
    /// no `Drop` of the program's.
    plain: bool,
}

/// Why a save can't rewrite the live values of its old types.
#[derive(Clone, Debug)]
pub(crate) enum Refusal {
    /// A live value is where the walk can't reach it.
    Unreached { path: String },
    /// A new field, or one whose type changed, has no `Default`.
    Undefaulted {
        path: String,
        field: ImmutableString,
        changed: bool,
    },
    /// A dropped field's value may run the program's `Drop`.
    Dropping {
        path: String,
        field: ImmutableString,
    },
    /// A live value is in a variant the save removed.
    RemovedVariant {
        path: String,
        variant: ImmutableString,
    },
    /// A live value is in a variant whose payload changed.
    ChangedVariant {
        path: String,
        variant: ImmutableString,
    },
    /// The new value isn't one of the new type.
    Construction { path: String, error: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreached { path } => {
                write!(
                    formatter,
                    "`{path}` changed its fields, but a live value holds it"
                )
            }
            Self::Undefaulted {
                path,
                field,
                changed: false,
            } => write!(
                formatter,
                "`{path}` gained `{field}`, whose type has no `Default`"
            ),
            Self::Undefaulted {
                path,
                field,
                changed: true,
            } => write!(
                formatter,
                "`{path}` changed the type of `{field}`, which has no `Default`"
            ),
            Self::Dropping { path, field } => write!(
                formatter,
                "`{path}` dropped `{field}`, whose `Drop` rawr can't run in a running program yet"
            ),
            Self::RemovedVariant { path, variant } => write!(
                formatter,
                "a live `{path}` is `{variant}`, which the save removed"
            ),
            Self::ChangedVariant { path, variant } => write!(
                formatter,
                "a live `{path}` is `{variant}`, whose fields changed"
            ),
            Self::Construction { path, error } => {
                write!(
                    formatter,
                    "a live `{path}` can't take its new fields: {error}"
                )
            }
        }
    }
}

/// What a survey found: how many live values of each changed type have
/// its old shape, each of which takes the new fields' `Default`s.
pub(crate) struct Survey {
    old: HashMap<type_def::Identity, usize>,
}

/// The `Default`s the rewrite puts in new fields, made before it runs.
pub(crate) struct Defaults {
    made: HashMap<type_def::Identity, Vec<Box<[Option<Dynamic>]>>>,
}

/// What a rewrite did: one line that says it, and the old fields it took
/// out, for the caller to drop once nothing it holds is borrowed.
pub(crate) struct Rewritten {
    pub(crate) report: String,
    pub(crate) dropped: Vec<Dynamic>,
}

impl Model {
    /// How this model's types replace the live values of `changed`: each
    /// script type a save changed the fields or variants of, with its
    /// schema in the running generation. A type this model doesn't have
    /// is left out: a save that removed it refuses elsewhere.
    pub(crate) fn migration<'model, 'old>(
        &'model self,
        changed: impl IntoIterator<Item = (&'old type_def::Identity, &'old schema::Fingerprint)>,
    ) -> Migration<'model> {
        let plans = changed
            .into_iter()
            .filter_map(|(identity, old)| {
                let definition = self.definitions.get(identity)?;
                let type_def::Identity::Script(key) = identity else {
                    return None;
                };
                let path = key
                    .modules()
                    .iter()
                    .map(ImmutableString::as_str)
                    .chain([key.name()])
                    .collect::<Vec<_>>()
                    .join("::");
                let layout = layout(old, &definition.schema, &definition.shape);
                Some((
                    identity.clone(),
                    Plan {
                        path,
                        old: old.clone(),
                        new: definition.schema.clone(),
                        layout,
                    },
                ))
            })
            .collect();
        Migration { model: self, plans }
    }
}

/// How the values of a type with the schema `old` become ones of `new`,
/// whose shape is `shape`.
fn layout(old: &schema::Fingerprint, new: &schema::Fingerprint, shape: &Shape) -> Layout {
    let before = old.members();
    let after = new.members();
    match shape {
        Shape::Struct(types) => {
            let mut reordered = false;
            let mut last = None;
            let fields = after
                .iter()
                .zip(types.iter())
                .map(|(field, ty)| {
                    let kept = before.iter().position(|old| old.name() == field.name());
                    match kept {
                        Some(index) if before[index].same_type(field) => {
                            reordered |= last.is_some_and(|last| index < last);
                            last = Some(index);
                            Source::Kept(index)
                        }
                        kept => Source::Made {
                            name: field.name().into(),
                            ty: Box::new(ty.clone()),
                            changed: kept.is_some(),
                        },
                    }
                })
                .collect::<Box<[_]>>();
            let dropped = before
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    !fields
                        .iter()
                        .any(|source| matches!(source, Source::Kept(kept) if kept == index))
                })
                .map(|(index, field)| Dropped {
                    index,
                    name: field.name().into(),
                    plain: field.plain(),
                })
                .collect();
            Layout::Struct {
                fields,
                dropped,
                reordered,
            }
        }
        Shape::Enum(_) => {
            let mut kept = HashSet::new();
            let mut changed = HashSet::new();
            for variant in &before {
                match after.iter().find(|new| new.name() == variant.name()) {
                    Some(new) if new.same_type(variant) => kept.insert(variant.name().into()),
                    Some(_) => changed.insert(variant.name().into()),
                    None => false,
                };
            }
            Layout::Enum { kept, changed }
        }
    }
}

impl Migration<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }

    /// Find every live value of a changed type `roots` reach, and check
    /// each can take its type's new shape: every one the census counts,
    /// and none in a variant the save removed or changed. `drops`, whether
    /// the running program has a `Drop`, refuses a field dropped whose
    /// type may run it.
    pub(crate) fn survey(&self, roots: &mut [Dynamic], drops: bool) -> Result<Survey, Refusal> {
        let mut found = HashMap::new();
        let mut old = HashMap::new();
        let mut failure = None;
        let mut seen = HashSet::new();
        for root in roots.iter_mut() {
            visit(root, &mut seen, &mut |value| {
                let Some((identity, schema, variant)) = nominal_of(value) else {
                    return;
                };
                let Some(plan) = self.plans.get(identity) else {
                    return;
                };
                *found.entry(identity.clone()).or_insert(0) += 1;
                if schema.same_representation(&plan.new) {
                    return;
                }
                *old.entry(identity.clone()).or_insert(0) += 1;
                if failure.is_none() {
                    failure = plan.refusal(schema, variant);
                }
            });
        }
        if let Some(refusal) = failure {
            return Err(refusal);
        }
        for (identity, plan) in &self.plans {
            let live = nominal::census::count(identity);
            if live == 0 {
                continue;
            }
            if found.get(identity).copied().unwrap_or(0) != live {
                return Err(Refusal::Unreached {
                    path: plan.path.clone(),
                });
            }
            if let Layout::Struct { dropped, .. } = &plan.layout
                && let Some(field) = dropped.iter().find(|field| drops && !field.plain)
            {
                return Err(Refusal::Dropping {
                    path: plan.path.clone(),
                    field: field.name.clone(),
                });
            }
        }
        Ok(Survey { old })
    }

    /// The `Default` of each field the new types make, once for each value
    /// `survey` found: `made` gives a script type's own, and this model
    /// the rest.
    pub(crate) fn defaults(
        &self,
        survey: &Survey,
        made: &dyn Fn(&type_def::Identity) -> Option<Result<Dynamic, String>>,
    ) -> Result<Defaults, Refusal> {
        let mut defaults = HashMap::new();
        for (identity, plan) in &self.plans {
            let Layout::Struct { fields, .. } = &plan.layout else {
                continue;
            };
            if !fields
                .iter()
                .any(|field| matches!(field, Source::Made { .. }))
            {
                continue;
            }
            let count = survey.old.get(identity).copied().unwrap_or(0);
            let values = (0..count)
                .map(|_| {
                    fields
                        .iter()
                        .map(|field| match field {
                            Source::Kept(_) => Ok(None),
                            Source::Made { name, ty, changed } => self
                                .default_of(ty, made)
                                .map(Some)
                                .map_err(|_| Refusal::Undefaulted {
                                    path: plan.path.clone(),
                                    field: name.clone(),
                                    changed: *changed,
                                }),
                        })
                        .collect::<Result<Box<[_]>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()?;
            defaults.insert(identity.clone(), values);
        }
        Ok(Defaults { made: defaults })
    }

    /// The `Default` of `ty`: a script type's own, which `made` gives, or
    /// the one this model makes.
    fn default_of(
        &self,
        ty: &type_ref::Type,
        made: &dyn Fn(&type_def::Identity) -> Option<Result<Dynamic, String>>,
    ) -> Result<Dynamic, String> {
        if let type_ref::Type::Path(path) = ty
            && let type_ref::Target::Script { identity, .. } = &path.target
            && let Some(made) = made(identity)
        {
            return made;
        }
        self.model
            .default_for_type(ty, &mut std::collections::BTreeSet::new())
            .map_err(|error| error.to_string())
    }

    /// Rewrite each live value of a changed type `roots` reach in place, a
    /// value inside another first, with `defaults` in its new fields.
    pub(crate) fn rewrite(
        &self,
        roots: &mut [Dynamic],
        mut defaults: Defaults,
    ) -> Result<Rewritten, Refusal> {
        let mut dropped = Vec::new();
        let mut failure = None;
        let mut seen = HashSet::new();
        for root in roots.iter_mut() {
            visit(root, &mut seen, &mut |value| {
                if failure.is_none()
                    && let Err(refusal) = self.migrate(value, &mut defaults, &mut dropped)
                {
                    failure = Some(refusal);
                }
            });
        }
        if let Some(refusal) = failure {
            return Err(refusal);
        }
        let mut paths = self.plans.values().collect::<Vec<_>>();
        paths.sort_by(|a, b| a.path.cmp(&b.path));
        let changes = paths
            .iter()
            .filter(|plan| !plan.old.same_representation(&plan.new))
            .map(|plan| plan.change())
            .collect::<Vec<_>>();
        Ok(Rewritten {
            report: format!("{}; state kept", changes.join(", ")),
            dropped,
        })
    }

    /// `value` as its type's new shape, when it is a value of a changed
    /// type in its old one; the fields it drops go to `dropped`.
    fn migrate(
        &self,
        value: &mut Dynamic,
        defaults: &mut Defaults,
        dropped: &mut Vec<Dynamic>,
    ) -> Result<(), Refusal> {
        let Some((identity, schema, _)) = nominal_of(value) else {
            return Ok(());
        };
        let Some(plan) = self.plans.get(identity) else {
            return Ok(());
        };
        if schema.same_representation(&plan.new) {
            return Ok(());
        }
        let identity = identity.clone();
        let failed = |error: crate::program::construction::Error| Refusal::Construction {
            path: plan.path.clone(),
            error: error.to_string(),
        };
        let made = match (mem_take(value), &plan.layout) {
            (
                Union::Struct(old, ..),
                Layout::Struct {
                    fields,
                    dropped: gone,
                    ..
                },
            ) => {
                let mut old = (*old).into_fields().into_vec();
                let mut made = defaults
                    .made
                    .get_mut(&identity)
                    .and_then(Vec::pop)
                    .map(Vec::from)
                    .unwrap_or_default();
                let members = fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| match field {
                        Source::Kept(at) => old.get_mut(*at).map(Dynamic::take).unwrap_or_default(),
                        Source::Made { .. } => made
                            .get_mut(index)
                            .and_then(Option::take)
                            .unwrap_or_default(),
                    })
                    .collect::<Box<[_]>>();
                dropped.extend(
                    gone.iter()
                        .filter_map(|field| old.get_mut(field.index).map(Dynamic::take)),
                );
                let proof = self
                    .model
                    .prove_struct(&identity, &plan.new, members)
                    .map_err(failed)?;
                Dynamic::from(
                    nominal::Struct::construct(self.model, &identity, &plan.new, proof)
                        .map_err(failed)?,
                )
            }
            (Union::Enum(old, ..), Layout::Enum { .. }) => {
                let (variant, payload) = (*old).into_variant();
                let proof = self
                    .model
                    .prove_enum(&identity, &plan.new, &variant, payload)
                    .map_err(failed)?;
                Dynamic::from(
                    nominal::Enum::construct(self.model, &identity, &plan.new, &variant, proof)
                        .map_err(failed)?,
                )
            }
            (other, _) => {
                *value = Dynamic(other);
                return Ok(());
            }
        };
        *value = made;
        Ok(())
    }
}

impl Plan {
    /// Why a value with `schema`, in `variant` when it is an enum's, can't
    /// take this plan's shape.
    fn refusal(
        &self,
        schema: &schema::Fingerprint,
        variant: Option<&nominal::variant::Id>,
    ) -> Option<Refusal> {
        if !schema.same_representation(&self.old) {
            return Some(Refusal::Unreached {
                path: self.path.clone(),
            });
        }
        let (Layout::Enum { kept, changed }, Some(variant)) = (&self.layout, variant) else {
            return None;
        };
        let name = variant.name();
        if kept.contains(name) {
            return None;
        }
        let path = self.path.clone();
        let variant = name.into();
        Some(if changed.contains(name) {
            Refusal::ChangedVariant { path, variant }
        } else {
            Refusal::RemovedVariant { path, variant }
        })
    }

    /// What the save did to this type, as the report says it.
    fn change(&self) -> String {
        let path = &self.path;
        let Layout::Struct {
            fields,
            dropped,
            reordered,
        } = &self.layout
        else {
            return format!("`{path}` changed its variants");
        };
        let mut parts = Vec::new();
        let gained = fields
            .iter()
            .filter_map(|field| match field {
                Source::Made {
                    name,
                    changed: false,
                    ..
                } => Some(format!("`{name}`")),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !gained.is_empty() {
            parts.push(format!("gained {} (default)", gained.join(", ")));
        }
        let retyped = fields
            .iter()
            .filter_map(|field| match field {
                Source::Made {
                    name,
                    changed: true,
                    ..
                } => Some(format!("`{name}`")),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !retyped.is_empty() {
            parts.push(format!(
                "changed the type of {} (default)",
                retyped.join(", ")
            ));
        }
        if !dropped.is_empty() {
            let names = dropped
                .iter()
                .map(|field| format!("`{}`", field.name))
                .collect::<Vec<_>>();
            parts.push(format!("dropped {}", names.join(", ")));
        }
        if *reordered {
            parts.push("reordered its fields".to_owned());
        }
        format!("`{path}` {}", parts.join(", "))
    }
}

/// The type, schema and, for an enum, variant of `value`, when it is a
/// value of a script struct or enum.
fn nominal_of(
    value: &Dynamic,
) -> Option<(
    &type_def::Identity,
    &schema::Fingerprint,
    Option<&nominal::variant::Id>,
)> {
    match &value.0 {
        Union::Struct(value, ..) => Some((value.identity(), value.schema(), None)),
        Union::Enum(value, ..) => Some((value.identity(), value.schema(), Some(value.variant()))),
        _ => None,
    }
}

/// `value`'s representation, `()` left in its place.
fn mem_take(value: &mut Dynamic) -> Union {
    std::mem::take(value).0
}

/// Call `each` on `value` and every value inside it, the inner ones first:
/// a struct's fields, an enum's payload, an array's or a map's members,
/// an `Option`'s or a `Result`'s value, and a shared cell's value once
/// however many handles reach it. A cell borrowed elsewhere is passed
/// over, and so is a native value, whose insides the walk can't see: the
/// census count then says a value was missed.
fn visit(value: &mut Dynamic, seen: &mut HashSet<usize>, each: &mut dyn FnMut(&mut Dynamic)) {
    match &mut value.0 {
        Union::Shared(cell, ..) => {
            if !seen.insert(crate::Shared::as_ptr(cell).cast::<()>() as usize) {
                return;
            }
            let cell = cell.clone();
            if let Some(mut inner) = crate::func::locked_write(&cell) {
                visit(&mut inner, seen, each);
            }
            return;
        }
        Union::Struct(structure, ..) => {
            for field in structure.fields_mut() {
                visit(field, seen, each);
            }
        }
        Union::Enum(enumeration, ..) => {
            for field in enumeration.payload_mut() {
                visit(field, seen, each);
            }
        }
        Union::Array(array, ..) => {
            for member in array.iter_mut() {
                visit(member, seen, each);
            }
        }
        Union::Map(map, ..) => {
            for member in map.values_mut() {
                visit(member, seen, each);
            }
        }
        Union::Variant(..) => {
            if let Some(Some(inner)) = value.downcast_mut::<crate::option::Value>() {
                visit(inner, seen, each);
            } else if let Some(Ok(inner) | Err(inner)) =
                value.downcast_mut::<crate::result::Value>()
            {
                visit(inner, seen, each);
            }
            return;
        }
        _ => return,
    }
    each(value);
}
