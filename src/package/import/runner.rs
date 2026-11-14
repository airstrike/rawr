//! Generated-runner reconstruction of a program's generated bindings.

pub mod account;
#[cfg(test)]
mod tests;

use crate::{host, receiver, ImmutableString};
use rawr_import::surface::facade::{self, ItemKind};

/// One generated runtime's metadata binding scope in its engine's native account.
///
/// Authentication borrows the already-decoded carrier. The scope moves to the
/// completed runtime registry, which keeps the decoded metadata alive.
#[doc(hidden)]
pub struct Runtime<'engine> {
    native: &'engine account::Native,
    structural: Option<Box<[host::runtime::StructuralAdapter]>>,
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
        Ok(Runtime {
            native,
            structural: None,
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
        let adapters = authenticate_structural(
            definitions,
            self.native.carrier(),
            rows,
            &mut self.allocation,
        )
        .map_err(runtime_error)?;
        self.structural = Some(adapters);
        Ok(())
    }

    /// Borrow this scope's authenticated structural adapters for runtime binding.
    #[must_use]
    pub fn structural(&self) -> &[host::runtime::StructuralAdapter] {
        self.structural.as_deref().unwrap_or_default()
    }

    /// Move this scope to the registry once its rows are authenticated.
    pub fn finish(
        self,
        registry: host::runtime::Registry,
    ) -> Result<host::runtime::Registry, host::runtime::Error> {
        if self.structural.is_none() {
            return Err(host::runtime::Error::profile(
                "generated runtime authentication is incomplete",
            ));
        }
        registry.retain_imported(self.allocation.finish())
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
    /// A variant's test, which shares the variant's selector.
    Test,
    /// A variant's fields moved out, which shares the variant's selector.
    Take,
    /// A variant's fields lent, which shares the variant's selector.
    Lend,
    /// A variant's fields lent mutably, which shares the variant's selector.
    LendMut,
    /// One of a variant's fields copied, which shares the variant's
    /// selector.
    Read {
        field: u32,
    },
    /// One of a variant's fields lent, which shares the variant's
    /// selector.
    Borrow {
        field: u32,
    },
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
    if let Some(path) = Definition::std_path(identity) {
        let std = crate::std_facade::definition().export_type(path)?;
        return host::type_ref::Type::external(std, arguments).map(Some);
    }
    // Rust's `Box<T>`: rawr's `Box` (D711 step 5).
    if identity == Definition::boxed().as_bytes() && arguments.len() == 1 {
        let element = arguments.into_vec().remove(0);
        return Ok(Some(host::type_ref::Type::boxed(element)));
    }
    // A thread's `JoinHandle<T>` a crate gives: rawr's (#1266).
    if identity == Definition::join_handle().as_bytes() && arguments.len() == 1 {
        let element = arguments.into_vec().remove(0);
        return Ok(Some(host::type_ref::Type::join_handle(element)));
    }
    // std's map or set: rawr's of its kind (#777).
    if let Some(kind) = Definition::keyed_kind(identity) {
        return Ok(Some(host::type_ref::Type::keyed(kind, arguments)));
    }
    // Rust's `Rc<T>` or `Arc<T>` of a crate's type: rawr's (#775).
    if let Some(arc) = Definition::counter(identity)
        && arguments.len() == 1
    {
        let element = arguments.into_vec().remove(0);
        return Ok(Some(host::type_ref::Type::counted(arc, element)));
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

/// Install the generated bindings embedded by one compiler-owned demand
/// brand: the runner's image, which each facade reads where it lies and
/// builds an entry of the first time a lookup reaches it (#1518).
pub fn install(
    engine: &mut crate::Engine,
    image: &'static [u8],
) -> Result<Box<[host::facade::Definition]>, Error> {
    let mut allocation = account::Attempt::fresh();
    install_image(engine, image, &[], &mut allocation)
}

/// Install generated bindings under their retained native account.
/// `path_crates` are the crates of the project's path packages, by their
/// crate names: a save to the project may change their facades, which
/// each source's lowering then reads entry by entry (#1735).
#[doc(hidden)]
pub fn install_retained(
    engine: &mut crate::Engine,
    image: &'static [u8],
    path_crates: &[&str],
    native: &mut account::Native,
) -> Result<Box<[host::facade::Definition]>, Error> {
    install_image(engine, image, path_crates, &mut native.allocation())
}

fn install_image(
    engine: &mut crate::Engine,
    image: &'static [u8],
    path_crates: &[&str],
    allocation: &mut account::Attempt,
) -> Result<Box<[host::facade::Definition]>, Error> {
    let plan = host::image::Plan::read(image, path_crates)?;
    let mut definitions = Vec::new();
    // In build order: a runtime registry installs each facade after those
    // whose types it names, as cargo builds a dependency before its user.
    for held in host::image::Plan::held(&plan) {
        let root = held.root().to_string();
        let type_aliases = held
            .type_aliases()
            .map_err(|source| entry_error(&root, "", source))?;
        // The image is the entries' storage: each is built once, from it.
        use host::ImportAllocation as _;
        allocation.retain_checked(Some(held.entries()), Some(image.len()))?;
        let definition = host::facade::Definition::from_held(held, type_aliases.into_boxed_slice());
        engine
            .register_host_facade(definition.clone())
            .map_err(|source| entry_error(&root, "", source.into()))?;
        definitions.push(definition);
    }
    Ok(definitions.into_boxed_slice())
}

/// Install a full plan's facades as a runner did before its image held
/// them, each built whole: what a held facade's lookups are tested against.
#[cfg(test)]
pub(crate) fn install_plan(
    engine: &mut crate::Engine,
    bytes: &[u8],
) -> Result<Box<[host::facade::Definition]>, Error> {
    let plan = facade::Plan::decode(bytes)?;
    install_decoded(engine, &plan, &mut account::Attempt::fresh())
}

#[cfg(test)]
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
    // A facade resolves a type another facade declares through that
    // facade, which is built first (`facade::Resolution`).
    let facades = plan.facades().collect::<Vec<_>>();
    let resolution = facade::Resolution::new(
        &facades
            .iter()
            .map(|facade| facade.types().map(|ty| *ty.identity()).collect())
            .collect::<Vec<_>>(),
        &facades
            .iter()
            .map(|facade| facade.named())
            .collect::<Vec<_>>(),
    );
    let mut built = vec![None; facades.len()];
    for &index in resolution.order() {
        let facade = facades[index];
        let foreign = resolution
            .through(index)
            .iter()
            .map(|(identity, provider)| {
                let identity = host::type_def::import::Identity::new(*identity);
                let export = built[*provider]
                    .as_ref()
                    .and_then(|provider: &host::facade::Definition| {
                        provider.export_imported(&identity)
                    })
                    .expect("bindings are built after those they resolve through");
                (identity, export)
            })
            .collect();
        let (definition, families) = build(facade, foreign, &mut shapes, allocation)?;
        retain_registry_definition(&definition, allocation)?;
        engine
            .register_host_facade(definition.clone())
            .map_err(|source| entry_error(facade.root(), "", source.into()))?;
        engine
            .host_registry
            .defer_imported(families)
            .map_err(|source| entry_error(facade.root(), "", source.into()))?;
        built[index] = Some(definition);
    }
    // In build order: a runtime registry installs each facade after those
    // whose types it names, as cargo builds a dependency before its user.
    definitions.extend(
        resolution
            .order()
            .iter()
            .filter_map(|&index| built[index].take()),
    );
    Ok(definitions.into_boxed_slice())
}

#[cfg(test)]
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
    let mut allocation = account::Attempt::fresh();
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
    super::admit_applications(&mut engine.host_registry, &applications).unwrap_or_else(|error| {
        panic!("each generated application admits into the runner's bindings: {error}")
    });
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
    let mut allocation = account::Attempt::fresh();
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
    if !carrier.contains_all(
        rows.iter()
            .map(|row| (&row.identity, &row.signature, row.arguments)),
    ) {
        panic!("each structural adapter is in the runner's manifest");
    }
    let facades = definitions
        .iter()
        .map(|definition| (definition.root(), definition))
        .collect::<std::collections::HashMap<_, _>>();
    for row in rows {
        let facade = *facades
            .get(row.root)
            .expect("each structural adapter names installed bindings");
        let (path, kind) = structural_path(facade, row);
        if kind != row.kind {
            panic!("each structural adapter's selector is of its kind");
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
                StructuralKind::Test => host::structural::Kind::Test,
                StructuralKind::Take => host::structural::Kind::Take,
                StructuralKind::Lend => host::structural::Kind::Lend,
                StructuralKind::LendMut => host::structural::Kind::LendMut,
                StructuralKind::Read { field } => host::structural::Kind::Read { field },
                StructuralKind::Borrow { field } => host::structural::Kind::Borrow { field },
                StructuralKind::Store => host::structural::Kind::Store,
            },
        )
        .with_instance(row.instance);
        adapters.push(host::runtime::StructuralAdapter::transform(definition));
    }
    let mut generated = std::collections::HashSet::with_capacity(adapters.len());
    if !adapters
        .iter()
        .all(|adapter: &host::runtime::StructuralAdapter| generated.insert(&adapter.definition))
    {
        panic!("each structural adapter is generated once");
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

/// Whether reading a field of type `ty` leaves its struct whole, as the
/// generated bridge's `shared_field` reads it: a Copy field, or one Rawr
/// holds as its own value.
fn shared_field(ty: &host::type_ref::Type, facade: &host::facade::Definition) -> bool {
    copy_scalar(ty, facade) || ty.plain_value()
}

/// Whether a field of type `ty` is read as a copy, as the generated bridge
/// reads it: a scalar, an imported Copy type of `facade`, or an `Option` or
/// `Result` of them.
fn copy_scalar(ty: &host::type_ref::Type, facade: &host::facade::Definition) -> bool {
    match ty.shape() {
        host::type_ref::Shape::Imported { identity, .. } => {
            let copy = |definition: &host::type_def::Definition| {
                definition
                    .capabilities()
                    .contains(crate::capability::Kind::Copy)
            };
            match facade.held() {
                Some(held) => held
                    .identity_types(identity)
                    .into_iter()
                    .any(|index| copy(held.type_at(index))),
                None => facade.types().any(|definition| {
                    definition.identity().imported() == Some(identity) && copy(definition)
                }),
            }
        }
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
        // A shared borrow, as iced's `LineDash::segments: &'a [f32]`, and a
        // `&dyn Trait`, as `Held(&'static dyn Shape)` holds (#1593).
        host::type_ref::Shape::Reference { mutable: false, .. } => true,
        #[cfg(feature = "rust-import")]
        host::type_ref::Shape::Object { pointer, .. } => !pointer.owns(),
        // An array of a length of Copy values, as `position: [f32; 2]`.
        host::type_ref::Shape::Array {
            element,
            sequence: host::type_ref::Sequence::Array(_),
        } => {
            !matches!(element.shape(), host::type_ref::Shape::Reference { .. })
                && copy_scalar(element, facade)
        }
        _ => false,
    }
}

fn structural_path<'facade>(
    facade: &'facade host::facade::Definition,
    row: &StructuralRow,
) -> (&'facade host::path::Path, StructuralKind) {
    let mut found: Option<(
        &host::path::Path,
        StructuralKind,
        Option<&host::type_def::import::Identity>,
    )> = None;
    // An owner below a re-exported module is held below the module it
    // names.
    let held = host::path::Path::parse(rust_item(facade.root(), row.owner))
        .ok()
        .and_then(|path| facade.through_reexports(&path))
        .map(|path| path.to_string());
    let held = held
        .as_deref()
        .and_then(|path| path.strip_prefix(facade.root()))
        .and_then(|path| path.strip_prefix("::"))
        .unwrap_or(row.owner);
    let owned = |definition: &host::type_def::Definition| {
        held.is_empty() || {
            let path = definition.path();
            let path = path.as_str();
            path == held
                || path
                    .strip_suffix(held)
                    .is_some_and(|prefix| prefix.ends_with("::"))
        }
    };
    // Only a type whose constructor, field or variant has the row's
    // selector is a candidate: a held facade finds those by its index.
    let candidates = match facade.held() {
        Some(held) => held.structural(&row.selector),
        None => facade.types().collect(),
    };
    for definition in candidates
        .into_iter()
        .filter(|definition| owned(definition))
    {
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
                    // A field's selector names its reader, its store and
                    // its lenders.
                    (field.imported_identity().fingerprint() == row.selector).then_some(
                        if matches!(
                            row.kind,
                            StructuralKind::Store
                                | StructuralKind::Borrow { .. }
                                | StructuralKind::LendMut
                        ) {
                            row.kind
                        } else if row.instance.is_some() && !definition.parameters().is_empty() {
                            // The authenticated adapter's signature is at
                            // the selected instance, not the type's defaults.
                            match row.kind {
                                StructuralKind::Field { copy } => StructuralKind::Field { copy },
                                _ => return None,
                            }
                        } else {
                            StructuralKind::Field {
                                copy: shared_field(defaulted(field.ty(), definition), facade),
                            }
                        },
                    )
                }))
                .next(),
            // A variant's selector names its constructor and its matchers.
            host::type_def::Shape::Enum { variants, .. } => variants.iter().find_map(|variant| {
                (variant.imported_identity().fingerprint() == row.selector).then_some(
                    match row.kind {
                        StructuralKind::Match
                        | StructuralKind::Test
                        | StructuralKind::Take
                        | StructuralKind::Lend
                        | StructuralKind::LendMut
                        | StructuralKind::Read { .. }
                        | StructuralKind::Borrow { .. } => row.kind,
                        _ => StructuralKind::Variant,
                    },
                )
            }),
            host::type_def::Shape::Opaque | host::type_def::Shape::Trait(_) => None,
        };
        if let Some(kind) = candidate {
            let identity = definition.identity().imported();
            match found {
                // One type at two paths, as ratatui re-exports
                // `ratatui_core::layout::Rect`: the first path stands, but
                // for one through a crate's hidden segment, which a public
                // path replaces, as values of the type are attested at it
                // (`iced::__rawr_crate_iced_widget_<id>::core::border::
                // Radius` is `iced::border::Radius`, #953).
                Some((path, _, first)) if first.is_some() && first == identity => {
                    let hidden = |path: &host::path::Path| {
                        path.segments()
                            .iter()
                            .any(|segment| host::path::is_hidden(segment))
                    };
                    if hidden(path) && !hidden(definition.path_ref()) {
                        found = Some((definition.path_ref(), kind, identity));
                    }
                    continue;
                }
                // Two types whose paths end in the owner's, as `iced::Point`
                // and tiny_skia's `Point` reached through its hidden
                // segment: the one at the owner's own path below the root
                // is the one the row names (#954).
                Some((first_path, _, _)) => {
                    let named = |path: &host::path::Path| {
                        path.to_string() == format!("{}::{held}", row.root)
                    };
                    match (named(first_path), named(definition.path_ref())) {
                        (true, false) => {}
                        (false, true) => found = Some((definition.path_ref(), kind, identity)),
                        _ => panic!("one structural adapter selector names one item"),
                    }
                }
                None => found = Some((definition.path_ref(), kind, identity)),
            }
        }
    }
    found
        .map(|(path, kind, _)| (path, kind))
        .expect("each structural adapter's selector names an item")
}

#[cfg(test)]
fn build(
    facade: &facade::Facade,
    foreign: Box<[(host::type_def::import::Identity, host::type_def::Export)]>,
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
                let shape = shape(ty.shape(), ty.layout())
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
            .imported_opaque_type(
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
                    ty.partial_ord(),
                    ty.ord(),
                    ty.iterator(),
                    ty.error(),
                ]),
                parameters,
                defaults,
                ty.associated()
                    .map(|(identity, name, position)| {
                        (identity, ImmutableString::from(name), position as usize)
                    })
                    .collect(),
                shape,
                ty.dropped() as usize,
                allocation,
            )
            .map_err(|source| entry_error(facade.root(), ty.path(), source.into()))?;
    }
    // Each re-export of a crate's item is the item's body at another path:
    // its signatures are converted once, for the first of its paths.
    let mut signatures =
        std::collections::HashMap::<facade::Body<'_>, Vec<host::signature::Definition>>::new();
    for item in facade.items() {
        let converted = match signatures.entry(item.body()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                std::iter::once(item.signature())
                    .chain(item.values())
                    .map(|form| signature(facade.root(), form))
                    .collect::<Result<_, _>>()
                    .map_err(|source| entry_error(facade.root(), item.path(), source))?,
            ),
        };
        // The item, and its form that a value fills, when it has one.
        let forms = std::iter::once(item.signature()).chain(item.values());
        for (form, signature) in forms.zip(converted.iter()) {
            let definition = item_definition(item, form, signature.clone())
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
        let Some(header) = impl_header(header) else {
            continue;
        };
        builder
            .impl_header(header)
            .map_err(|source| entry_error(facade.root(), "<impl>", source.into()))?;
    }
    // A re-exported variant, as `pub use Alignment::Center;`, names its
    // enum's variant.
    for (path, variant) in facade.variants() {
        builder
            .variant_alias(path, variant)
            .map_err(|source| entry_error(facade.root(), path, source.into()))?;
    }
    // A trait or a module the program names is a module (`use rand::Rng;`,
    // `use crate::icon;`), unless the bindings already declare something at
    // its path (iced's `Widget`, whose type the bindings carry).
    for path in facade.traits() {
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes.chain(std::iter::once(path)) {
            match builder.imported_module(module, allocation) {
                Ok(_)
                | Err(host::Error::DuplicateFacadeEntry {
                    existing: "module", ..
                }) => {}
                // A prefix that is a type or a re-export, as bytemuck's
                // `pub use ::core as __core;`, holds no modules of the
                // crate's: what lies under it is reached through it.
                Err(host::Error::DuplicateFacadeEntry { .. }) => break,
                Err(error) => return Err(entry_error(facade.root(), module, error.into())),
            }
        }
    }
    // A module the facade reaches at several paths is held at one: what
    // lies below each other path is what lies below that one.
    for (path, target) in facade.reexports() {
        builder
            .imported_reexport(path, target, allocation)
            .map_err(|source| entry_error(facade.root(), path, source.into()))?;
    }
    // A dependency's `macro_rules!` macros, which Rawr expands, each in its
    // module: `$crate` reads as the defining crate's root through this root.
    for rules in facade.macros() {
        let path = rules.path();
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes {
            match builder.imported_module(module, allocation) {
                Ok(_)
                | Err(host::Error::DuplicateFacadeEntry {
                    existing: "module", ..
                }) => {}
                // A prefix that is a type or a re-export, as bytemuck's
                // `pub use ::core as __core;`, holds no modules of the
                // crate's: what lies under it is reached through it.
                Err(host::Error::DuplicateFacadeEntry { .. }) => break,
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
    // A dependency's derive macros, which a program's `#[derive(..)]`
    // names, each in its module.
    for path in facade.derives() {
        let prefixes = path.match_indices("::").map(|(end, _)| &path[..end]);
        for module in prefixes {
            match builder.imported_module(module, allocation) {
                Ok(_)
                | Err(host::Error::DuplicateFacadeEntry {
                    existing: "module", ..
                }) => {}
                // A prefix that is a type or a re-export, as bytemuck's
                // `pub use ::core as __core;`, holds no modules of the
                // crate's: what lies under it is reached through it.
                Err(host::Error::DuplicateFacadeEntry { .. }) => break,
                Err(error) => return Err(entry_error(facade.root(), module, error.into())),
            }
        }
        builder
            .derive(path)
            .map_err(|source| entry_error(facade.root(), path, source.into()))?;
    }
    builder
        .finish_imported(foreign, allocation)
        .map_err(|source| entry_error(facade.root(), "", source.into()))
}

#[cfg(test)]
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
        host::type_def::Shape::Enum { variants, .. } => {
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
        host::type_def::Shape::Enum { variants, .. } => {
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

/// The impl `header` as a facade records it: `None` when its trait's
/// arguments or self type are types Rawr can't install. A bound or
/// projection that names one is left out.
pub(crate) fn impl_header(header: &facade::ImplHeader) -> Option<host::facade::ImplHeader> {
    let types = |types: &[facade::Type]| {
        types
            .iter()
            .map(self::ty)
            .collect::<Result<Box<[_]>, _>>()
            .ok()
    };
    let arguments = types(header.arguments())?;
    let self_type = self::ty(header.self_type()).ok()?;
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
    let projections = header
        .projections()
        .iter()
        .filter_map(|projection| {
            Some(host::facade::ImplProjection {
                ty: self::ty(projection.ty()).ok()?,
                value: self::ty(projection.value()).ok()?,
                cases: projection
                    .cases()
                    .iter()
                    .map(|(this, value)| Some((self::ty(this).ok()?, self::ty(value).ok()?)))
                    .collect::<Option<_>>()?,
            })
        })
        .collect();
    Some(host::facade::ImplHeader {
        parameters: header.parameters().map(ImmutableString::from).collect(),
        trait_identity: *header.trait_identity(),
        arguments,
        self_type,
        bounds,
        projections,
    })
}

/// The crate type `ty` at `path`, qualified from its root, with `shape`,
/// which every type of its body shares.
pub(crate) fn imported_type(
    path: host::path::Path,
    ty: &facade::TypeDefinition,
    shape: crate::Shared<host::type_def::Shape>,
) -> Result<host::type_def::Definition, Error> {
    let parameters = ty.parameters().map(ImmutableString::from).collect();
    let defaults = ty
        .defaults()
        .iter()
        .map(|default| default.as_ref().map(self::ty).transpose())
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    let implemented = crate::package::import::implemented([
        ty.copy(),
        ty.default(),
        ty.debug(),
        ty.cloneable(),
        ty.partial_eq(),
        ty.eq(),
        ty.hash(),
        ty.partial_ord(),
        ty.ord(),
        ty.iterator(),
        ty.error(),
    ]);
    let associated = ty
        .associated()
        .map(|(identity, name, position)| {
            (identity, ImmutableString::from(name), position as usize)
        })
        .collect();
    Ok(host::type_def::Definition::imported(
        path,
        host::type_def::import::Identity::new(*ty.identity()),
        &implemented,
        parameters,
        defaults,
        shape,
    )?
    .with_associated(associated)
    .with_dropped(ty.dropped() as usize))
}

/// rustc's layout of a struct, as the importer gives it.
pub(crate) fn layout(
    layout: &rawr_import::api::model::Layout,
) -> Box<host::type_def::layout::Layout> {
    Box::new(host::type_def::layout::Layout::new(
        layout.size(),
        layout.pointers().into(),
        layout.varied(),
    ))
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

/// The host shape of `shape`, a struct's with rustc's `layout` of it.
pub(crate) fn shape(
    shape: &facade::Shape,
    layout: Option<&rawr_import::api::model::Layout>,
) -> Result<host::type_def::Shape, Error> {
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
            unsupported: shape.unsupported().map(|unsupported| {
                Box::new(host::type_def::Unsupported {
                    field: unsupported.field().into(),
                    ty: unsupported.ty().into(),
                })
            }),
            layout: layout.map(self::layout),
        });
    }
    let variants = shape
        .variants()
        .expect("a generated type that is not a struct is an enum");
    Ok(host::type_def::Shape::Enum {
        variants: variants
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
                )
                .with_unsupported(variant.unsupported().map(|unsupported| {
                    host::type_def::Unsupported {
                        field: unsupported.field().into(),
                        ty: unsupported.ty().into(),
                    }
                })))
            })
            .collect::<Result<Vec<_>, Error>>()?
            .into_boxed_slice(),
        non_exhaustive: shape.non_exhaustive(),
    })
}

fn field(field: &facade::Field) -> Result<host::type_def::Field, Error> {
    Ok(host::type_def::Field::new(
        field.name().into(),
        ty(field.ty())?,
        field.selector().try_clone()?,
    ))
}

pub(crate) fn item_definition(
    item: &facade::Item,
    form: &facade::Signature,
    signature: host::signature::Definition,
) -> Result<host::item::Definition, Error> {
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
        .instancing(item.member().map(|member| *member.as_bytes()))
        .guarded(guard)
        .opening(form.scope()))
}

pub(crate) fn signature(
    root: &str,
    signature: &facade::Signature,
) -> Result<host::signature::Definition, Error> {
    let mut builder = host::signature::Builder::new();
    for (index, parameter) in signature.parameters().enumerate() {
        builder.type_parameter(parameter, &super::requirements(signature.bounds(index)))?;
        if signature.is_length(parameter) {
            builder.length(parameter.into())?;
        }
    }
    for (parameter, shown) in signature.shown() {
        builder.shown(parameter.into(), shown.into())?;
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
            relation.cases().iter().map(|case| {
                (
                    case.this(),
                    case.arguments(),
                    case.projections()
                        .iter()
                        .map(|projection| {
                            (
                                projection.ty(),
                                projection.value(),
                                projection
                                    .cases()
                                    .iter()
                                    .map(|(this, at)| (this, at))
                                    .collect(),
                            )
                        })
                        .collect(),
                )
            }),
            relation.trait_identity(),
            relation.associated(),
            relation.std_trait(),
            |source| {
                pattern(source, &|nominal| {
                    ty(nominal).ok().and_then(|ty| rooted(root, ty))
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
        | facade::Type::Slice(element)
        | facade::Type::FixedArray { element, .. }
        | facade::Type::ConstArray { element, .. }
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
        // A `&str` an implementation is for, as the program's string slice
        // is (#1678).
        facade::Type::Reference {
            mutable: false,
            target,
        } if matches!(**target, facade::Type::String) => Pattern::Type(ty(source).ok()?),
        facade::Type::Tuple(members) => Pattern::Tuple(
            members
                .iter()
                .map(|member| pattern(member, nominal))
                .collect::<Option<_>>()?,
        ),
        facade::Type::Nominal { .. }
        | facade::Type::FixedArray { .. }
        | facade::Type::Array(_)
        | facade::Type::Slice(_)
            if !names_variable(source) =>
        {
            Pattern::Type(nominal(source)?)
        }
        // A crate type that names the impl's own parameters, as `Text<'a,
        // Theme, Renderer>` of `From<Text<..>> for Element<..>`: each is
        // named as the case's variable.
        facade::Type::Nominal { .. }
        | facade::Type::FixedArray { .. }
        | facade::Type::Array(_)
        | facade::Type::Slice(_) => Pattern::Generic(nominal(source)?),
        _ => return None,
    })
}

pub(crate) fn ty(source: &facade::Type) -> Result<host::type_ref::Type, Error> {
    Ok(match source {
        facade::Type::Unit => host::type_ref::Type::unit(),
        facade::Type::Never => host::type_ref::Type::builtin(host::type_ref::Builtin::Never),
        facade::Type::Bool => host::type_ref::Type::builtin(host::type_ref::Builtin::Bool),
        facade::Type::Char => host::type_ref::Type::builtin(host::type_ref::Builtin::Char),
        facade::Type::F64 => host::type_ref::Type::builtin(host::type_ref::Builtin::F64),
        facade::Type::I64 => host::type_ref::Type::builtin(host::type_ref::Builtin::I64),
        facade::Type::InclusiveRange => {
            host::type_ref::Type::builtin(host::type_ref::Builtin::InclusiveRange)
        }
        facade::Type::String => host::type_ref::Type::builtin(host::type_ref::Builtin::String),
        facade::Type::Text => host::type_ref::Type::builtin(host::type_ref::Builtin::Text),
        facade::Type::Arguments => {
            host::type_ref::Type::builtin(host::type_ref::Builtin::Arguments)
        }
        facade::Type::Primitive(name) => host::type_ref::Type::builtin(
            host::type_ref::Builtin::primitive(name)
                .expect("a generated primitive type is unknown"),
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
            // A program's iterator a crate takes or holds, as itertools'
            // `Product<Self, J::IntoIter>` does: an iterator of its item.
            if identity == rawr_import::surface::identity::Definition::iterator().as_bytes()
                && let [item] = &*arguments
            {
                return Ok(host::type_ref::Type::iterator(item.clone()));
            }
            // A script closure a crate value holds, as iced's `Map<W, F, A>`
            // from `Widget::map` does: the closure's type (T417).
            if identity == rawr_import::surface::identity::Definition::callable().as_bytes() {
                let (output, inputs) = arguments
                    .split_last()
                    .expect("a callable type names its output");
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
        // Only a relation's case names an impl's own parameter, as the
        // parameter its variable is named in a `Pattern::Generic`.
        facade::Type::Variable(index) => {
            host::type_ref::Type::parameter(host::signature::case_variable(*index))?
        }
        facade::Type::Result(ok, err) => host::type_ref::Type::result(ty(ok)?, ty(err)?),
        facade::Type::Array(element) => host::type_ref::Type::vec(ty(element)?),
        facade::Type::Slice(element) => host::type_ref::Type::slice(ty(element)?),
        facade::Type::FixedArray { element, length } => host::type_ref::Type::array(
            ty(element)?,
            usize::try_from(*length).expect("an array length fits in usize"),
        ),
        facade::Type::ConstArray { element, length } => {
            host::type_ref::Type::array_of_length(ty(element)?, length.as_ref().into())
        }
        facade::Type::Tuple(members) => {
            host::type_ref::Type::tuple(members.iter().map(ty).collect::<Result<Vec<_>, _>>()?)?
        }
        facade::Type::Object {
            pointer,
            identity,
            arguments,
        } => host::type_ref::Type::object(
            *pointer,
            *identity,
            arguments.iter().map(ty).collect::<Result<_, _>>()?,
        ),
    })
}

/// The modules above `path`, up to the first type. `types` is sorted.
#[cfg(test)]
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
        "internal error: rawr cannot register the bindings of `{}`: {source}. This is a rawr bug",
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
    /// Bounded allocation of the bindings failed.
    #[error("out of memory while loading the program's bindings")]
    Allocation,
}
