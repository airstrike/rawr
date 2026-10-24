//! Generated-runner reconstruction of an exact imported facade plan.

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
    Field { copy: bool },
    Variant,
    Match,
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
        }
    }
}

/// Linked profile decisions applied while reconstructing one exact imported facade.
///
/// The importer remains authoritative for every declaration not claimed by this
/// hook. A profile may substitute only already-validated host proofs and must
/// map every imported identity it replaces back to the corresponding semantic
/// type reference.
#[doc(hidden)]
pub trait Profile {
    /// Return whether all profile-owned target dependencies for this facade exist.
    fn ready(&self, _facade: &facade::Facade, _installed: &[host::facade::Definition]) -> bool {
        true
    }

    /// Return one complete unregistered compatibility seed for this imported root.
    fn compatibility_seed<'a>(
        &'a self,
        _facade: &facade::Facade,
    ) -> Option<&'a host::facade::Definition> {
        None
    }

    /// Return the exact proof seal for one compatibility composition.
    fn compatibility_seal(&self, _facade: &facade::Facade) -> Option<[u8; 32]> {
        None
    }

    /// Whether one seed item survives later controlling suppression law.
    fn retain_compatibility_item(
        &self,
        _facade: &facade::Facade,
        _item: &host::item::Definition,
    ) -> bool {
        true
    }

    /// Whether this profile owns the imported type's semantic representation.
    fn owns_type(&self, facade: &facade::Facade, ty: &facade::TypeDefinition) -> bool;

    /// Install the linked semantic types owned by this profile.
    fn install_types(
        &self,
        facade: &facade::Facade,
        builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error>;

    /// Map one replaced imported identity to its linked semantic reference.
    /// `installed` holds the facades built before this one.
    fn semantic_type(
        &self,
        facade: &facade::Facade,
        installed: &[host::facade::Definition],
        identity: &[u8; 32],
        arguments: Box<[host::type_ref::Type]>,
    ) -> Result<Option<host::type_ref::Type>, host::Error>;

    /// Whether this profile owns the imported item's compiler/runtime meaning.
    fn owns_item(&self, facade: &facade::Facade, item: &facade::Item) -> bool;

    /// Install the linked semantic items owned by this profile.
    fn install_items(
        &self,
        facade: &facade::Facade,
        builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error>;

    /// Install exact support-only operation endpoints authenticated by this profile.
    fn install_endpoints(
        &self,
        _facade: &facade::Facade,
        _builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        Ok(())
    }

    /// Install exact direct projections already authenticated by this profile.
    fn install_projections(
        &self,
        _facade: &facade::Facade,
        _installed: &[host::facade::Definition],
        _builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        Ok(())
    }
}

struct Ordinary;

impl Profile for Ordinary {
    fn owns_type(&self, _facade: &facade::Facade, _ty: &facade::TypeDefinition) -> bool {
        false
    }

    fn install_types(
        &self,
        _facade: &facade::Facade,
        _builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        Ok(())
    }

    fn semantic_type(
        &self,
        _facade: &facade::Facade,
        _installed: &[host::facade::Definition],
        _identity: &[u8; 32],
        _arguments: Box<[host::type_ref::Type]>,
    ) -> Result<Option<host::type_ref::Type>, host::Error> {
        Ok(None)
    }

    fn owns_item(&self, _facade: &facade::Facade, _item: &facade::Item) -> bool {
        false
    }

    fn install_items(
        &self,
        _facade: &facade::Facade,
        _builder: &mut host::facade::Builder,
    ) -> Result<(), host::Error> {
        Ok(())
    }
}

/// Install the compiler facades embedded by one compiler-owned demand brand.
pub fn install(
    engine: &mut crate::Engine,
    bytes: &[u8],
) -> Result<Box<[host::facade::Definition]>, Error> {
    install_profile(engine, bytes, &Ordinary)
}

/// Reconstruct an ordinary generated facade under its retained native account.
#[doc(hidden)]
pub fn install_retained(
    engine: &mut crate::Engine,
    bytes: &[u8],
    native: &mut account::Native,
) -> Result<Box<[host::facade::Definition]>, Error> {
    let limits = rawr_import::cargo::Limits::standard();
    let plan = facade::Plan::decode(bytes, &limits)?;
    install_decoded_profile_retained(engine, &plan, &Ordinary, native)
}

/// Install exact imported facades with one linked profile's semantic anchors.
#[doc(hidden)]
pub fn install_profile(
    engine: &mut crate::Engine,
    bytes: &[u8],
    profile: &impl Profile,
) -> Result<Box<[host::facade::Definition]>, Error> {
    let limits = rawr_import::cargo::Limits::standard();
    let plan = facade::Plan::decode(bytes, &limits)?;
    install_decoded_profile(engine, &plan, profile)
}

/// Install one already-decoded exact plan with linked profile semantics.
#[doc(hidden)]
pub fn install_decoded_profile(
    engine: &mut crate::Engine,
    plan: &facade::Plan,
    profile: &impl Profile,
) -> Result<Box<[host::facade::Definition]>, Error> {
    let mut allocation = account::Attempt::standard()?;
    install_decoded(engine, plan, profile, &mut allocation)
}

/// Install decoded facades while retaining their charges with the native owner.
#[doc(hidden)]
pub fn install_decoded_profile_retained(
    engine: &mut crate::Engine,
    plan: &facade::Plan,
    profile: &impl Profile,
    native: &mut account::Native,
) -> Result<Box<[host::facade::Definition]>, Error> {
    install_decoded(engine, plan, profile, &mut native.allocation())
}

fn install_decoded(
    engine: &mut crate::Engine,
    plan: &facade::Plan,
    profile: &impl Profile,
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
    let mut pending = plan.facades().collect::<Vec<_>>();
    while !pending.is_empty() {
        let Some(index) = pending
            .iter()
            .position(|facade| profile.ready(facade, &definitions))
        else {
            return Err(Error::ProfileOrder);
        };
        let facade = pending.remove(index);
        let definition = build(facade, profile, &definitions, &mut shapes, allocation)?;
        retain_registry_definition(&definition, allocation)?;
        engine.register_host_facade(definition.clone())?;
        definitions.push(definition);
    }
    Ok(definitions.into_boxed_slice())
}

/// Consume a linked profile extension into the accounted native compiler.
#[doc(hidden)]
pub fn install_profile_facade_retained(
    engine: &mut crate::Engine,
    definition: host::facade::Definition,
    native: &mut account::Native,
) -> Result<(), Error> {
    retain_registry_definition(&definition, &mut native.allocation())?;
    engine.register_host_facade(definition)?;
    Ok(())
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
            },
        );
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

/// Whether a field of type `ty` is read as a copy, as the generated bridge
/// reads it: a scalar, an imported Copy type of `facade`, or an `Option` or
/// `Result` of them.
fn copy_scalar(ty: &host::type_ref::Type, facade: &host::facade::Definition) -> bool {
    match ty.shape() {
        host::type_ref::Shape::Imported { identity, .. } => facade.types().any(|definition| {
            definition.identity().imported() == Some(identity)
                && definition.capabilities().contains(crate::capability::Kind::Copy)
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
    let mut found = None;
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
                    (field.imported_identity().fingerprint() == row.selector).then_some(
                        StructuralKind::Field {
                            copy: copy_scalar(field.ty(), facade),
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
            if found.is_some() {
                return Err(Error::StructuralDuplicate);
            }
            found = Some((definition.path_ref(), kind));
        }
    }
    found.ok_or(Error::StructuralSelector)
}

fn build(
    facade: &facade::Facade,
    profile: &impl Profile,
    installed: &[host::facade::Definition],
    shapes: &mut Vec<([u8; 32], crate::Shared<host::type_def::Shape>)>,
    allocation: &mut account::Attempt,
) -> Result<host::facade::Definition, Error> {
    let abi = host::facade::Abi::new(1)?;
    let mut builder = host::facade::Builder::new_imported(facade.root(), abi, allocation)?;
    let compatibility = profile.compatibility_seed(facade);
    match (compatibility, profile.compatibility_seal(facade)) {
        (Some(seed), Some(seal)) if seed.root() == facade.root() => {
            if !seed.support_map().is_empty() || !seed.endpoint_map().is_empty() {
                return Err(Error::CompatibilitySeed);
            }
            builder.compatibility_seal(seal)?;
        }
        (None, None) => {}
        _ => return Err(Error::CompatibilitySeed),
    }
    let type_paths = facade
        .types()
        .map(facade::TypeDefinition::path)
        .collect::<Vec<_>>();
    let mut modules = Vec::<Box<str>>::new();
    for path in type_paths
        .iter()
        .copied()
        .chain(facade.items().map(facade::Item::path))
    {
        collect_modules(path, &type_paths, &mut modules)?;
    }
    if let Some(seed) = compatibility {
        let prefix = format!("{}::", facade.root());
        for module in seed.modules() {
            let display = module.to_string();
            let relative = display
                .strip_prefix(&prefix)
                .ok_or(Error::CompatibilitySeed)?;
            if type_paths.contains(&relative) {
                return Err(host::Error::DuplicateFacadeEntry {
                    path: display.into(),
                    existing: "imported type",
                    incoming: "compatibility module",
                }
                .into());
            }
            match modules.binary_search_by(|candidate| candidate.as_ref().cmp(relative)) {
                Ok(_) => {}
                Err(index) => {
                    modules.try_reserve(1).map_err(|_| Error::Allocation)?;
                    modules.insert(index, relative.into());
                }
            }
        }
    }
    for module in modules {
        builder.imported_module(&module, allocation)?;
    }
    // The members of a type the profile owns are the profile's under each
    // of the type's paths (iced's `Application` and
    // `application::Application`).
    let owned = facade
        .types()
        .filter(|ty| profile.owns_type(facade, ty))
        .map(|ty| *ty.identity())
        .collect::<Vec<_>>();
    // A path the compatibility seed re-exports (iced's `Renderer` and
    // `canvas::Event`) keeps the seed's meaning, whatever the importer made of
    // the Rust item there.
    let prefix = format!("{}::", facade.root());
    let seed_aliases = compatibility
        .map(|seed| {
            seed.aliases()
                .iter()
                .filter_map(|(alias, _)| alias.to_string().strip_prefix(&prefix).map(Box::<str>::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let aliased = |path: &str| seed_aliases.iter().any(|alias| alias.as_ref() == path);
    let owned_path = |path: &str| {
        aliased(path)
            || facade
                .types()
                .any(|ty| ty.path() == path && owned.contains(ty.identity()))
    };
    for ty in facade.types() {
        if profile.owns_type(facade, ty) || aliased(ty.path()) {
            continue;
        }
        let identity = *ty.identity();
        let shape = match shapes.binary_search_by_key(&identity, |(identity, _)| *identity) {
            Ok(index) => shapes[index].1.clone(),
            Err(index) => {
                let shape = shape(facade, ty.shape(), profile, installed)
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
            .map(|default| {
                default
                    .as_ref()
                    .map(|default| self::ty(facade, default, profile, installed))
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| entry_error(facade.root(), ty.path(), source))?
            .into_boxed_slice();
        builder
            .imported_type(
                ty.path(),
                host::type_def::import::Identity::new(identity),
                ty.copy(),
                ty.default(),
                ty.debug(),
                ty.cloneable(),
                parameters,
                defaults,
                shape,
                allocation,
            )
            .map_err(|source| entry_error(facade.root(), ty.path(), source.into()))?;
    }
    profile
        .install_types(facade, &mut builder)
        .map_err(|source| entry_error(facade.root(), "<profile-types>", source.into()))?;
    if let Some(seed) = compatibility {
        for definition in seed.types() {
            builder
                .compatibility_type(definition, allocation)
                .map_err(|source| entry_error(facade.root(), &definition.path(), source.into()))?;
        }
        for (alias, target) in seed.aliases() {
            builder
                .compatibility_alias(alias, target)
                .map_err(|source| entry_error(facade.root(), &alias.to_string(), source.into()))?;
        }
    }
    for item in facade.items() {
        let owner_owned = item
            .path()
            .rsplit_once("::")
            .is_some_and(|(owner, _)| owned_path(owner));
        if !profile.owns_item(facade, item) && !owner_owned {
            let definition = item_definition(facade, item, profile, installed)
                .map_err(|source| entry_error(facade.root(), item.path(), source))?;
            builder
                .item(definition)
                .map_err(|source| entry_error(facade.root(), item.path(), source.into()))?;
        }
    }
    profile
        .install_items(facade, &mut builder)
        .map_err(|source| entry_error(facade.root(), "<profile-items>", source.into()))?;
    if let Some(seed) = compatibility {
        for item in seed
            .items()
            .filter(|item| profile.retain_compatibility_item(facade, item))
        {
            builder
                .compatibility_item(item, allocation)
                .map_err(|source| entry_error(facade.root(), &item.path(), source.into()))?;
        }
    }
    profile
        .install_endpoints(facade, &mut builder)
        .map_err(|source| entry_error(facade.root(), "<profile-endpoints>", source.into()))?;
    profile
        .install_projections(facade, installed, &mut builder)
        .map_err(|source| entry_error(facade.root(), "<profile-projections>", source.into()))?;
    if let Some(seed) = compatibility {
        builder.install_compatibility_projections(seed)?;
    }
    // A trait the program names is a module (`use rand::Rng;`), unless the
    // profile already gives its path a meaning (iced's `Widget`).
    for path in facade.traits() {
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes.chain(std::iter::once(path)) {
            match builder.imported_module(module, allocation) {
                Ok(_) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    builder.finish_imported(allocation).map_err(Error::from)
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
    Error::FacadeEntry {
        root: root.into(),
        path: path.into(),
        source: Box::new(source),
    }
}

fn shape(
    facade: &facade::Facade,
    shape: &facade::Shape,
    profile: &impl Profile,
    installed: &[host::facade::Definition],
) -> Result<host::type_def::Shape, Error> {
    if let Some((form, constructor, fields, constructible)) = shape.struct_parts() {
        return Ok(host::type_def::Shape::Struct {
            form: form.into(),
            constructor: constructor
                .map(rawr_import::surface::identity::Selector::try_clone)
                .transpose()?,
            fields: fields
                .iter()
                .map(|source| field(facade, source, profile, installed))
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
                        .map(|source| field(facade, source, profile, installed))
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

fn field(
    facade: &facade::Facade,
    field: &facade::Field,
    profile: &impl Profile,
    installed: &[host::facade::Definition],
) -> Result<host::type_def::Field, Error> {
    Ok(host::type_def::Field::new(
        field.name().into(),
        ty(facade, field.ty(), profile, installed)?,
        field.selector().try_clone()?,
    ))
}

fn item_definition(
    facade: &facade::Facade,
    item: &facade::Item,
    profile: &impl Profile,
    installed: &[host::facade::Definition],
) -> Result<host::item::Definition, Error> {
    let signature = signature(facade, item.signature(), profile, installed)?;
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
        ItemKind::Constant => host::item::Definition::value(
            item.path(),
            ty(facade, item.signature().output(), profile, installed)?,
        )?,
        ItemKind::AssociatedConstant => host::item::Definition::associated_value(
            item.path(),
            ty(facade, item.signature().output(), profile, installed)?,
        )?,
    };
    Ok(definition.imported(item.selector().try_clone()?))
}

fn signature(
    facade: &facade::Facade,
    signature: &facade::Signature,
    profile: &impl Profile,
    installed: &[host::facade::Definition],
) -> Result<host::signature::Definition, Error> {
    let mut builder = host::signature::Builder::new();
    for parameter in signature.parameters() {
        builder.type_parameter(parameter, &[])?;
    }
    for input in signature.inputs() {
        builder.try_parameter(ty(facade, input, profile, installed)?)?;
    }
    builder.output(ty(facade, signature.output(), profile, installed)?);
    Ok(builder.finish())
}

fn ty(
    facade: &facade::Facade,
    source: &facade::Type,
    profile: &impl Profile,
    installed: &[host::facade::Definition],
) -> Result<host::type_ref::Type, Error> {
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
                .map(|argument| ty(facade, argument, profile, installed))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            if let Some(ty) = profile.semantic_type(facade, installed, identity, arguments.clone())? {
                return Ok(ty);
            }
            host::type_ref::Type::imported_boxed(
                host::type_def::import::Identity::new(*identity),
                arguments,
            )
        }
        facade::Type::FunctionItem { input, output } => {
            host::type_ref::Type::imported_function_item(
                ty(facade, input, profile, installed)?,
                ty(facade, output, profile, installed)?,
            )
        }
        facade::Type::Parameter(parameter) => host::type_ref::Type::parameter(parameter.as_ref())?,
        facade::Type::Option(element) => {
            host::type_ref::Type::option(ty(facade, element, profile, installed)?)
        }
        facade::Type::Result(ok, err) => {
            host::type_ref::Type::result(ty(facade, ok, profile, installed)?, ty(facade, err, profile, installed)?)
        }
    })
}

fn collect_modules(path: &str, types: &[&str], modules: &mut Vec<Box<str>>) -> Result<(), Error> {
    for (separator, _) in path.match_indices("::") {
        let candidate = &path[..separator];
        if types.contains(&candidate) {
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
    if types.contains(&path)
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

/// Invalid generated facade plan or compiler registration.
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
    /// Host facade validation failed.
    #[error(transparent)]
    Host(#[from] host::Error),
    /// One exact reconstructed entry failed before the facade could publish.
    #[error("generated facade entry `{root}::{path}` is invalid: {source}")]
    FacadeEntry {
        /// Imported facade root.
        root: Box<str>,
        /// Relative imported type or item path.
        path: Box<str>,
        /// Exact nested reconstruction failure.
        #[source]
        source: Box<Error>,
    },
    /// The plan contains contradictory nominal shape evidence.
    #[error("generated imported facade has no nominal shape")]
    Shape,
    /// Bounded runner facade allocation failed.
    #[error("cannot allocate generated imported facade")]
    Allocation,
    /// Profile-owned cross-facade projections contain an unresolved dependency cycle.
    #[error("generated profile facade dependencies cannot be ordered")]
    ProfileOrder,
    /// A whole-facade compatibility seed was stale, incomplete, or contradictory.
    #[error("generated compatibility facade seed is not one exact atomic composition")]
    CompatibilitySeed,
    /// A private event producer is absent from the generated manifest.
    #[error("generated event producer is absent from its manifest")]
    EventManifest,
    /// A private event binding names an absent facade.
    #[error("generated event facade is absent")]
    EventFacade,
    /// A private event row disagrees with its source or target contract.
    #[error("generated event source or target selector disagrees")]
    EventSelector,
    /// More than one producer claims the same private event binding.
    #[error("generated event conversion is duplicated")]
    EventDuplicate,
    /// A structural row names no exact installed facade.
    #[error("generated structural adapter names no exact installed facade")]
    StructuralFacade,
    /// A structural selector is absent, ambiguous, or has the wrong operation kind.
    #[error("generated structural adapter does not match one exact facade selector")]
    StructuralSelector,
    /// A structural row is absent from the authenticated generated manifest.
    #[error("generated structural adapter is absent from the authenticated manifest")]
    StructuralManifest,
    /// A generated structural adapter is duplicated.
    #[error("generated structural adapter is duplicated")]
    StructuralDuplicate,
}
