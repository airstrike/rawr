//! Stable structural types retained by validated application contracts.

use crate::resolve;
use crate::types::nominal;
use crate::{capability, host, program, type_def, ImmutableString, Span};
use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// One stable structural type proof in a validated application contract.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Type(Shape);

#[derive(Clone)]
pub(crate) struct RuntimeGraph {
    tuple_free: BTreeMap<type_def::Id, bool>,
    dependencies: BTreeMap<type_def::Id, Box<[host::facade::Dependency]>>,
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Shape {
    Unit,
    Tuple(Box<[Type]>),
    Builtin(ImmutableString),
    Host {
        contract: host::type_def::Contract,
        arguments: Box<[Type]>,
        projections: Box<[host::type_def::Projection]>,
        owner: Option<host::facade::Dependency>,
    },
    Script {
        identity: type_def::Identity,
        schema: program::schema::Fingerprint,
        kind: type_def::Kind,
        runtime: host::runtime::type_ref::Representation,
        capabilities: capability::Set,
        tuple_free: bool,
        dependencies: Box<[host::facade::Dependency]>,
    },
    Reference {
        mutable: bool,
        target: Box<Type>,
    },
}

impl fmt::Debug for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Type")
            .field(&self.display_name())
            .finish()
    }
}

impl Type {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        fn types(values: &mut [Type], profile: &host::facade::ProfileFinalization<'_>) -> usize {
            values.iter_mut().fold(0_usize, |completed, value| {
                completed.saturating_add(value.normalize_profile(profile))
            })
        }

        match &mut self.0 {
            Shape::Unit | Shape::Builtin(_) => 0,
            Shape::Tuple(values) => types(values, profile),
            Shape::Host {
                contract,
                arguments,
                projections,
                owner,
            } => {
                let mut completed = contract.normalize_profile(profile);
                completed = completed.saturating_add(types(arguments, profile));
                for projection in projections {
                    completed = completed.saturating_add(projection.normalize_profile(profile));
                }
                if let Some(owner) = owner {
                    completed = completed.saturating_add(profile.normalize_dependency(owner).1);
                }
                completed
            }
            Shape::Script {
                schema,
                kind,
                runtime,
                dependencies,
                ..
            } => {
                let completed = dependencies.iter_mut().fold(
                    schema.normalize_profile(profile),
                    |completed, dependency| {
                        completed.saturating_add(profile.normalize_dependency(dependency).1)
                    },
                );
                if runtime.project_script(*kind) {
                    completed.saturating_add(1)
                } else {
                    profile.reject_runtime_representation();
                    completed
                }
            }
            Shape::Reference { target, .. } => target.normalize_profile(profile),
        }
    }

    #[cfg(test)]
    pub(crate) const fn digest_unit() -> Self {
        Self(Shape::Unit)
    }

    pub(crate) fn from_definition(
        definition: &crate::type_def::Definition,
        runtime: &RuntimeGraph,
    ) -> Self {
        Self(Shape::Script {
            identity: type_def::Identity::Script(definition.key().clone()),
            schema: definition.schema().clone(),
            kind: definition.kind(),
            runtime: host::runtime::type_ref::Representation::exact(nominal_runtime(definition)),
            capabilities: definition.capabilities(),
            tuple_free: runtime.is_tuple_free(definition.id()),
            dependencies: runtime.dependencies(definition.id()).into(),
        })
    }

    pub(crate) const fn unit() -> Self {
        Self(Shape::Unit)
    }

    pub(crate) fn builtin(name: impl Into<ImmutableString>) -> Self {
        Self(Shape::Builtin(name.into()))
    }

    pub(crate) fn host(
        definition: &host::type_def::Definition,
        arguments: Box<[Self]>,
        owner: Option<host::facade::Dependency>,
    ) -> Self {
        Self(Shape::Host {
            contract: definition.contract(),
            arguments,
            projections: definition.projections().into(),
            owner,
        })
    }

    pub(crate) fn from_resolved(
        ty: &resolve::type_ref::Type,
        definitions: &BTreeMap<type_def::Id, &crate::type_def::Definition>,
        runtime: &RuntimeGraph,
    ) -> Self {
        match ty {
            resolve::type_ref::Type::Unit(_) => Self(Shape::Unit),
            resolve::type_ref::Type::Tuple { members, .. } => Self(Shape::Tuple(
                members
                    .iter()
                    .map(|member| Self::from_resolved(member, definitions, runtime))
                    .collect(),
            )),
            resolve::type_ref::Type::Path(path) => {
                let arguments = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        resolve::type_ref::Argument::Type(ty) => {
                            Some(Self::from_resolved(ty, definitions, runtime))
                        }
                        resolve::type_ref::Argument::Lifetime(_) => None,
                    })
                    .collect();
                match &path.target {
                    resolve::type_ref::Target::Builtin
                    | resolve::type_ref::Target::Parameter
                    | resolve::type_ref::Target::Projection(..) => {
                        Self(Shape::Builtin(path.name.clone()))
                    }
                    resolve::type_ref::Target::Host { definition, owner } => Self(Shape::Host {
                        contract: definition.contract(),
                        arguments,
                        projections: definition.projections().into(),
                        owner: owner.clone(),
                    }),
                    resolve::type_ref::Target::Script {
                        definition,
                        identity,
                    } => {
                        let definition = definitions
                            .get(definition)
                            .expect("resolved script types retain one stable definition");
                        Self(Shape::Script {
                            identity: identity.clone(),
                            schema: definition.schema().clone(),
                            kind: definition.kind(),
                            runtime: host::runtime::type_ref::Representation::exact(
                                nominal_runtime(definition),
                            ),
                            capabilities: definition.capabilities(),
                            tuple_free: runtime.is_tuple_free(definition.id()),
                            dependencies: runtime.dependencies(definition.id()).into(),
                        })
                    }
                }
            }
            resolve::type_ref::Type::Reference {
                mutable, target, ..
            } => Self(Shape::Reference {
                mutable: *mutable,
                target: Box::new(Self::from_resolved(target, definitions, runtime)),
            }),
        }
    }

    /// Return the stable nominal identity, when this is a nominal type.
    #[must_use]
    pub const fn identity(&self) -> Option<&type_def::Identity> {
        match &self.0 {
            Shape::Script { identity, .. } => Some(identity),
            Shape::Unit
            | Shape::Tuple(_)
            | Shape::Builtin(_)
            | Shape::Host { .. }
            | Shape::Reference { .. } => None,
        }
    }

    /// Return the exact script schema proof, when this is a script nominal type.
    #[must_use]
    pub const fn schema(&self) -> Option<&program::schema::Fingerprint> {
        match &self.0 {
            Shape::Script { schema, .. } => Some(schema),
            Shape::Unit
            | Shape::Tuple(_)
            | Shape::Builtin(_)
            | Shape::Host { .. }
            | Shape::Reference { .. } => None,
        }
    }

    /// Return whether this type carries the requested compiler capability proof.
    #[must_use]
    pub fn provides(&self, capability: capability::Kind) -> bool {
        match &self.0 {
            Shape::Unit => true,
            Shape::Tuple(members) => members.iter().all(|member| member.provides(capability)),
            Shape::Builtin(name) => builtin_provides(name, capability),
            Shape::Host { contract, .. } => contract.capabilities().contains(capability),
            Shape::Script { capabilities, .. } => capabilities.contains(capability),
            Shape::Reference { .. } => false,
        }
    }

    /// Return a stable human-facing structural type name.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.0 {
            Shape::Unit => "()".to_string(),
            Shape::Tuple(members) => format!(
                "({})",
                members
                    .iter()
                    .map(Self::display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Shape::Builtin(name) => name.to_string(),
            Shape::Host {
                contract,
                arguments,
                ..
            } => applied_name(&contract.path(), arguments),
            Shape::Script { identity, .. } => match identity {
                type_def::Identity::Script(key) => key.to_string(),
                type_def::Identity::Host(_) => "<host>".to_string(),
            },
            Shape::Reference { mutable, target } => format!(
                "&{}{}",
                if *mutable { "mut " } else { "" },
                target.display_name()
            ),
        }
    }

    pub(crate) const fn is_unit(&self) -> bool {
        matches!(self.0, Shape::Unit)
    }

    pub(crate) const fn reference(&self) -> Option<(bool, &Self)> {
        match &self.0 {
            Shape::Reference { mutable, target } => Some((*mutable, target)),
            Shape::Unit
            | Shape::Tuple(_)
            | Shape::Builtin(_)
            | Shape::Host { .. }
            | Shape::Script { .. } => None,
        }
    }

    pub(crate) fn host_arguments(&self, target: &host::type_def::Contract) -> Option<&[Self]> {
        match &self.0 {
            Shape::Host {
                contract,
                arguments,
                ..
            } if contract == target => Some(arguments),
            Shape::Unit
            | Shape::Tuple(_)
            | Shape::Builtin(_)
            | Shape::Host { .. }
            | Shape::Script { .. }
            | Shape::Reference { .. } => None,
        }
    }

    pub(crate) fn is_tuple_free(&self) -> bool {
        match &self.0 {
            Shape::Unit | Shape::Builtin(_) => true,
            Shape::Tuple(_) => false,
            Shape::Host { arguments, .. } => arguments.iter().all(Self::is_tuple_free),
            Shape::Script { tuple_free, .. } => *tuple_free,
            Shape::Reference { target, .. } => target.is_tuple_free(),
        }
    }

    pub(crate) fn tuple(members: &[Self]) -> Self {
        Self(Shape::Tuple(members.into()))
    }

    /// This type without references, `&str` read as `String`: how a preview
    /// holds `bind` arguments, which are values at runtime.
    pub(crate) fn dereferenced(&self) -> Self {
        match &self.0 {
            Shape::Reference { target, .. } => target.dereferenced(),
            Shape::Builtin(name) if name.as_str() == "str" => Self(Shape::Builtin("String".into())),
            Shape::Tuple(members) => Self(Shape::Tuple(
                members.iter().map(Self::dereferenced).collect(),
            )),
            Shape::Unit | Shape::Builtin(_) | Shape::Host { .. } | Shape::Script { .. } => {
                self.clone()
            }
        }
    }

    pub(crate) fn tuple_members(&self) -> Option<&[Self]> {
        match &self.0 {
            Shape::Tuple(members) => Some(members),
            Shape::Unit
            | Shape::Builtin(_)
            | Shape::Host { .. }
            | Shape::Script { .. }
            | Shape::Reference { .. } => None,
        }
    }

    pub(crate) fn matches_host(
        &self,
        target: &host::type_def::Contract,
        arguments: &[Self],
    ) -> bool {
        matches!(
            &self.0,
            Shape::Host {
                contract,
                arguments: actual,
                ..
            } if contract == target && actual.as_ref() == arguments
        )
    }

    pub(crate) fn project_to(
        &self,
        target: &host::type_def::Contract,
        expected: Option<&[Self]>,
    ) -> Option<(host::type_def::Projection, Box<[Self]>)> {
        let Shape::Host {
            contract,
            arguments,
            projections,
            ..
        } = &self.0
        else {
            return None;
        };
        let projection = projections
            .iter()
            .find(|projection| projection.target_contract() == target)?;
        let projected = projection
            .arguments()
            .iter()
            .enumerate()
            .map(|(target_index, argument)| {
                match argument {
                    host::type_def::Argument::Source(parameter) => contract
                        .parameters()
                        .iter()
                        .position(|candidate| candidate == parameter)
                        .and_then(|index| arguments.get(index)),
                    host::type_def::Argument::Fresh => {
                        expected.and_then(|arguments| arguments.get(target_index))
                    }
                }
                .cloned()
            })
            .collect::<Option<Box<[_]>>>()?;
        if expected.is_some_and(|expected| expected != projected.as_ref()) {
            return None;
        }
        Some((projection.clone(), projected))
    }

    pub(crate) fn projection_site(
        &self,
        target: &host::type_def::Contract,
        expected: Option<&[Self]>,
        span: Span,
    ) -> Option<(host::projection::Site, Box<[Self]>)> {
        let Shape::Host {
            contract,
            owner: Some(owner),
            ..
        } = &self.0
        else {
            return None;
        };
        let (projection, arguments) = self.project_to(target, expected)?;
        let source = self.exact_runtime_type()?;
        let target_owner = projection
            .target_dependency()
            .unwrap_or_else(|| owner.clone());
        let target_type = host::runtime::type_ref::Type::host(
            target.path().into(),
            target.runtime_identity()?,
            arguments
                .iter()
                .map(Self::exact_runtime_type)
                .collect::<Option<Vec<_>>>()?
                .into_boxed_slice(),
            Some(target_owner),
        );
        let key = host::projection::Key::new(owner.clone(), contract.clone(), projection);
        Some((
            host::projection::Site::new(key, source, target_type, span),
            arguments,
        ))
    }

    pub(crate) fn exact_runtime_type(&self) -> Option<host::runtime::type_ref::Type> {
        match &self.0 {
            Shape::Unit => Some(host::runtime::type_ref::Type::unit()),
            Shape::Builtin(name) => builtin_runtime(name)
                .map(|(name, runtime)| host::runtime::type_ref::Type::builtin(name, runtime)),
            Shape::Host {
                contract,
                arguments,
                owner,
                ..
            } => Some(host::runtime::type_ref::Type::host(
                contract.path().into(),
                contract.runtime_identity()?,
                arguments
                    .iter()
                    .map(Self::exact_runtime_type)
                    .collect::<Option<Vec<_>>>()?
                    .into_boxed_slice(),
                owner.clone(),
            )),
            Shape::Script {
                identity,
                schema,
                kind,
                runtime,
                ..
            } => Some(host::runtime::type_ref::Type::script(
                identity.clone(),
                schema.clone(),
                *kind,
                runtime.runtime(),
            )),
            Shape::Tuple(members) => Some(host::runtime::type_ref::Type::Tuple(
                members
                    .iter()
                    .map(Self::exact_runtime_type)
                    .collect::<Option<_>>()?,
            )),
            Shape::Reference { .. } => None,
        }
    }

    pub(crate) fn collect_dependencies(
        &self,
        dependencies: &mut BTreeSet<host::facade::Dependency>,
    ) {
        match &self.0 {
            Shape::Unit | Shape::Builtin(_) => {}
            Shape::Tuple(members) => {
                for member in members {
                    member.collect_dependencies(dependencies);
                }
            }
            Shape::Host {
                arguments, owner, ..
            } => {
                dependencies.extend(owner.iter().cloned());
                for argument in arguments {
                    argument.collect_dependencies(dependencies);
                }
            }
            Shape::Script {
                dependencies: retained,
                ..
            } => dependencies.extend(retained.iter().cloned()),
            Shape::Reference { target, .. } => target.collect_dependencies(dependencies),
        }
    }

    pub(crate) fn collect_schema_roots(&self, roots: &mut BTreeSet<type_def::Identity>) {
        match &self.0 {
            Shape::Unit | Shape::Builtin(_) => {}
            Shape::Tuple(members) => {
                for member in members {
                    member.collect_schema_roots(roots);
                }
            }
            Shape::Host { arguments, .. } => {
                for argument in arguments {
                    argument.collect_schema_roots(roots);
                }
            }
            Shape::Script { identity, .. } => {
                roots.insert(identity.clone());
            }
            Shape::Reference { target, .. } => target.collect_schema_roots(roots),
        }
    }

    pub(crate) fn retained_root_is_opaque(&self) -> bool {
        match &self.0 {
            Shape::Unit | Shape::Builtin(_) | Shape::Script { .. } => false,
            Shape::Tuple(members) => members.iter().any(Self::retained_root_is_opaque),
            Shape::Host { .. } => true,
            Shape::Reference { target, .. } => target.retained_root_is_opaque(),
        }
    }
}

impl crate::preview::digest::Write for Type {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::{write_option, write_script_identity, write_slice};

        match &self.0 {
            Shape::Unit => writer.tag(0x2001),
            Shape::Tuple(members) => {
                writer.tag(0x2002);
                write_slice(writer, members)?;
            }
            Shape::Builtin(name) => {
                writer.tag(0x2003);
                writer.text(name);
            }
            Shape::Host {
                contract,
                arguments,
                projections,
                owner,
            } => {
                writer.tag(0x2004);
                writer.tag(0x2011);
                contract.write(writer)?;
                writer.tag(0x2012);
                write_slice(writer, arguments)?;
                writer.tag(0x2013);
                write_slice(writer, projections)?;
                writer.tag(0x2014);
                write_option(writer, owner.as_ref())?;
            }
            Shape::Script {
                identity,
                schema,
                kind: _,
                runtime: _,
                capabilities,
                tuple_free,
                dependencies,
            } => {
                writer.tag(0x2005);
                writer.tag(0x2021);
                write_script_identity(writer, identity, "preview script type identity")?;
                writer.tag(0x2022);
                schema.write(writer)?;
                writer.tag(0x2023);
                capabilities.write(writer)?;
                writer.tag(0x2024);
                writer.bool(*tuple_free);
                writer.tag(0x2025);
                write_slice(writer, dependencies)?;
            }
            Shape::Reference { mutable, target } => {
                writer.tag(0x2006);
                writer.tag(0x2031);
                writer.bool(*mutable);
                writer.tag(0x2032);
                target.write(writer)?;
            }
        }
        Ok(())
    }
}

fn nominal_runtime(definition: &crate::type_def::Definition) -> TypeId {
    match definition.kind() {
        crate::type_def::Kind::Struct => TypeId::of::<nominal::Struct>(),
        crate::type_def::Kind::Enum => TypeId::of::<nominal::Enum>(),
    }
}

fn builtin_runtime(name: &str) -> Option<(&'static str, TypeId)> {
    match crate::host::runtime_builtin(name) {
        "bool" => Some(("bool", TypeId::of::<bool>())),
        "char" => Some(("char", TypeId::of::<char>())),
        "i64" => Some(("i64", TypeId::of::<crate::INT>())),
        #[cfg(not(feature = "no_float"))]
        "f64" => Some(("f64", TypeId::of::<crate::FLOAT>())),
        #[cfg(feature = "no_float")]
        "f64" => Some(("f64", TypeId::of::<crate::Dynamic>())),
        "String" => Some(("String", TypeId::of::<ImmutableString>())),
        _ => None,
    }
}

impl RuntimeGraph {
    pub(crate) fn new(program: &crate::resolve::Program) -> Self {
        let mut tuple_free = program
            .types()
            .iter()
            .map(|definition| (definition.id(), true))
            .collect::<BTreeMap<_, _>>();

        loop {
            let rejected = program
                .items()
                .iter()
                .filter_map(|shell| match &shell.kind {
                    crate::resolve::item::Kind::Struct(item) => Some((
                        item.definition,
                        item.fields
                            .iter()
                            .all(|field| resolved_type_is_tuple_free(&field.ty, &tuple_free)),
                    )),
                    crate::resolve::item::Kind::Enum(item) => Some((
                        item.definition,
                        item.variants.iter().all(|variant| {
                            variant
                                .payload
                                .iter()
                                .all(|payload| resolved_type_is_tuple_free(payload, &tuple_free))
                        }),
                    )),
                    crate::resolve::item::Kind::Use(_)
                    | crate::resolve::item::Kind::Impl(_)
                    | crate::resolve::item::Kind::Trait(_)
                    | crate::resolve::item::Kind::Function(_)
                    | crate::resolve::item::Kind::LegacyFunction(_) => None,
                })
                .filter_map(|(definition, is_tuple_free)| {
                    (!is_tuple_free && tuple_free[&definition]).then_some(definition)
                })
                .collect::<Vec<_>>();
            if rejected.is_empty() {
                break;
            }
            for definition in rejected {
                tuple_free.insert(definition, false);
            }
        }

        let mut dependencies = program
            .types()
            .iter()
            .map(|definition| (definition.id(), BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        loop {
            let mut changed = false;
            for shell in program.items() {
                let (definition, discovered) = match &shell.kind {
                    crate::resolve::item::Kind::Struct(item) => {
                        let mut discovered = dependencies[&item.definition].clone();
                        for field in &item.fields {
                            collect_resolved_dependencies(
                                &field.ty,
                                &dependencies,
                                &mut discovered,
                            );
                        }
                        (item.definition, discovered)
                    }
                    crate::resolve::item::Kind::Enum(item) => {
                        let mut discovered = dependencies[&item.definition].clone();
                        for payload in item
                            .variants
                            .iter()
                            .flat_map(|variant| variant.payload.iter())
                        {
                            collect_resolved_dependencies(payload, &dependencies, &mut discovered);
                        }
                        (item.definition, discovered)
                    }
                    crate::resolve::item::Kind::Use(_)
                    | crate::resolve::item::Kind::Impl(_)
                    | crate::resolve::item::Kind::Trait(_)
                    | crate::resolve::item::Kind::Function(_)
                    | crate::resolve::item::Kind::LegacyFunction(_) => continue,
                };
                if discovered != dependencies[&definition] {
                    dependencies.insert(definition, discovered);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        Self {
            tuple_free,
            dependencies: dependencies
                .into_iter()
                .map(|(id, dependencies)| (id, dependencies.into_iter().collect()))
                .collect(),
        }
    }

    fn is_tuple_free(&self, definition: type_def::Id) -> bool {
        self.tuple_free[&definition]
    }

    fn dependencies(&self, definition: type_def::Id) -> &[host::facade::Dependency] {
        &self.dependencies[&definition]
    }
}

fn collect_resolved_dependencies(
    ty: &resolve::type_ref::Type,
    scripts: &BTreeMap<type_def::Id, BTreeSet<host::facade::Dependency>>,
    dependencies: &mut BTreeSet<host::facade::Dependency>,
) {
    match ty {
        resolve::type_ref::Type::Unit(_) => {}
        resolve::type_ref::Type::Tuple { members, .. } => {
            for member in members {
                collect_resolved_dependencies(member, scripts, dependencies);
            }
        }
        resolve::type_ref::Type::Reference { target, .. } => {
            collect_resolved_dependencies(target, scripts, dependencies);
        }
        resolve::type_ref::Type::Path(path) => {
            match &path.target {
                resolve::type_ref::Target::Builtin
                | resolve::type_ref::Target::Parameter
                | resolve::type_ref::Target::Projection(..) => {}
                resolve::type_ref::Target::Host { owner, .. } => {
                    dependencies.extend(owner.iter().cloned());
                }
                resolve::type_ref::Target::Script { definition, .. } => {
                    dependencies.extend(scripts[definition].iter().cloned());
                }
            }
            for argument in &path.arguments {
                if let resolve::type_ref::Argument::Type(argument) = argument {
                    collect_resolved_dependencies(argument, scripts, dependencies);
                }
            }
        }
    }
}

fn resolved_type_is_tuple_free(
    ty: &resolve::type_ref::Type,
    definitions: &BTreeMap<type_def::Id, bool>,
) -> bool {
    match ty {
        resolve::type_ref::Type::Unit(_) => true,
        resolve::type_ref::Type::Tuple { .. } => false,
        resolve::type_ref::Type::Path(path) => {
            let target = match &path.target {
                resolve::type_ref::Target::Builtin
                | resolve::type_ref::Target::Host { .. }
                | resolve::type_ref::Target::Parameter
                | resolve::type_ref::Target::Projection(..) => true,
                resolve::type_ref::Target::Script { definition, .. } => definitions[definition],
            };
            target
                && path.arguments.iter().all(|argument| match argument {
                    resolve::type_ref::Argument::Type(ty) => {
                        resolved_type_is_tuple_free(ty, definitions)
                    }
                    resolve::type_ref::Argument::Lifetime(_) => true,
                })
        }
        resolve::type_ref::Type::Reference { target, .. } => {
            resolved_type_is_tuple_free(target, definitions)
        }
    }
}

fn applied_name(path: &str, arguments: &[Type]) -> String {
    if arguments.is_empty() {
        path.to_string()
    } else {
        format!(
            "{path}<{}>",
            arguments
                .iter()
                .map(Type::display_name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn builtin_provides(name: &str, capability: capability::Kind) -> bool {
    match crate::host::runtime_builtin(name) {
        "String" => capability != capability::Kind::Copy,
        "f64" => capability != capability::Kind::Eq,
        "bool" | "char" | "i64" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Type;
    use crate::{host, Span};

    struct Element;
    struct Text;

    #[test]
    fn fresh_projection_uses_each_application_context_exactly() {
        let mut builder =
            host::facade::Builder::new("ui", host::facade::Abi::new(1).unwrap()).unwrap();
        builder
            .type_::<Element>("Element", &["Message"], &[])
            .unwrap();
        builder.type_::<Text>("Text", &[], &[]).unwrap();
        builder
            .project_with("Text", "Element", &[host::type_def::Argument::fresh()])
            .unwrap();
        let facade = builder.finish().unwrap();
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let text = facade.types().find(|ty| ty.path() == "ui::Text").unwrap();
        let element = facade
            .types()
            .find(|ty| ty.path() == "ui::Element")
            .unwrap();
        let element = element.contract();
        let source = Type::host(text, Box::default(), Some(owner));
        let first = Type::builtin("String");
        let second = Type::builtin("i64");

        assert!(source.projection_site(&element, None, Span::NONE).is_none());
        let (first_site, first_arguments) = source
            .projection_site(&element, Some(std::slice::from_ref(&first)), Span::NONE)
            .unwrap();
        let (second_site, second_arguments) = source
            .projection_site(&element, Some(std::slice::from_ref(&second)), Span::NONE)
            .unwrap();

        assert_eq!(first_arguments.as_ref(), std::slice::from_ref(&first));
        assert_eq!(second_arguments.as_ref(), std::slice::from_ref(&second));
        assert_ne!(first_site.target(), second_site.target());
    }
}
