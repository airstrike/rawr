//! Generated-runner reconstruction of a program's generated bindings.

pub mod account;

use crate::{host, receiver, ImmutableString};
use rawr_import::surface::facade::{self, ItemKind};

/// One generated runtime's metadata binding scope in its engine's native account.
///
/// Authentication borrows the already-decoded carrier. Temporary adapters and
/// the copies retained by runtime bindings are charged separately; only the
/// latter contribution is transferred to the completed runtime registry.
#[doc(hidden)]
pub struct Runtime<'engine> {
    native: &'engine account::Native,
    structural: Option<Box<[host::runtime::StructuralAdapter]>>,
    events: Option<Box<[host::runtime::event::Adapter]>>,
    scratch: account::Attempt,
    allocation: account::Attempt,
}

impl crate::Engine {
    /// Begin binding one runtime against this engine's retained native metadata.
    #[doc(hidden)]
    pub fn begin_imported_runtime(&self) -> Result<Runtime<'_>, host::runtime::Error> {
        let native = self.imported_native.as_ref().ok_or_else(|| {
            host::runtime::Error::profile("generated runtime has no retained native owner")
        })?;
        let allocation = native
            .attempt()
            .map_err(|_| host::runtime::Error::ImportedAllocation)?;
        let mut scratch = native
            .attempt()
            .map_err(|_| host::runtime::Error::ImportedAllocation)?;
        use host::ImportAllocation as _;
        scratch
            .retain_checked(
                Some(1),
                std::mem::size_of::<Runtime<'_>>()
                    .checked_sub(2 * std::mem::size_of::<account::Attempt>()),
            )
            .map_err(|_| host::runtime::Error::ImportedAllocation)?;
        Ok(Runtime {
            native,
            structural: None,
            events: None,
            scratch,
            allocation,
        })
    }
}

impl Runtime<'_> {
    /// Authenticate temporary structural rows and reserve their runtime copies.
    pub fn authenticate_structural(
        &mut self,
        definitions: &[host::facade::Definition],
        rows: &[StructuralRow],
    ) -> Result<(), host::runtime::Error> {
        if self.structural.is_some() {
            return Err(host::runtime::Error::profile(
                "structural runtime rows already authenticated",
            ));
        }
        let before = self.scratch.usage();
        let adapters =
            authenticate_structural(definitions, self.native.carrier(), rows, &mut self.scratch)
                .map_err(runtime_error)?;
        self.retain_copies(before)?;
        self.structural = Some(adapters);
        Ok(())
    }

    /// Authenticate temporary replay rows and reserve their runtime copies.
    pub fn authenticate_events(
        &mut self,
        definitions: &[host::facade::Definition],
        rows: &[EventRow],
    ) -> Result<(), host::runtime::Error> {
        if self.events.is_some() {
            return Err(host::runtime::Error::profile(
                "event runtime rows already authenticated",
            ));
        }
        let before = self.scratch.usage();
        let adapters =
            authenticate_events(definitions, self.native.carrier(), rows, &mut self.scratch)
                .map_err(runtime_error)?;
        self.retain_copies(before)?;
        self.events = Some(adapters);
        Ok(())
    }

    /// Borrow this scope's authenticated structural adapters for runtime binding.
    #[must_use]
    pub fn structural(&self) -> &[host::runtime::StructuralAdapter] {
        self.structural.as_deref().unwrap_or_default()
    }

    /// Borrow this scope's authenticated event adapters for runtime binding.
    #[must_use]
    pub fn events(&self) -> &[host::runtime::event::Adapter] {
        self.events.as_deref().unwrap_or_default()
    }

    /// Retire authentication scratch and move native proof charges to the registry.
    pub fn finish(
        self,
        registry: host::runtime::Registry,
    ) -> Result<host::runtime::Registry, host::runtime::Error> {
        if self.structural.is_none() || self.events.is_none() {
            return Err(host::runtime::Error::profile(
                "generated runtime authentication is incomplete",
            ));
        }
        let Self {
            structural,
            events,
            scratch,
            allocation,
            ..
        } = self;
        // Retire the actual proof arrays before releasing their scratch charges.
        drop(structural);
        drop(events);
        drop(scratch);
        registry.retain_imported(allocation.finish())
    }

    fn retain_copies(&mut self, before: (usize, usize)) -> Result<(), host::runtime::Error> {
        let after = self.scratch.usage();
        use host::ImportAllocation as _;
        self.allocation
            .retain_checked(after.0.checked_sub(before.0), after.1.checked_sub(before.1))
            .map_err(|_| host::runtime::Error::ImportedAllocation)
    }
}

fn runtime_error(error: Error) -> host::runtime::Error {
    match error {
        Error::Allocation | Error::Surface(rawr_import::surface::Error::Allocation) => {
            host::runtime::Error::ImportedAllocation
        }
        error => host::runtime::Error::profile(error),
    }
}

/// Closed generated structural operation family carried by runner source.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum StructuralKind {
    Constructor,
    Field {
        copy: bool,
    },
    Variant,
    Match,
    /// A field set in place, which shares the field's selector.
    Store,
}

/// One scalar-only generated structural row awaiting manifest authentication.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StructuralRow {
    root: &'static str,
    /// The owning type's path below the root, `echo::Event`, which tells
    /// two types' variants or fields of one name apart.
    owner: &'static str,
    selector: [u8; 32],
    identity: [u8; 32],
    signature: [u8; 32],
    arguments: u32,
    kind: StructuralKind,
    instance: Option<[u8; 32]>,
}

/// One generated producer and its profile-validated private replay conversion.
#[doc(hidden)]
pub struct EventRow {
    root: &'static str,
    selector: [u8; 32],
    identity: [u8; 32],
    signature: [u8; 32],
    arguments: u32,
    source: [u8; 32],
    target_root: &'static str,
    target_path: &'static str,
    mapper: [u8; 16],
}

impl EventRow {
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        root: &'static str,
        selector: [u8; 32],
        identity: [u8; 32],
        signature: [u8; 32],
        arguments: u32,
        source: [u8; 32],
        target_root: &'static str,
        target_path: &'static str,
        mapper: [u8; 16],
    ) -> Self {
        Self {
            root,
            selector,
            identity,
            signature,
            arguments,
            source,
            target_root,
            target_path,
            mapper,
        }
    }
}

/// Join generated producer entries and exact installed contracts into private
/// replay bindings. Rows are emitted only after the profile/instance proof join.
#[doc(hidden)]
pub fn authenticate_generated_events(
    definitions: &[host::facade::Definition],
    manifest: &[u8],
    rows: &[EventRow],
) -> Result<Box<[host::runtime::event::Adapter]>, Error> {
    let carrier = rawr_import::runner::manifest::Carrier::transform(manifest)?;
    let mut allocation = account::Attempt::standard()?;
    authenticate_events(definitions, &carrier, rows, &mut allocation)
}

/// Authenticate replay bindings while their native owner retains the charges.
#[doc(hidden)]
pub fn authenticate_generated_events_retained(
    definitions: &[host::facade::Definition],
    rows: &[EventRow],
    native: &mut account::Native,
) -> Result<Box<[host::runtime::event::Adapter]>, Error> {
    let (carrier, mut allocation) = native.parts();
    authenticate_events(definitions, carrier, rows, &mut allocation)
}

fn authenticate_events(
    definitions: &[host::facade::Definition],
    carrier: &rawr_import::runner::manifest::Carrier,
    rows: &[EventRow],
    allocation: &mut account::Attempt,
) -> Result<Box<[host::runtime::event::Adapter]>, Error> {
    let mut adapters = Vec::new();
    use host::ImportAllocation as _;
    for row in rows {
        if row.mapper == [0; 16] || !carrier.contains(&row.identity, &row.signature, row.arguments)
        {
            return Err(Error::EventManifest);
        }
        let source = definitions
            .iter()
            .find(|definition| definition.root() == row.root)
            .ok_or(Error::EventFacade)?;
        let producer = source
            .items()
            .find(|item| {
                item.imported_identity()
                    .is_some_and(|identity| identity.fingerprint() == row.selector)
            })
            .ok_or(Error::EventSelector)?;
        let output_matches = match producer.signature().output().shape() {
            host::type_ref::Shape::Imported { identity, .. } => identity.as_bytes() == &row.source,
            host::type_ref::Shape::Registered { path, .. } => source.types().any(|ty| {
                ty.path_ref() == path
                    && ty
                        .identity()
                        .imported()
                        .is_some_and(|identity| identity.as_bytes() == &row.source)
            }),
            _ => false,
        };
        if !output_matches {
            return Err(Error::EventSelector);
        }
        let target = definitions
            .iter()
            .find(|definition| definition.root() == row.target_root)
            .ok_or(Error::EventFacade)?;
        let target_type = target
            .types()
            .find(|ty| {
                ty.path_ref()
                    .segments()
                    .iter()
                    .map(|segment| segment.as_str())
                    .eq(row.target_path.split("::"))
            })
            .filter(|ty| ty.parameters().len() == 1)
            .ok_or(Error::EventSelector)?;
        let mut found = false;
        for ty in source.types().filter(|ty| {
            ty.identity()
                .imported()
                .is_some_and(|identity| identity.as_bytes() == &row.source)
        }) {
            found = true;
            let bytes = ty
                .retained_bytes()
                .and_then(|bytes| bytes.checked_add(target_type.retained_bytes()?))
                .and_then(|bytes| {
                    bytes.checked_add(std::mem::size_of::<host::runtime::event::Adapter>())
                })
                .and_then(|bytes| bytes.checked_add(row.root.len()))
                .and_then(|bytes| bytes.checked_add(row.target_root.len()));
            allocation.retain_checked(Some(1), bytes)?;
            let key = host::runtime::event::Key::selected(
                host::facade::Dependency::new(source.root_path(), source.fingerprint().clone()),
                ty.contract(),
                host::facade::Dependency::new(target.root_path(), target.fingerprint().clone()),
                target_type.contract(),
                row.mapper,
            );
            if adapters
                .iter()
                .any(|adapter: &host::runtime::event::Adapter| adapter.matches(&key))
            {
                return Err(Error::EventDuplicate);
            }
            adapters
                .try_reserve_exact(1)
                .map_err(|_| Error::Allocation)?;
            adapters.push(host::runtime::event::Adapter::authenticated(key));
        }
        if !found {
            return Err(Error::EventSelector);
        }
    }
    Ok(adapters.into_boxed_slice())
}

impl StructuralRow {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(
        root: &'static str,
        owner: &'static str,
        selector: [u8; 32],
        identity: [u8; 32],
        signature: [u8; 32],
        arguments: u32,
        kind: StructuralKind,
    ) -> Self {
        Self {
            root,
            owner,
            selector,
            identity,
            signature,
            arguments,
            kind,
            instance: None,
        }
    }

    /// The exact generic instance authenticated by this row's adapter.
    #[doc(hidden)]
    #[must_use]
    pub const fn with_instance(mut self, instance: [u8; 32]) -> Self {
        self.instance = Some(instance);
        self
    }
}

/// A Rust future, which no crate's type names, is the core's future
/// (D511.3, D513.3), and std's `Duration` and `Instant` are Rawr's own.
fn semantic_type(
    identity: &[u8; 32],
    arguments: Box<[host::type_ref::Type]>,
) -> Result<Option<host::type_ref::Type>, host::Error> {
    use rawr_import::surface::identity::Definition;
    if identity == Definition::future().as_bytes() {
        let future = crate::future::facade::definition()?.export_type("future::Future")?;
        return host::type_ref::Type::external(future, arguments).map(Some);
    }
    if identity == Definition::duration().as_bytes() && arguments.is_empty() {
        return Ok(Some(host::type_ref::Type::builtin(
            host::type_ref::Builtin::Duration,
        )));
    }
    if identity == Definition::instant().as_bytes() && arguments.is_empty() {
        return Ok(Some(host::type_ref::Type::builtin(
            host::type_ref::Builtin::Instant,
        )));
    }
    Ok(None)
}

/// Install the generated bindings embedded by one compiler-owned demand brand.
pub fn install(
    engine: &mut crate::Engine,
    bytes: &[u8],
) -> Result<Box<[host::facade::Definition]>, Error> {
    let limits = rawr_import::cargo::Limits::standard();
    let plan = facade::Plan::decode(bytes, &limits)?;
    let mut allocation = account::Attempt::standard()?;
    install_decoded(engine, &plan, &mut allocation)
}

/// Install generated bindings under their retained native account.
#[doc(hidden)]
pub fn install_retained(
    engine: &mut crate::Engine,
    bytes: &[u8],
    native: &mut account::Native,
) -> Result<Box<[host::facade::Definition]>, Error> {
    let limits = rawr_import::cargo::Limits::standard();
    let plan = facade::Plan::decode(bytes, &limits)?;
    install_decoded(engine, &plan, &mut native.allocation())
}

fn install_decoded(
    engine: &mut crate::Engine,
    plan: &facade::Plan,
    allocation: &mut account::Attempt,
) -> Result<Box<[host::facade::Definition]>, Error> {
    let mut shapes = Vec::<([u8; 32], crate::Shared<host::type_def::Shape>)>::new();
    let mut definitions = Vec::new();
    use host::ImportAllocation as _;
    allocation.retain_checked(
        Some(plan.facades().len()),
        plan.facades()
            .len()
            .checked_mul(std::mem::size_of::<host::facade::Definition>()),
    )?;
    definitions
        .try_reserve_exact(plan.facades().len())
        .map_err(|_| Error::Allocation)?;
    for facade in plan.facades() {
        let (definition, families) = build(facade, &mut shapes, allocation)?;
        retain_registry_definition(&definition, allocation)?;
        engine
            .register_host_facade(definition.clone())
            .map_err(|source| entry_error(facade.root(), "", source.into()))?;
        engine
            .host_registry
            .defer_imported(families)
            .map_err(|source| entry_error(facade.root(), "", source.into()))?;
        definitions.push(definition);
    }
    Ok(definitions.into_boxed_slice())
}

fn retain_registry_definition(
    definition: &host::facade::Definition,
    allocation: &mut account::Attempt,
) -> Result<(), Error> {
    // Registry usage includes inline stores and imported indices. Add the
    // canonical owned payload behind paths, declarations and the fingerprint.
    let (items, mut bytes) = definition.registry_usage();
    for path in std::iter::once(definition.root_path()).chain(definition.modules()) {
        bytes = bytes.and_then(|bytes| {
            bytes.checked_add(
                path.retained_bytes()?
                    .checked_sub(std::mem::size_of_val(path))?,
            )
        });
    }
    for ty in definition.types() {
        bytes = bytes.and_then(|bytes| {
            bytes.checked_add(
                ty.retained_bytes()?
                    .checked_sub(std::mem::size_of_val(ty))?,
            )
        });
    }
    for item in definition.items() {
        bytes = bytes.and_then(|bytes| {
            bytes.checked_add(
                item.retained_bytes()?
                    .checked_sub(std::mem::size_of_val(item))?,
            )
        });
    }
    let fingerprint = definition.fingerprint();
    bytes = bytes.and_then(|bytes| {
        bytes.checked_add(
            fingerprint
                .retained_bytes()?
                .checked_sub(std::mem::size_of_val(fingerprint))?,
        )
    });
    use host::ImportAllocation as _;
    allocation.retain_checked(items, bytes)?;
    Ok(())
}

/// Admit manifest-authenticated private structural adapters into this compiler.
#[doc(hidden)]
pub fn admit_generated_structural(
    engine: &mut crate::Engine,
    definitions: &[host::facade::Definition],
    manifest: &[u8],
    rows: &[StructuralRow],
) -> Result<(), Error> {
    let adapters = authenticate_generated_structural(definitions, manifest, rows)?;
    let mut allocation = account::Attempt::standard()?;
    for adapter in adapters.iter() {
        engine
            .host_registry
            .admit_structural(adapter.definition.clone(), &mut allocation)?;
    }
    Ok(())
}

/// Admit a generated runner's proof-backed applications into this compiler.
/// Each must have an adapter the sealed demand generated for it; the runner
/// never proves anything, it replays what the host proved.
#[doc(hidden)]
pub fn admit_generated_applications(
    engine: &mut crate::Engine,
    _definitions: &[host::facade::Definition],
    encoded: &[u8],
    adapters: impl Iterator<Item = [u8; 32]>,
) -> Result<(), Error> {
    let applications = rawr_import::surface::application::Application::decode_all(encoded)?;
    let adapters = adapters.collect::<std::collections::BTreeSet<_>>();
    if applications.len() != adapters.len()
        || applications
            .iter()
            .any(|application| !adapters.contains(application.identity()))
    {
        return Err(Error::Surface(rawr_import::surface::Error::Canonical));
    }
    super::admit_applications(&mut engine.host_registry, &applications)
        .map_err(|error| Error::Application(error.to_string().into_boxed_str()))
}

/// Admit structural bindings under the same account as the retained carrier.
#[doc(hidden)]
pub fn admit_generated_structural_retained(
    engine: &mut crate::Engine,
    definitions: &[host::facade::Definition],
    rows: &[StructuralRow],
    native: &mut account::Native,
) -> Result<(), Error> {
    let mut scratch = native.attempt()?;
    let adapters = authenticate_structural(definitions, native.carrier(), rows, &mut scratch)?;
    for adapter in adapters {
        engine
            .host_registry
            .admit_structural(adapter.definition, &mut *native.allocation())?;
    }
    Ok(())
}

/// Authenticate the exact private structural bindings for one generated runtime.
#[doc(hidden)]
pub fn authenticate_generated_structural(
    definitions: &[host::facade::Definition],
    manifest: &[u8],
    rows: &[StructuralRow],
) -> Result<Box<[host::runtime::StructuralAdapter]>, Error> {
    let carrier = rawr_import::runner::manifest::Carrier::transform(manifest)?;
    let mut allocation = account::Attempt::standard()?;
    authenticate_structural(definitions, &carrier, rows, &mut allocation)
}

/// Authenticate runtime structural bindings while the native owner retains them.
#[doc(hidden)]
pub fn authenticate_generated_structural_retained(
    definitions: &[host::facade::Definition],
    rows: &[StructuralRow],
    native: &mut account::Native,
) -> Result<Box<[host::runtime::StructuralAdapter]>, Error> {
    let (carrier, mut allocation) = native.parts();
    authenticate_structural(definitions, carrier, rows, &mut allocation)
}

fn authenticate_structural(
    definitions: &[host::facade::Definition],
    carrier: &rawr_import::runner::manifest::Carrier,
    rows: &[StructuralRow],
    allocation: &mut account::Attempt,
) -> Result<Box<[host::runtime::StructuralAdapter]>, Error> {
    use host::ImportAllocation as _;
    allocation.retain_checked(
        Some(rows.len()),
        rows.len()
            .checked_mul(std::mem::size_of::<host::runtime::StructuralAdapter>()),
    )?;
    let mut adapters = Vec::new();
    adapters
        .try_reserve_exact(rows.len())
        .map_err(|_| Error::Allocation)?;
    for row in rows {
        if !carrier.contains(&row.identity, &row.signature, row.arguments) {
            return Err(Error::StructuralManifest);
        }
        let facade = definitions
            .iter()
            .find(|definition| definition.root() == row.root)
            .ok_or(Error::StructuralFacade)?;
        let (path, kind) = structural_path(facade, row)?;
        if kind != row.kind {
            return Err(Error::StructuralSelector);
        }
        let path_bytes = path.display_len().ok_or(Error::Allocation)?;
        allocation.retain_checked(Some(0), Some(path_bytes))?;
        let mut display = String::new();
        display
            .try_reserve_exact(path_bytes)
            .map_err(|_| Error::Allocation)?;
        for (index, segment) in path.segments().iter().enumerate() {
            if index != 0 {
                display.push_str("::");
            }
            display.push_str(segment);
        }
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let definition = host::structural::Definition::new(
            owner,
            display.into(),
            row.selector,
            row.identity,
            row.signature,
            row.arguments,
            *carrier.identity(),
            match row.kind {
                StructuralKind::Constructor => host::structural::Kind::Constructor,
                StructuralKind::Field { copy } => host::structural::Kind::Field { copy },
                StructuralKind::Variant => host::structural::Kind::Variant,
                StructuralKind::Match => host::structural::Kind::Match,
                StructuralKind::Store => host::structural::Kind::Store,
            },
        )
        .with_instance(row.instance);
        if adapters
            .iter()
            .any(|adapter: &host::runtime::StructuralAdapter| adapter.definition == definition)
        {
            return Err(Error::StructuralDuplicate);
        }
        adapters.push(host::runtime::StructuralAdapter::transform(definition));
    }
    Ok(adapters.into_boxed_slice())
}

/// A field's type `ty` that is one of `owner`'s parameters, at its default:
/// the bridge reads a generic struct's fields at its defaulted instance, as
/// palette's `Rgb` is `Rgb<Srgb, f32>`.
fn defaulted<'a>(
    ty: &'a host::type_ref::Type,
    owner: &'a host::type_def::Definition,
) -> &'a host::type_ref::Type {
    let host::type_ref::Shape::Parameter(parameter) = ty.shape() else {
        return ty;
    };
    owner
        .parameters()
        .iter()
        .position(|name| name.as_str() == parameter.name())
        .and_then(|index| owner.defaults().get(index))
        .and_then(Option::as_ref)
        .unwrap_or(ty)
}

/// Whether a field of type `ty` is read as a copy, as the generated bridge
/// reads it: a scalar, an imported Copy type of `facade`, or an `Option` or
/// `Result` of them.
fn copy_scalar(ty: &host::type_ref::Type, facade: &host::facade::Definition) -> bool {
    match ty.shape() {
        host::type_ref::Shape::Imported { identity, .. } => facade.types().any(|definition| {
            definition.identity().imported() == Some(identity)
                && definition
                    .capabilities()
                    .contains(crate::capability::Kind::Copy)
        }),
        host::type_ref::Shape::Unit => true,
        host::type_ref::Shape::Builtin(builtin) => matches!(
            builtin,
            host::type_ref::Builtin::Bool
                | host::type_ref::Builtin::Char
                | host::type_ref::Builtin::F64
                | host::type_ref::Builtin::I64
                | host::type_ref::Builtin::I8
                | host::type_ref::Builtin::I16
                | host::type_ref::Builtin::I32
                | host::type_ref::Builtin::Isize
                | host::type_ref::Builtin::U8
                | host::type_ref::Builtin::U16
                | host::type_ref::Builtin::U32
                | host::type_ref::Builtin::U64
                | host::type_ref::Builtin::Usize
                | host::type_ref::Builtin::F32
        ),
        host::type_ref::Shape::Option(inner) => copy_scalar(inner, facade),
        host::type_ref::Shape::Result(ok, err) => {
            copy_scalar(ok, facade) && copy_scalar(err, facade)
        }
        _ => false,
    }
}

fn structural_path<'facade>(
    facade: &'facade host::facade::Definition,
    row: &StructuralRow,
) -> Result<(&'facade host::path::Path, StructuralKind), Error> {
    let mut found: Option<(
        &host::path::Path,
        StructuralKind,
        Option<&host::type_def::import::Identity>,
    )> = None;
    let owned = |definition: &host::type_def::Definition| {
        row.owner.is_empty() || {
            let path = definition.path();
            let path = path.as_str();
            path == row.owner
                || path
                    .strip_suffix(row.owner)
                    .is_some_and(|prefix| prefix.ends_with("::"))
        }
    };
    for definition in facade.types().filter(|definition| owned(definition)) {
        let candidate = match definition.shape() {
            host::type_def::Shape::Struct {
                constructor,
                fields,
                ..
            } => constructor
                .iter()
                .filter(|selector| selector.fingerprint() == row.selector)
                .map(|_| StructuralKind::Constructor)
                .chain(fields.iter().filter_map(|field| {
                    // A field's selector names its reader and its store.
                    (field.imported_identity().fingerprint() == row.selector).then_some(
                        if row.kind == StructuralKind::Store {
                            StructuralKind::Store
                        } else if row.instance.is_some() && !definition.parameters().is_empty() {
                            // The authenticated adapter's signature is at
                            // the selected instance, not the type's defaults.
                            match row.kind {
                                StructuralKind::Field { copy } => StructuralKind::Field { copy },
                                _ => return None,
                            }
                        } else {
                            StructuralKind::Field {
                                copy: copy_scalar(defaulted(field.ty(), definition), facade),
                            }
                        },
                    )
                }))
                .next(),
            // A variant's selector names its constructor and its matcher.
            host::type_def::Shape::Enum(variants) => variants.iter().find_map(|variant| {
                (variant.imported_identity().fingerprint() == row.selector).then_some(
                    if row.kind == StructuralKind::Match {
                        StructuralKind::Match
                    } else {
                        StructuralKind::Variant
                    },
                )
            }),
            host::type_def::Shape::Opaque | host::type_def::Shape::Trait(_) => None,
        };
        if let Some(kind) = candidate {
            let identity = definition.identity().imported();
            match found {
                // One type at two paths, as ratatui re-exports
                // `ratatui_core::layout::Rect`: the first path stands.
                Some((_, _, first)) if first.is_some() && first == identity => continue,
                Some(_) => return Err(Error::StructuralDuplicate),
                None => found = Some((definition.path_ref(), kind, identity)),
            }
        }
    }
    found
        .map(|(path, kind, _)| (path, kind))
        .ok_or(Error::StructuralSelector)
}

fn build(
    facade: &facade::Facade,
    shapes: &mut Vec<([u8; 32], crate::Shared<host::type_def::Shape>)>,
    allocation: &mut account::Attempt,
) -> Result<(host::facade::Definition, Vec<host::family::Pending>), Error> {
    let abi = host::facade::Abi::new(1)?;
    let mut builder = host::facade::Builder::new_imported(facade.root(), abi, allocation)
        .map_err(|source| entry_error(facade.root(), "", source.into()))?;
    let type_paths = facade
        .types()
        .map(facade::TypeDefinition::path)
        .collect::<Vec<_>>();
    // An alias's associated functions, as `widget::Text::new`, sit under
    // the alias, which is no module. Sorted, so each prefix of each path is
    // a search, not a scan of every type.
    let mut owner_paths = type_paths
        .iter()
        .copied()
        .chain(facade.type_aliases().map(facade::TypeAlias::path))
        .collect::<Vec<_>>();
    owner_paths.sort_unstable();
    let mut modules = Vec::<Box<str>>::new();
    for path in type_paths
        .iter()
        .copied()
        .chain(facade.items().map(facade::Item::path))
    {
        collect_modules(path, &owner_paths, &mut modules)?;
    }
    for module in modules {
        builder
            .imported_module(&module, allocation)
            .map_err(|source| entry_error(facade.root(), &module, source.into()))?;
    }
    for ty in facade.types() {
        let identity = *ty.identity();
        let shape = match shapes.binary_search_by_key(&identity, |(identity, _)| *identity) {
            Ok(index) => shapes[index].1.clone(),
            Err(index) => {
                let shape = shape(ty.shape())
                    .map_err(|source| entry_error(facade.root(), ty.path(), source))?;
                retain_shape(&shape, allocation)?;
                let shape = crate::Shared::new(shape);
                shapes.try_reserve(1).map_err(|_| Error::Allocation)?;
                shapes.insert(index, (identity, shape.clone()));
                shape
            }
        };
        let parameters = ty.parameters().map(ImmutableString::from).collect();
        let defaults = ty
            .defaults()
            .iter()
            .map(|default| default.as_ref().map(self::ty).transpose())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| entry_error(facade.root(), ty.path(), source))?
            .into_boxed_slice();
        builder
            .imported_type(
                ty.path(),
                host::type_def::import::Identity::new(identity),
                &crate::package::import::implemented([
                    ty.copy(),
                    ty.default(),
                    ty.debug(),
                    ty.cloneable(),
                    ty.partial_eq(),
                    ty.eq(),
                    ty.hash(),
                    ty.iterator(),
                    ty.widget(),
                ]),
                parameters,
                defaults,
                shape,
                allocation,
            )
            .map_err(|source| entry_error(facade.root(), ty.path(), source.into()))?;
    }
    for item in facade.items() {
        // The item, and its form that a value fills, when it has one.
        for signature in std::iter::once(item.signature()).chain(item.values()) {
            let definition = item_definition(facade, item, signature)
                .map_err(|source| entry_error(facade.root(), item.path(), source))?;
            builder
                .imported_item(definition)
                .map_err(|source| entry_error(facade.root(), item.path(), source.into()))?;
        }
    }
    // A type alias of a builtin, as iced's `markdown::Uri` is `String`.
    for (path, builtin) in facade.builtins() {
        builder
            .builtin_alias(path, builtin)
            .map_err(|source| entry_error(facade.root(), path, source.into()))?;
    }
    // A crate's re-export of a std item, as iced's `time::Duration`.
    for (path, std) in facade.std_reexports() {
        match builder.std_alias(path, std) {
            // A path the crate also declares something at keeps that.
            Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
            Err(source) => return Err(entry_error(facade.root(), path, source.into())),
        }
    }
    // A crate's type alias, as `pub type Result = std::result::Result<(),
    // Error>;`.
    for alias in facade.type_aliases() {
        let path = alias.path();
        let parameters = alias.parameters().map(ImmutableString::from).collect();
        let defaults = alias
            .defaults()
            .iter()
            .map(|default| default.as_ref().map(self::ty).transpose())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| entry_error(facade.root(), path, source))?
            .into_boxed_slice();
        let target =
            self::ty(alias.target()).map_err(|source| entry_error(facade.root(), path, source))?;
        builder
            .type_alias(path, parameters, defaults, target)
            .map_err(|source| entry_error(facade.root(), path, source.into()))?;
    }
    // The headers of the traits the program names and of the crate's trait
    // impls, which fix a type a trait's defaults leave open (D604). A header
    // naming a type Rawr can't install is left out.
    for header in facade.trait_headers() {
        let defaults = header
            .defaults()
            .iter()
            .map(|default| default.as_ref().map(self::ty).transpose())
            .collect::<Result<Box<[_]>, _>>();
        let Ok(defaults) = defaults else {
            continue;
        };
        builder
            .trait_header(
                header.path(),
                *header.identity(),
                header.parameters().map(ImmutableString::from).collect(),
                defaults,
            )
            .map_err(|source| entry_error(facade.root(), header.path(), source.into()))?;
    }
    for header in facade.impls() {
        let types = |types: &[facade::Type]| {
            types
                .iter()
                .map(self::ty)
                .collect::<Result<Box<[_]>, _>>()
                .ok()
        };
        let (Some(arguments), Some(self_type)) =
            (types(header.arguments()), self::ty(header.self_type()).ok())
        else {
            continue;
        };
        let bounds = header
            .bounds()
            .iter()
            .filter_map(|bound| {
                Some(host::facade::ImplBound {
                    ty: self::ty(bound.ty()).ok()?,
                    trait_identity: *bound.trait_identity(),
                    arguments: types(bound.arguments())?,
                })
            })
            .collect();
        builder
            .impl_header(host::facade::ImplHeader {
                parameters: header.parameters().map(ImmutableString::from).collect(),
                trait_identity: *header.trait_identity(),
                arguments,
                self_type,
                bounds,
            })
            .map_err(|source| entry_error(facade.root(), "<impl>", source.into()))?;
    }
    // A re-exported variant, as `pub use Alignment::Center;`, names its
    // enum's variant.
    for (path, variant) in facade.variants() {
        builder
            .variant_alias(path, variant)
            .map_err(|source| entry_error(facade.root(), path, source.into()))?;
    }
    // A trait the program names is a module (`use rand::Rng;`), unless the
    // bindings already declare something at its path (iced's `Widget`, whose
    // type the bindings carry).
    for path in facade.traits() {
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes.chain(std::iter::once(path)) {
            match builder.imported_module(module, allocation) {
                Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
                Err(error) => return Err(entry_error(facade.root(), module, error.into())),
            }
        }
    }
    // A dependency's `macro_rules!` macros, which Rawr expands, each in its
    // module: `$crate` reads as the defining crate's root through this root.
    for rules in facade.macros() {
        let path = rules.path();
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes {
            match builder.imported_module(module, allocation) {
                Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
                Err(error) => return Err(entry_error(facade.root(), module, error.into())),
            }
        }
        let item = host::item::Definition::macro_rules(
            path,
            rules.source(),
            format!("{}::{}", facade.root(), rules.krate()),
        )
        .map_err(|source| entry_error(facade.root(), path, source.into()))?;
        match builder.item(item) {
            Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
            Err(error) => return Err(entry_error(facade.root(), path, error.into())),
        }
    }
    builder
        .finish_imported(allocation)
        .map_err(|source| entry_error(facade.root(), "", source.into()))
}

fn retain_shape(
    shape: &host::type_def::Shape,
    allocation: &mut account::Attempt,
) -> Result<(), Error> {
    let mut items = Some(1_usize);
    let mut bytes = std::mem::size_of_val(shape).checked_add(2 * std::mem::size_of::<usize>());
    let mut retain_fields = |fields: &[host::type_def::Field]| {
        items = items.and_then(|items| items.checked_add(fields.len()));
        for field in fields {
            bytes = bytes.and_then(|bytes| {
                bytes
                    .checked_add(std::mem::size_of_val(field))?
                    .checked_add(field.name().len())?
                    .checked_add(field.imported_identity().retained_bytes()?)?
                    .checked_add(
                        field
                            .ty()
                            .retained_bytes()?
                            .checked_sub(std::mem::size_of_val(field.ty()))?,
                    )
            });
        }
    };
    match shape {
        host::type_def::Shape::Struct { fields, .. } => retain_fields(fields),
        host::type_def::Shape::Enum(variants) => {
            for variant in variants {
                retain_fields(variant.fields());
            }
        }
        host::type_def::Shape::Opaque | host::type_def::Shape::Trait(_) => {}
    }
    match shape {
        host::type_def::Shape::Struct { constructor, .. } => {
            bytes = bytes.and_then(|bytes| {
                bytes.checked_add(
                    constructor
                        .as_ref()
                        .map_or(Some(0), |selector| selector.retained_bytes())?,
                )
            });
        }
        host::type_def::Shape::Enum(variants) => {
            items = items.and_then(|items| items.checked_add(variants.len()));
            for variant in variants {
                bytes = bytes.and_then(|bytes| {
                    bytes
                        .checked_add(std::mem::size_of_val(variant))?
                        .checked_add(variant.name().len())?
                        .checked_add(variant.imported_identity().retained_bytes()?)
                });
            }
        }
        host::type_def::Shape::Opaque | host::type_def::Shape::Trait(_) => {}
    }
    use host::ImportAllocation as _;
    allocation.retain_checked(items, bytes)?;
    Ok(())
}

fn entry_error(root: &str, path: &str, source: Error) -> Error {
    // The innermost item names the failure: a type's field registers
    // under its type.
    if let Error::Registration { .. } = source {
        return source;
    }
    Error::Registration {
        root: root.into(),
        path: path.into(),
        source: Box::new(source),
    }
}

/// `root::path` as Rust names it, or `root` alone for the crate itself.
fn rust_item(root: &str, path: &str) -> String {
    if path.is_empty() {
        root.to_owned()
    } else {
        format!("{root}::{path}")
    }
}

fn shape(shape: &facade::Shape) -> Result<host::type_def::Shape, Error> {
    if let Some((form, constructor, fields, constructible)) = shape.struct_parts() {
        return Ok(host::type_def::Shape::Struct {
            form: form.into(),
            constructor: constructor
                .map(rawr_import::surface::identity::Selector::try_clone)
                .transpose()?,
            fields: fields
                .iter()
                .map(field)
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
            constructible,
        });
    }
    let variants = shape.variants().ok_or(Error::Shape)?;
    Ok(host::type_def::Shape::Enum(
        variants
            .iter()
            .map(|variant| {
                Ok(host::type_def::Variant::new(
                    variant.name().into(),
                    variant
                        .fields()
                        .iter()
                        .map(field)
                        .collect::<Result<Vec<_>, Error>>()?
                        .into_boxed_slice(),
                    variant.constructible(),
                    variant.selector().try_clone()?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>()?
            .into_boxed_slice(),
    ))
}

fn field(field: &facade::Field) -> Result<host::type_def::Field, Error> {
    Ok(host::type_def::Field::new(
        field.name().into(),
        ty(field.ty())?,
        field.selector().try_clone()?,
    ))
}

fn item_definition(
    facade: &facade::Facade,
    item: &facade::Item,
    form: &facade::Signature,
) -> Result<host::item::Definition, Error> {
    let signature = signature(facade, form)?;
    let definition = match item.kind() {
        ItemKind::Function => host::item::Definition::function(item.path(), signature)?,
        ItemKind::AssociatedFunction => {
            host::item::Definition::associated_function(item.path(), signature)?
        }
        ItemKind::OwnedMethod => {
            host::item::Definition::method(item.path(), receiver::Mode::Owned, signature)?
        }
        ItemKind::SharedMethod => {
            host::item::Definition::method(item.path(), receiver::Mode::Shared, signature)?
        }
        ItemKind::MutableMethod => {
            host::item::Definition::method(item.path(), receiver::Mode::Mutable, signature)?
        }
        ItemKind::Constant => host::item::Definition::value(item.path(), ty(form.output())?)?,
        ItemKind::AssociatedConstant => {
            host::item::Definition::associated_value(item.path(), ty(form.output())?)?
        }
    };
    let guard = form.guard().map(|guard| match guard {
        rawr_import::classify::representability::signature::Guard::Shared => {
            host::item::Guard::Shared
        }
        rawr_import::classify::representability::signature::Guard::Mutable => {
            host::item::Guard::Mutable
        }
        rawr_import::classify::representability::signature::Guard::OptionShared => {
            host::item::Guard::OptionShared
        }
        rawr_import::classify::representability::signature::Guard::OptionMutable => {
            host::item::Guard::OptionMutable
        }
    });
    Ok(definition
        .imported(item.selector().try_clone()?)
        .guarded(guard)
        .opening(form.scope().is_some()))
}

fn signature(
    facade: &facade::Facade,
    signature: &facade::Signature,
) -> Result<host::signature::Definition, Error> {
    let mut builder = host::signature::Builder::new();
    for (index, parameter) in signature.parameters().enumerate() {
        builder.type_parameter(parameter, &super::requirements(signature.bounds(index)))?;
    }
    if signature.crossing() {
        builder.crossing();
    }
    for input in signature.inputs() {
        builder.try_parameter(ty(input)?)?;
    }
    builder.output(ty(signature.output())?);
    if let Some(instance) = signature.instance() {
        builder.instance(ty(instance)?);
    }
    for relation in signature.relations() {
        if let Some(relation) = super::relation(
            relation.parameter(),
            relation.arguments(),
            relation
                .cases()
                .iter()
                .map(|case| (case.this(), case.arguments())),
            relation.trait_identity(),
            |source| {
                pattern(source, &|nominal| {
                    ty(nominal).ok().and_then(|ty| rooted(facade.root(), ty))
                })
            },
        ) {
            builder.relation(relation)?;
        }
    }
    Ok(builder.finish())
}

/// `ty` with a facade type's path from the root `root`, as a relation's
/// case names it: a relation is read where the program calls the item, not
/// resolved where the facade installs (`Padding`, which pane_grid's
/// `TitleBar::padding(impl Into<Padding>)` converts to).
fn rooted(root: &str, ty: host::type_ref::Type) -> Option<host::type_ref::Type> {
    match ty.shape() {
        host::type_ref::Shape::Registered { path, arguments } if arguments.is_empty() => {
            let path = path.to_string();
            if path.starts_with(&format!("{root}::")) {
                Some(ty)
            } else {
                host::type_ref::Type::registered(format!("{root}::{path}")).ok()
            }
        }
        host::type_ref::Shape::Registered { .. } => None,
        _ => Some(ty),
    }
}

/// Whether `source` names an implementation's own type parameter or the
/// item's.
fn names_variable(source: &facade::Type) -> bool {
    match source {
        facade::Type::Variable(_) | facade::Type::Parameter(_) => true,
        facade::Type::Nominal { arguments, .. } | facade::Type::Tuple(arguments) => {
            arguments.iter().any(names_variable)
        }
        facade::Type::Option(element)
        | facade::Type::Array(element)
        | facade::Type::FixedArray { element, .. }
        | facade::Type::Range(element) => names_variable(element),
        facade::Type::Result(ok, err) => names_variable(ok) || names_variable(err),
        facade::Type::FunctionItem { input, output } | facade::Type::Callable { input, output } => {
            names_variable(input) || names_variable(output)
        }
        _ => false,
    }
}

/// A relation's type, when it is one a relation can match. `nominal` gives
/// a facade type as a host type.
fn pattern(
    source: &facade::Type,
    nominal: &dyn Fn(&facade::Type) -> Option<host::type_ref::Type>,
) -> Option<host::signature::Pattern> {
    use host::signature::Pattern;
    use host::type_ref::Builtin;
    Some(match source {
        facade::Type::Unit => Pattern::Unit,
        facade::Type::Bool => Pattern::Builtin(Builtin::Bool),
        facade::Type::Char => Pattern::Builtin(Builtin::Char),
        facade::Type::F64 => Pattern::Builtin(Builtin::F64),
        facade::Type::I64 => Pattern::Builtin(Builtin::I64),
        facade::Type::Primitive(name) => Pattern::Builtin(Builtin::primitive(name)?),
        facade::Type::Parameter(name) => Pattern::Parameter(name.as_ref().into()),
        facade::Type::Variable(index) => Pattern::Variable(*index),
        facade::Type::Range(element) => Pattern::Range(Box::new(pattern(element, nominal)?)),
        facade::Type::Option(element) => Pattern::Option(Box::new(pattern(element, nominal)?)),
        facade::Type::Tuple(members) => Pattern::Tuple(
            members
                .iter()
                .map(|member| pattern(member, nominal))
                .collect::<Option<_>>()?,
        ),
        facade::Type::Nominal { .. } | facade::Type::FixedArray { .. }
            if !names_variable(source) =>
        {
            Pattern::Type(nominal(source)?)
        }
        _ => return None,
    })
}

fn ty(source: &facade::Type) -> Result<host::type_ref::Type, Error> {
    Ok(match source {
        facade::Type::Unit => host::type_ref::Type::unit(),
        facade::Type::Bool => host::type_ref::Type::builtin(host::type_ref::Builtin::Bool),
        facade::Type::Char => host::type_ref::Type::builtin(host::type_ref::Builtin::Char),
        facade::Type::F64 => host::type_ref::Type::builtin(host::type_ref::Builtin::F64),
        facade::Type::I64 => host::type_ref::Type::builtin(host::type_ref::Builtin::I64),
        facade::Type::InclusiveRange => {
            host::type_ref::Type::builtin(host::type_ref::Builtin::InclusiveRange)
        }
        facade::Type::String => host::type_ref::Type::builtin(host::type_ref::Builtin::String),
        facade::Type::Primitive(name) => host::type_ref::Type::builtin(
            host::type_ref::Builtin::primitive(name).ok_or(Error::Shape)?,
        ),
        facade::Type::Nominal {
            identity,
            arguments,
        } => {
            let arguments = arguments
                .iter()
                .map(ty)
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            // core's `RangeInclusive<T>`: Rawr's range of `T`'s kind of
            // number.
            if identity == rawr_import::surface::identity::Definition::range_inclusive().as_bytes()
                && let [element] = &*arguments
            {
                return Ok(host::type_ref::Type::range_inclusive_of(element.clone()));
            }
            // A script closure a crate value holds, as iced's `Map<W, F, A>`
            // from `Widget::map` does: the closure's type (T417).
            if identity == rawr_import::surface::identity::Definition::callable().as_bytes() {
                let (output, inputs) = arguments.split_last().ok_or(Error::Shape)?;
                let input = match inputs {
                    [] => host::type_ref::Type::unit(),
                    [single] => single.clone(),
                    many => host::type_ref::Type::tuple(many.to_vec())?,
                };
                return Ok(host::type_ref::Type::callable(input, output.clone()));
            }
            if let Some(ty) = semantic_type(identity, arguments.clone())? {
                return Ok(ty);
            }
            host::type_ref::Type::imported_boxed(
                host::type_def::import::Identity::new(*identity),
                arguments,
            )
        }
        facade::Type::Callable { input, output } => {
            host::type_ref::Type::callable(ty(input)?, ty(output)?)
        }
        facade::Type::FunctionItem { input, output } => {
            host::type_ref::Type::imported_function_item(ty(input)?, ty(output)?)
        }
        facade::Type::Parameter(parameter) => host::type_ref::Type::parameter(parameter.as_ref())?,
        facade::Type::Reference { mutable, target } => {
            host::type_ref::Type::reference(*mutable, ty(target)?)
        }
        facade::Type::Option(element) => host::type_ref::Type::option(ty(element)?),
        facade::Type::Range(element) => host::type_ref::Type::range(ty(element)?),
        // Only a relation's case names an impl's own parameter.
        facade::Type::Variable(_) => return Err(Error::Shape),
        facade::Type::Result(ok, err) => host::type_ref::Type::result(ty(ok)?, ty(err)?),
        facade::Type::Array(element) => host::type_ref::Type::slice(ty(element)?),
        facade::Type::FixedArray { element, length } => host::type_ref::Type::array(
            ty(element)?,
            usize::try_from(*length).map_err(|_| Error::Shape)?,
        ),
        facade::Type::Tuple(members) => {
            host::type_ref::Type::tuple(members.iter().map(ty).collect::<Result<Vec<_>, _>>()?)?
        }
    })
}

/// The modules above `path`, up to the first type. `types` is sorted.
fn collect_modules(path: &str, types: &[&str], modules: &mut Vec<Box<str>>) -> Result<(), Error> {
    for (separator, _) in path.match_indices("::") {
        let candidate = &path[..separator];
        if types.binary_search(&candidate).is_ok() {
            break;
        }
        match modules.binary_search_by(|module| module.as_ref().cmp(candidate)) {
            Ok(_) => {}
            Err(index) => {
                modules.try_reserve(1).map_err(|_| Error::Allocation)?;
                modules.insert(index, candidate.into());
            }
        }
    }
    if types.binary_search(&path).is_ok()
        && let Ok(index) = modules.binary_search_by(|module| module.as_ref().cmp(path))
    {
        modules.remove(index);
    }
    Ok(())
}

impl From<facade::Form> for host::type_def::Form {
    fn from(form: facade::Form) -> Self {
        match form {
            facade::Form::Unit => Self::Unit,
            facade::Form::Tuple => Self::Tuple,
            facade::Form::Record => Self::Record,
        }
    }
}

/// Invalid generated bindings or their registration: a rawr bug, never the
/// program's.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A replacement compiler has no original child allocation owner.
    #[error("native reconstruction requires retained compiler ownership")]
    NativeOwner,
    /// Importer plan decoding or semantic allocation failed.
    #[error(transparent)]
    Surface(#[from] rawr_import::surface::Error),
    /// Embedded generated-manifest authentication failed.
    #[error(transparent)]
    Manifest(#[from] rawr_import::runner::manifest::Error),
    /// Validating a generated declaration failed.
    #[error(transparent)]
    Host(#[from] host::Error),
    /// Registering one generated item failed. The bindings are rawr's own,
    /// so this is a rawr bug.
    #[error(
        "internal error: rawr cannot register its bindings for `{}`: {source}. This is a rawr bug",
        rust_item(.root, .path)
    )]
    Registration {
        /// The crate's root.
        root: Box<str>,
        /// The Rust item's path below the root, empty for the crate itself.
        path: Box<str>,
        /// What went wrong.
        #[source]
        source: Box<Error>,
    },
    /// Admitting a generated application failed: a rawr bug.
    #[error("internal error: rawr cannot admit a generated application: {0}. This is a rawr bug")]
    Application(Box<str>),
    /// The plan contains contradictory nominal shape evidence.
    #[error("a generated type has no nominal shape")]
    Shape,
    /// Bounded allocation of the bindings failed.
    #[error("cannot allocate generated bindings")]
    Allocation,
    /// A private event producer is absent from the generated manifest.
    #[error("generated event producer is absent from its manifest")]
    EventManifest,
    /// A private event binding names bindings that are absent.
    #[error("generated event names absent bindings")]
    EventFacade,
    /// A private event row disagrees with its source or target contract.
    #[error("generated event source or target selector disagrees")]
    EventSelector,
    /// More than one producer claims the same private event binding.
    #[error("generated event conversion is duplicated")]
    EventDuplicate,
    /// A structural row names no exact installed bindings.
    #[error("generated structural adapter names no installed bindings")]
    StructuralFacade,
    /// A structural selector is absent, ambiguous, or has the wrong operation kind.
    #[error("generated structural adapter does not match one exact selector")]
    StructuralSelector,
    /// A structural row is absent from the authenticated generated manifest.
    #[error("generated structural adapter is absent from the authenticated manifest")]
    StructuralManifest,
    /// A generated structural adapter is duplicated.
    #[error("generated structural adapter is duplicated")]
    StructuralDuplicate,
}
