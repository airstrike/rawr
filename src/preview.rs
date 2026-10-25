//! Compiler-validated local preview metadata.

mod bound;
mod catalog;
pub mod digest;
mod error;
mod launch;
mod update;

pub use bound::BoundCatalog;
pub use catalog::Catalog;
pub use digest::Digest;
pub use error::{Error, RuntimeError};
pub use launch::Launch;
pub use update::Update;

use crate::{application, capability, host, item, receiver, resolve, signature, type_def};
use crate::{ImmutableString, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub(crate) const DEFAULT_WIDTH: u32 = 1024;
pub(crate) const DEFAULT_HEIGHT: u32 = 768;

/// Stable identity of one named preview variant.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Key {
    subject: item::Key,
    variant: ImmutableString,
}

/// Logical preview canvas size, independent of native window scale.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogicalSize {
    width: u32,
    height: u32,
}

/// How a preview subject receives its fixture value.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Input {
    /// A zero-argument subject.
    Stateless,
    /// A function or method observes the fixture by shared reference.
    Shared(application::Type),
    /// A function consumes a compiler-proven clone of the fixture.
    Owned(application::Type),
}

/// One exact compiler-selected preview callback.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Callback {
    item: item::Value,
    receiver: Option<receiver::Mode>,
    parameters: Box<[application::Type]>,
    output: application::Type,
}

/// The compiler-proven fixture construction strategy.
// Boxing `Callback` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Seed {
    /// No fixture exists for a stateless preview.
    None,
    /// Construct the exact input through its proven `Default` capability.
    Default,
    /// Invoke an exact zero-argument function returning the input.
    Callback(Callback),
}

/// One explicit theme choice for a preview.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Theme {
    name: ImmutableString,
    callback: Callback,
}

/// One opaque source-qualified preview location.
#[derive(Clone, Copy)]
pub struct Location {
    source: crate::program::source::Key,
    span: Span,
}

/// Source reference and exact declaration for one selected callback.
#[derive(Clone, Copy, Debug)]
pub struct CallbackLocation {
    reference: Location,
    declaration: Location,
}

/// Exact positioned evidence retained beside one preview proof.
#[derive(Clone, Debug)]
pub struct Locations {
    attribute: Location,
    subject: Location,
    input: Option<Location>,
    message: Location,
    seed: Option<CallbackLocation>,
    update: Option<CallbackLocation>,
    themes: Box<[CallbackLocation]>,
}

/// One opaque compiler-validated local preview definition.
#[derive(Clone, Debug)]
pub struct Definition {
    key: Key,
    name: ImmutableString,
    group: ImmutableString,
    subject: Callback,
    input: Input,
    message: application::Type,
    seed: Seed,
    update: Option<Callback>,
    themes: Box<[Theme]>,
    snapshot: bool,
    size: LogicalSize,
    runtime_output: host::runtime::type_ref::Type,
    projection: Option<host::projection::Site>,
    locations: Locations,
}

/// Deterministic exact proof of every canonical preview definition.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint(Box<[Proof]>);

#[derive(Clone, Debug)]
pub(crate) enum PathTarget {
    Item(item::Key),
    Associated {
        owner: type_def::Identity,
        member: ImmutableString,
    },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Proof {
    key: Key,
    name: ImmutableString,
    group: ImmutableString,
    subject: Callback,
    input: Input,
    message: application::Type,
    seed: Seed,
    update: Option<Callback>,
    themes: Box<[Theme]>,
    snapshot: bool,
    size: LogicalSize,
    runtime_output: host::runtime::type_ref::Type,
    projection: Option<host::projection::Site>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Syntax {
    pub(crate) name: Option<Text>,
    pub(crate) group: Option<Text>,
    pub(crate) seed: Option<item::type_ref::Path>,
    pub(crate) update: Option<item::type_ref::Path>,
    pub(crate) themes: Box<[item::type_ref::Path]>,
    pub(crate) snapshot: Option<Flag>,
    pub(crate) size: Option<SizeSyntax>,
    /// `bind = (a, b)` until the parser makes it the `seed` (see `bound`).
    pub(crate) bind: Option<Bind>,
    /// The `seed` is a `bind`: the arguments of a free function.
    pub(crate) bound: bool,
    pub(crate) span: Span,
}

/// `bind = (a, b)`: the parenthesized arguments, span and tokens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Bind {
    pub(crate) span: Span,
    pub(crate) tokens: Vec<item::BodyToken>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Text {
    pub(crate) value: ImmutableString,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Flag {
    pub(crate) value: bool,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SizeSyntax {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) span: Span,
}

/// One `#[preview]` as written, read from parsed sources without checking
/// them, so a listing costs a parse (`rawr mcp`'s `list_previews`). The
/// name and group default as the checked [`Definition`] does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listed {
    /// Display name: the written `name`, else the function's.
    pub name: String,
    /// Written `group`, else the module's last segment, else the package.
    pub group: String,
    /// `package::module::Owner::function` of the previewed function.
    pub subject: String,
    /// Theme paths as written, in declaration order.
    pub themes: Vec<String>,
    /// Logical size, written or the default.
    pub size: (u32, u32),
    /// Whether `rawr snapshot` renders it.
    pub snapshot: bool,
}

/// The previews written on `items`, a parse of the module `modules` of
/// `package`.
pub(crate) fn listed(
    package: &str,
    modules: &[ImmutableString],
    items: &[item::Parsed],
) -> Vec<Listed> {
    let mut found = Vec::new();
    let mut push = |owner: Option<&str>, function: &item::Function| {
        for syntax in function
            .header
            .attributes
            .iter()
            .filter_map(item::Attribute::preview)
        {
            let subject = std::iter::once(package)
                .chain(modules.iter().map(ImmutableString::as_str))
                .chain(owner)
                .chain(std::iter::once(function.name.text.as_str()))
                .collect::<Vec<_>>()
                .join("::");
            let path = |path: &item::type_ref::Path| {
                path.segments
                    .iter()
                    .map(|segment| segment.name.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::")
            };
            found.push(Listed {
                name: syntax.name.as_ref().map_or_else(
                    || function.name.text.to_string(),
                    |name| name.value.to_string(),
                ),
                group: syntax.group.as_ref().map_or_else(
                    || {
                        modules
                            .last()
                            .map_or(package, ImmutableString::as_str)
                            .to_owned()
                    },
                    |group| group.value.to_string(),
                ),
                subject,
                themes: syntax.themes.iter().map(path).collect(),
                size: syntax.size.map_or((DEFAULT_WIDTH, DEFAULT_HEIGHT), |size| {
                    (size.width, size.height)
                }),
                snapshot: syntax.snapshot.is_none_or(|snapshot| snapshot.value),
            });
        }
    };
    for item in items {
        match item {
            item::Parsed::Function(function) => push(None, function),
            item::Parsed::Impl(implementation) => {
                let owner = match &implementation.owner {
                    item::type_ref::Type::Path(path) => path
                        .segments
                        .last()
                        .map(|segment| segment.name.text.as_str()),
                    _ => None,
                };
                for function in &implementation.functions {
                    push(owner, function);
                }
            }
            _ => {}
        }
    }
    found
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", self.subject, self.variant)
    }
}

impl Key {
    /// Return the complete stable subject item key.
    #[must_use]
    pub const fn subject(&self) -> &item::Key {
        &self.subject
    }

    /// Return the normalized preview variant name.
    #[must_use]
    pub fn variant(&self) -> &str {
        self.variant.as_str()
    }
}

impl LogicalSize {
    /// Return the logical width in pixels.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Return the logical height in pixels.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

impl Callback {
    #[cfg(feature = "rust-import")]
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        let mut completed = self.item.normalize_profile(profile);
        for parameter in &mut self.parameters {
            completed = completed.saturating_add(parameter.normalize_profile(profile));
        }
        completed.saturating_add(self.output.normalize_profile(profile))
    }

    /// Return the stable callback item key.
    #[must_use]
    pub const fn item(&self) -> &item::Value {
        &self.item
    }

    /// Return the callback receiver mode.
    #[must_use]
    pub const fn receiver(&self) -> Option<receiver::Mode> {
        self.receiver
    }

    /// Return callback parameter types, excluding the receiver.
    #[must_use]
    pub const fn parameters(&self) -> &[application::Type] {
        &self.parameters
    }

    /// Return the exact callback output type.
    #[must_use]
    pub const fn output(&self) -> &application::Type {
        &self.output
    }
}

impl Theme {
    #[cfg(feature = "rust-import")]
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        self.callback.normalize_profile(profile)
    }

    /// Return the stable theme choice label.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Return the exact theme callback.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }
}

#[cfg(feature = "rust-import")]
impl Input {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        match self {
            Self::Stateless => 0,
            Self::Shared(ty) | Self::Owned(ty) => ty.normalize_profile(profile),
        }
    }
}

#[cfg(feature = "rust-import")]
impl Seed {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        match self {
            Self::None | Self::Default => 0,
            Self::Callback(callback) => callback.normalize_profile(profile),
        }
    }
}

#[cfg(feature = "rust-import")]
impl Fingerprint {
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        self.0.iter_mut().fold(0_usize, |completed, proof| {
            completed.saturating_add(proof.normalize_profile(profile))
        })
    }
}

#[cfg(feature = "rust-import")]
impl Proof {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        let mut completed = self.subject.normalize_profile(profile);
        completed = completed.saturating_add(self.input.normalize_profile(profile));
        completed = completed.saturating_add(self.message.normalize_profile(profile));
        completed = completed.saturating_add(self.seed.normalize_profile(profile));
        if let Some(update) = &mut self.update {
            completed = completed.saturating_add(update.normalize_profile(profile));
        }
        for theme in &mut self.themes {
            completed = completed.saturating_add(theme.normalize_profile(profile));
        }
        completed = completed.saturating_add(self.runtime_output.normalize_profile(profile));
        if let Some(projection) = &mut self.projection {
            completed = completed.saturating_add(projection.normalize_profile(profile));
        }
        completed
    }
}

impl Location {
    /// Return the exact span within this location's retained program source.
    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }

    /// Return whether two locations belong to the same retained source unit.
    #[must_use]
    pub fn same_source(self, other: Self) -> bool {
        self.source == other.source
    }

    pub(crate) const fn source(self) -> crate::program::source::Key {
        self.source
    }
}

impl fmt::Debug for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Location")
            .field("source", &"<private>")
            .field("span", &self.span)
            .finish()
    }
}

impl CallbackLocation {
    /// Return the attribute reference that selected this callback.
    #[must_use]
    pub const fn reference(self) -> Location {
        self.reference
    }

    /// Return the selected callback's exact declaration location.
    #[must_use]
    pub const fn declaration(self) -> Location {
        self.declaration
    }
}

impl Locations {
    /// Return the complete preview attribute location.
    #[must_use]
    pub const fn attribute(&self) -> Location {
        self.attribute
    }

    /// Return the subject callback declaration location.
    #[must_use]
    pub const fn subject(&self) -> Location {
        self.subject
    }

    /// Return the receiver or state/input parameter location, when present.
    #[must_use]
    pub const fn input(&self) -> Option<Location> {
        self.input
    }

    /// Return the subject output location that proved `Message`.
    #[must_use]
    pub const fn message(&self) -> Location {
        self.message
    }

    /// Return the explicit seed reference and declaration, when present.
    #[must_use]
    pub const fn seed(&self) -> Option<CallbackLocation> {
        self.seed
    }

    /// Return the resolved update reference and declaration, when present.
    #[must_use]
    pub const fn update(&self) -> Option<CallbackLocation> {
        self.update
    }

    /// Return exact references and declarations for explicit theme choices.
    #[must_use]
    pub const fn themes(&self) -> &[CallbackLocation] {
        &self.themes
    }
}

impl Definition {
    /// Return this preview's stable identity.
    #[must_use]
    pub const fn key(&self) -> &Key {
        &self.key
    }

    /// Return the human-facing preview name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Return the human-facing preview group.
    #[must_use]
    pub fn group(&self) -> &str {
        self.group.as_str()
    }

    /// Return the exact preview subject callback.
    #[must_use]
    pub const fn subject(&self) -> &Callback {
        &self.subject
    }

    /// Return how the subject receives its optional fixture.
    #[must_use]
    pub const fn input(&self) -> &Input {
        &self.input
    }

    /// Return the exact message type projected from the subject output.
    #[must_use]
    pub const fn message(&self) -> &application::Type {
        &self.message
    }

    /// Return the exact fixture construction proof.
    #[must_use]
    pub const fn seed(&self) -> &Seed {
        &self.seed
    }

    /// Return the optional exact update callback.
    #[must_use]
    pub const fn update(&self) -> Option<&Callback> {
        self.update.as_ref()
    }

    /// Return explicit theme choices. An empty slice means inherit the app theme.
    #[must_use]
    pub const fn themes(&self) -> &[Theme] {
        &self.themes
    }

    /// Return whether snapshot commands include this preview.
    #[must_use]
    pub const fn snapshot(&self) -> bool {
        self.snapshot
    }

    /// Return this preview's logical canvas size.
    #[must_use]
    pub const fn size(&self) -> LogicalSize {
        self.size
    }

    /// Return all source-qualified positioned evidence for this definition.
    #[must_use]
    pub const fn locations(&self) -> &Locations {
        &self.locations
    }

    /// Return versioned canonical digest evidence for this exact private proof.
    ///
    /// This value is evidence for manifests and diagnostics only. Runtime
    /// authority continues to use the complete compiler-private proof bundle.
    #[must_use = "digest failures must be handled"]
    pub fn digest(&self) -> Result<Digest, digest::Error> {
        digest::of(&self.proof())
    }

    pub(crate) fn reload_fingerprint(&self) -> Fingerprint {
        Fingerprint(vec![self.proof()].into_boxed_slice())
    }

    pub(crate) fn schema_roots(&self) -> (BTreeSet<type_def::Identity>, bool) {
        let mut roots = BTreeSet::new();
        collect_callback_schema_roots(&self.subject, &mut roots);
        self.message.collect_schema_roots(&mut roots);
        let opaque = match &self.input {
            Input::Stateless => false,
            Input::Shared(input) | Input::Owned(input) => {
                input.collect_schema_roots(&mut roots);
                input.retained_root_is_opaque()
            }
        };
        if let Seed::Callback(seed) = &self.seed {
            collect_callback_schema_roots(seed, &mut roots);
        }
        if let Some(update) = &self.update {
            collect_callback_schema_roots(update, &mut roots);
        }
        for theme in &self.themes {
            collect_callback_schema_roots(theme.callback(), &mut roots);
        }
        (roots, opaque)
    }

    pub(crate) const fn runtime_output(&self) -> &host::runtime::type_ref::Type {
        &self.runtime_output
    }

    pub(crate) const fn projection_site(&self) -> Option<&host::projection::Site> {
        self.projection.as_ref()
    }

    pub(crate) fn boundary_types(&self) -> Result<Vec<host::runtime::type_ref::Type>, Error> {
        let mut types = vec![self.runtime_output.clone()];
        push_runtime_type(&mut types, &self.message, "Message")?;
        match &self.input {
            Input::Stateless => {}
            Input::Shared(input) | Input::Owned(input) => {
                push_runtime_type(&mut types, input, "input")?;
            }
        }
        for theme in &self.themes {
            push_runtime_type(&mut types, theme.callback.output(), "theme output")?;
        }
        if let Some(update) = &self.update {
            push_runtime_type(&mut types, update.output(), "update output")?;
        }
        types.sort();
        types.dedup();
        Ok(types)
    }

    fn proof(&self) -> Proof {
        Proof {
            key: self.key.clone(),
            name: self.name.clone(),
            group: self.group.clone(),
            subject: self.subject.clone(),
            input: self.input.clone(),
            message: self.message.clone(),
            seed: self.seed.clone(),
            update: self.update.clone(),
            themes: self.themes.clone(),
            snapshot: self.snapshot,
            size: self.size,
            runtime_output: self.runtime_output.clone(),
            projection: self.projection.clone(),
        }
    }
}

fn collect_callback_schema_roots(callback: &Callback, roots: &mut BTreeSet<type_def::Identity>) {
    for parameter in callback.parameters() {
        parameter.collect_schema_roots(roots);
    }
    callback.output().collect_schema_roots(roots);
}

fn push_runtime_type(
    types: &mut Vec<host::runtime::type_ref::Type>,
    ty: &application::Type,
    role: &'static str,
) -> Result<(), Error> {
    let runtime = ty.exact_runtime_type().ok_or(Error::RuntimeType { role })?;
    types.push(runtime);
    Ok(())
}

impl Fingerprint {
    pub(crate) fn from_definitions(definitions: &[Definition]) -> Self {
        Self(definitions.iter().map(Definition::proof).collect())
    }
}

impl Syntax {
    pub(crate) fn bare(span: Span) -> Self {
        Self {
            name: None,
            group: None,
            seed: None,
            update: None,
            themes: Box::new([]),
            snapshot: None,
            size: None,
            bind: None,
            bound: false,
            span,
        }
    }
}

#[derive(Clone)]
struct Candidate {
    source: crate::program::source::Id,
    key: item::Key,
    value: item::Value,
    public: bool,
    declaration: Span,
    signature: Signature,
}

#[derive(Clone)]
struct Signature {
    receiver: Option<receiver::Mode>,
    owner: Option<application::Type>,
    parameters: Box<[application::Type]>,
    output: application::Type,
    receiver_span: Option<Span>,
    parameter_spans: Box<[Span]>,
    output_span: Span,
}

struct Subject<'a> {
    candidate: Candidate,
    syntaxes: Vec<&'a Syntax>,
}

struct Context<'a> {
    program: &'a crate::resolve::Program,
    candidates: BTreeMap<item::Key, Candidate>,
    widget: host::type_def::Contract,
    task: host::type_def::Contract,
    theme: host::type_def::Contract,
}

pub(crate) fn resolve(
    program: &crate::resolve::Program,
    registry: &host::Registry,
) -> Result<Box<[Definition]>, crate::resolve::LocatedError> {
    let subjects = collect_subjects(program)?;
    if subjects.is_empty() {
        return Ok(Box::new([]));
    }
    let widget = registry
        .type_("iced::Element")
        .filter(|definition| definition.parameters().len() == 1)
        .map(host::type_def::Definition::contract)
        .ok_or_else(|| {
            invalid(
                subjects[0].candidate.source,
                None,
                subjects[0].candidate.key.to_string(),
                "installed iced facade does not expose `Element<Message>`",
                subjects[0].syntaxes[0].span,
                None,
            )
        })?;
    let task = registry
        .type_("iced::Task")
        .filter(|definition| definition.parameters().len() == 1)
        .map(host::type_def::Definition::contract)
        .ok_or_else(|| {
            invalid(
                subjects[0].candidate.source,
                None,
                subjects[0].candidate.key.to_string(),
                "installed iced facade does not expose `Task<Message>`",
                subjects[0].syntaxes[0].span,
                None,
            )
        })?;
    let theme = registry
        .type_("iced::Theme")
        .filter(|definition| definition.parameters().is_empty())
        .map(host::type_def::Definition::contract)
        .ok_or_else(|| {
            invalid(
                subjects[0].candidate.source,
                None,
                subjects[0].candidate.key.to_string(),
                "installed iced facade does not expose `Theme`",
                subjects[0].syntaxes[0].span,
                None,
            )
        })?;
    let runtime = application::RuntimeGraph::new(program);
    let candidates = collect_candidates(program, &runtime);
    let context = Context {
        program,
        candidates,
        widget,
        task,
        theme,
    };
    let mut definitions = Vec::new();
    let mut occupied = BTreeMap::<Key, (crate::program::source::Id, Span)>::new();

    for subject in subjects {
        for syntax in subject.syntaxes {
            let source = subject.candidate.source;
            let definition = context.normalize(&subject.candidate, syntax)?;
            if let Some((source, span)) =
                occupied.insert(definition.key.clone(), (source, syntax.span))
            {
                return Err(invalid(
                    subject.candidate.source,
                    Some(source),
                    definition.key.to_string(),
                    "duplicate normalized preview identity",
                    syntax.span,
                    Some(span),
                ));
            }
            definitions.push(definition);
        }
    }
    definitions.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(definitions.into_boxed_slice())
}

impl Context<'_> {
    fn normalize(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
    ) -> Result<Definition, crate::resolve::LocatedError> {
        let display_name = syntax
            .name
            .as_ref()
            .map_or_else(|| subject.key.name().into(), |name| name.value.clone());
        let variant = normalize_name(&display_name).ok_or_else(|| {
            invalid(
                subject.source,
                None,
                subject.key.to_string(),
                "preview `name` must contain an ASCII letter or digit",
                syntax.name.as_ref().map_or(syntax.span, |name| name.span),
                Some(subject.declaration),
            )
        })?;
        let group = syntax.group.as_ref().map_or_else(
            || {
                subject
                    .key
                    .modules()
                    .last()
                    .cloned()
                    .unwrap_or_else(|| subject.key.package().into())
            },
            |group| group.value.clone(),
        );
        if group.trim().is_empty() {
            return Err(invalid(
                subject.source,
                None,
                subject.key.to_string(),
                "preview `group` cannot be empty",
                syntax
                    .group
                    .as_ref()
                    .map_or(syntax.span, |group| group.span),
                Some(subject.declaration),
            ));
        }
        let (input, state) = self.subject_input(subject, syntax)?;
        let (message, runtime_output, projection) =
            self.subject_message(subject, syntax, &subject.signature.output)?;
        require_message_capabilities(subject, syntax, &message)?;
        let seed = self.seed(subject, syntax, state.as_ref())?;
        let update = self.update(subject, syntax, state.as_ref(), &message)?;
        let themes = self.themes(subject, syntax, state.as_ref())?;
        let locations = self.locations(subject, syntax, &seed, update.as_ref(), &themes);

        Ok(Definition {
            key: Key {
                subject: subject.key.clone(),
                variant,
            },
            name: display_name,
            group,
            subject: subject.callback(),
            input,
            message,
            seed,
            update,
            themes,
            snapshot: syntax.snapshot.is_none_or(|snapshot| snapshot.value),
            size: syntax.size.map_or(
                LogicalSize {
                    width: DEFAULT_WIDTH,
                    height: DEFAULT_HEIGHT,
                },
                |size| LogicalSize {
                    width: size.width,
                    height: size.height,
                },
            ),
            runtime_output,
            projection,
            locations,
        })
    }

    fn locations(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
        seed: &Seed,
        update: Option<&Callback>,
        themes: &[Theme],
    ) -> Locations {
        let here = |span| Location {
            source: self.program.source().key(subject.source),
            span,
        };
        let callback = |callback: &Callback, reference: Span| {
            let candidate = self
                .candidates
                .get(callback.item().key())
                .expect("normalized preview callbacks retain exact candidates");
            CallbackLocation {
                reference: here(reference),
                declaration: Location {
                    source: self.program.source().key(candidate.source),
                    span: candidate.declaration,
                },
            }
        };
        Locations {
            attribute: here(syntax.span),
            subject: here(subject.declaration),
            input: subject
                .signature
                .receiver_span
                .or_else(|| subject.signature.parameter_spans.first().copied())
                .map(here),
            message: here(subject.signature.output_span),
            seed: match (seed, syntax.seed.as_ref()) {
                (Seed::Callback(seed), Some(path)) => Some(callback(seed, path.span)),
                // A hand-written `Default::default`, taken without a `seed`.
                (Seed::Callback(seed), None) => Some(callback(seed, syntax.span)),
                (Seed::None | Seed::Default, None) => None,
                (Seed::None | Seed::Default, Some(_)) => {
                    unreachable!("normalized preview seeds match their bounded syntax")
                }
            },
            update: update.map(|update| {
                callback(
                    update,
                    syntax.update.as_ref().map_or(syntax.span, |path| path.span),
                )
            }),
            themes: themes
                .iter()
                .zip(syntax.themes.iter())
                .map(|(theme, path)| callback(theme.callback(), path.span))
                .collect(),
        }
    }

    fn subject_input(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
    ) -> Result<(Input, Option<application::Type>), crate::resolve::LocatedError> {
        let signature = &subject.signature;
        if syntax.bound {
            // `bind` arguments are constants re-read on each reset, as Granita
            // re-evaluates them, so no `Clone` is asked of them.
            let state = match signature.parameters.as_ref() {
                [parameter] => parameter.dereferenced(),
                parameters => application::Type::tuple(parameters).dereferenced(),
            };
            return Ok((Input::Owned(state.clone()), Some(state)));
        }
        if signature.receiver.is_some() {
            if signature.receiver != Some(receiver::Mode::Shared)
                || !signature.parameters.is_empty()
            {
                return Err(subject_error(
                    subject,
                    syntax,
                    "preview methods require exactly a shared `&self` receiver and no parameters",
                    signature.receiver_span.unwrap_or(subject.declaration),
                ));
            }
            let state = signature
                .owner
                .clone()
                .expect("resolved preview methods retain an owner type");
            return Ok((Input::Shared(state.clone()), Some(state)));
        }
        match signature.parameters.as_ref() {
            [] => Ok((Input::Stateless, None)),
            [parameter] => match parameter.reference() {
                Some((false, target)) => Ok((Input::Shared(target.clone()), Some(target.clone()))),
                Some((true, _)) => Err(subject_error(
                    subject,
                    syntax,
                    "preview function input cannot be a mutable reference",
                    signature.parameter_spans[0],
                )),
                None if parameter.provides(capability::Kind::Clone) => {
                    Ok((Input::Owned(parameter.clone()), Some(parameter.clone())))
                }
                None => Err(subject_error(
                    subject,
                    syntax,
                    "owned preview function input must provide `Clone`",
                    signature.parameter_spans[0],
                )),
            },
            _ => Err(subject_error(
                subject,
                syntax,
                "preview functions accept at most one state/input parameter",
                subject.declaration,
            )),
        }
    }

    fn subject_message(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
        output: &application::Type,
    ) -> Result<
        (
            application::Type,
            host::runtime::type_ref::Type,
            Option<host::projection::Site>,
        ),
        crate::resolve::LocatedError,
    > {
        if let Some([message]) = output.host_arguments(&self.widget) {
            let runtime = output.exact_runtime_type().ok_or_else(|| {
                subject_error(
                    subject,
                    syntax,
                    "preview output has no exact runtime representation",
                    subject.signature.output_span,
                )
            })?;
            return Ok((message.clone(), runtime, None));
        }
        if let Some((projection, arguments)) =
            output.projection_site(&self.widget, None, subject.signature.output_span)
            && let [message] = arguments.as_ref()
        {
            return Ok((
                message.clone(),
                projection.target().clone(),
                Some(projection),
            ));
        }
        Err(subject_error(
            subject,
            syntax,
            format!(
                "subject output `{}` is not compatible with `iced::Element<Message>`",
                output.display_name()
            ),
            subject.signature.output_span,
        ))
    }

    fn seed(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
        state: Option<&application::Type>,
    ) -> Result<Seed, crate::resolve::LocatedError> {
        let Some(state) = state else {
            if syntax.seed.is_some() {
                return Err(subject_error(
                    subject,
                    syntax,
                    "stateless previews cannot declare `seed`",
                    syntax.seed.as_ref().map_or(syntax.span, |seed| seed.span),
                ));
            }
            return Ok(Seed::None);
        };
        let Some(path) = syntax.seed.as_ref() else {
            // A hand-written `impl Default` runs, as `State::default()` does.
            if let Some(seed) = self.infer_method(subject, state, "Default::default")
                && seed.signature.receiver.is_none()
                && seed.signature.parameters.is_empty()
                && &seed.signature.output == state
            {
                return Ok(Seed::Callback(seed.callback()));
            }
            if state.provides(capability::Kind::Default) {
                return Ok(Seed::Default);
            }
            return Err(subject_error(
                subject,
                syntax,
                format!(
                    "preview input `{}` does not provide `Default`; declare an exact zero-argument `seed`",
                    state.display_name()
                ),
                syntax.span,
            ));
        };
        let seed = self.path(subject, path)?;
        let output = if syntax.bound {
            seed.signature.output.dereferenced()
        } else {
            seed.signature.output.clone()
        };
        if seed.signature.receiver.is_some()
            || !seed.signature.parameters.is_empty()
            || &output != state
        {
            return Err(path_error(
                subject,
                path,
                &seed,
                format!(
                    "seed must be a zero-argument function returning `{}`",
                    state.display_name()
                ),
            ));
        }
        Ok(Seed::Callback(seed.callback()))
    }

    fn update(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
        state: Option<&application::Type>,
        message: &application::Type,
    ) -> Result<Option<Callback>, crate::resolve::LocatedError> {
        let Some(state) = state else {
            if syntax.update.is_some() {
                return Err(subject_error(
                    subject,
                    syntax,
                    "stateless previews cannot declare `update`",
                    syntax
                        .update
                        .as_ref()
                        .map_or(syntax.span, |update| update.span),
                ));
            }
            if !message.is_unit() {
                return Err(subject_error(
                    subject,
                    syntax,
                    "stateless interactive previews are unsupported; use one State input and an exact update",
                    subject.signature.output_span,
                ));
            }
            return Ok(None);
        };
        let update = if let Some(path) = syntax.update.as_ref() {
            Some((self.path(subject, path)?, path.span))
        } else {
            self.infer_method(subject, state, "update")
                .map(|candidate| (candidate, syntax.span))
        };
        let Some((update, reference)) = update else {
            // A `bind` preview without an update shows its arguments; its
            // messages go nowhere.
            if message.is_unit() || syntax.bound {
                return Ok(None);
            }
            return Err(subject_error(
                subject,
                syntax,
                format!(
                    "interactive preview requires `{}` update `&mut State, Message -> () | Task<Message>`",
                    state.display_name()
                ),
                syntax.span,
            ));
        };
        let valid_output = update.signature.output.is_unit()
            || update
                .signature
                .output
                .matches_host(&self.task, std::slice::from_ref(message));
        if update.signature.receiver != Some(receiver::Mode::Mutable)
            || update.signature.owner.as_ref() != Some(state)
            || update.signature.parameters.as_ref() != std::slice::from_ref(message)
            || !valid_output
        {
            return Err(invalid(
                subject.source,
                Some(update.source),
                update.key.to_string(),
                format!(
                    "update must be `&mut {}, {} -> () | iced::Task<{}>`",
                    state.display_name(),
                    message.display_name(),
                    message.display_name()
                ),
                reference,
                Some(update.declaration),
            ));
        }
        Ok(Some(update.callback()))
    }

    fn themes(
        &self,
        subject: &Candidate,
        syntax: &Syntax,
        state: Option<&application::Type>,
    ) -> Result<Box<[Theme]>, crate::resolve::LocatedError> {
        let mut themes = Vec::with_capacity(syntax.themes.len());
        let mut names = BTreeSet::new();
        for path in &syntax.themes {
            let theme = self.path(subject, path)?;
            let valid_input = match (theme.signature.receiver, state) {
                (None, _) => theme.signature.parameters.is_empty(),
                (Some(receiver::Mode::Shared), Some(state)) => {
                    theme.signature.owner.as_ref() == Some(state)
                        && theme.signature.parameters.is_empty()
                }
                (Some(receiver::Mode::Owned | receiver::Mode::Mutable), _) | (_, None) => false,
            };
            if !valid_input || !theme.signature.output.matches_host(&self.theme, &[]) {
                return Err(path_error(
                    subject,
                    path,
                    &theme,
                    "theme must be `fn() -> iced::Theme` or `fn(&State) -> iced::Theme`",
                ));
            }
            let name: ImmutableString = theme.key.name().into();
            if !names.insert(name.clone()) {
                return Err(path_error(
                    subject,
                    path,
                    &theme,
                    "duplicate theme choice name",
                ));
            }
            themes.push(Theme {
                name,
                callback: theme.callback(),
            });
        }
        Ok(themes.into_boxed_slice())
    }

    fn path(
        &self,
        subject: &Candidate,
        path: &item::type_ref::Path,
    ) -> Result<Candidate, crate::resolve::LocatedError> {
        if path
            .segments
            .iter()
            .any(|segment| !segment.arguments.is_empty())
        {
            return Err(subject_error(
                subject,
                &Syntax::bare(path.span),
                "preview callback paths cannot have generic arguments",
                path.span,
            ));
        }
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.name.text.clone())
            .collect::<Vec<_>>();
        let graph_target = self.program.preview_path(subject.source, path.span);
        let key = if let Some(target) = graph_target {
            match target {
                PathTarget::Item(key) => Some(key.clone()),
                PathTarget::Associated { owner, member } => self.associated(owner, member),
            }
        } else {
            match segments.as_slice() {
                [name] => self.imported_item(subject.source, name).or_else(|| {
                    self.candidates.keys().find_map(|key| {
                        (key.owner().is_none()
                            && key.package() == subject.key.package()
                            && key.modules() == subject.key.modules()
                            && key.name() == name)
                            .then(|| key.clone())
                    })
                }),
                [owner, name] => self
                    .imported_type(subject.source, owner)
                    .or_else(|| self.local_type(subject, owner))
                    .and_then(|owner| self.associated(&owner, name)),
                _ => None,
            }
        };
        let Some(key) = key else {
            return Err(subject_error(
                subject,
                &Syntax::bare(path.span),
                format!(
                    "unknown or inaccessible preview callback `{}`; import package callbacks before referencing them",
                    segments.join("::")
                ),
                path.span,
            ));
        };
        let candidate = self
            .candidates
            .get(&key)
            .cloned()
            .expect("resolved preview paths select script callback candidates");
        if candidate.source != subject.source && !candidate.public {
            return Err(invalid(
                subject.source,
                Some(candidate.source),
                segments.join("::"),
                "preview callback is private to another source module",
                path.span,
                Some(candidate.declaration),
            ));
        }
        Ok(candidate)
    }

    fn imported_item(&self, source: crate::program::source::Id, name: &str) -> Option<item::Key> {
        self.program.items().iter().find_map(|shell| {
            if shell.source != source {
                return None;
            }
            let resolve::item::Kind::Use(item) = &shell.kind else {
                return None;
            };
            item.bindings.iter().find_map(|binding| {
                (binding.name.text == name)
                    .then_some(&binding.target)
                    .and_then(|target| match target {
                        resolve::item::Import::ScriptItem { key } => Some(key.clone()),
                        _ => None,
                    })
            })
        })
    }

    fn imported_type(
        &self,
        source: crate::program::source::Id,
        name: &str,
    ) -> Option<type_def::Identity> {
        self.program.items().iter().find_map(|shell| {
            if shell.source != source {
                return None;
            }
            let resolve::item::Kind::Use(item) = &shell.kind else {
                return None;
            };
            item.bindings.iter().find_map(|binding| {
                (binding.name.text == name)
                    .then_some(&binding.target)
                    .and_then(|target| match target {
                        resolve::item::Import::ScriptType { identity, .. } => {
                            Some(identity.clone())
                        }
                        _ => None,
                    })
            })
        })
    }

    fn local_type(&self, subject: &Candidate, name: &str) -> Option<type_def::Identity> {
        self.program.types().iter().find_map(|definition| {
            (definition.key().package() == subject.key.package()
                && definition.key().modules() == subject.key.modules()
                && definition.name() == name)
                .then(|| type_def::Identity::Script(definition.key().clone()))
        })
    }

    fn associated(&self, owner: &type_def::Identity, name: &str) -> Option<item::Key> {
        self.candidates
            .keys()
            .find_map(|key| (key.owner() == Some(owner) && key.name() == name).then(|| key.clone()))
    }

    fn infer_method(
        &self,
        subject: &Candidate,
        state: &application::Type,
        name: &str,
    ) -> Option<Candidate> {
        let identity = state.identity()?;
        self.candidates.values().find_map(|candidate| {
            (candidate.key.owner() == Some(identity)
                && candidate.key.name() == name
                && (candidate.source == subject.source || candidate.public))
                .then(|| candidate.clone())
        })
    }
}

impl Candidate {
    fn callback(&self) -> Callback {
        Callback {
            item: self.value.clone(),
            receiver: self.signature.receiver,
            parameters: self.signature.parameters.clone(),
            output: self.signature.output.clone(),
        }
    }
}

fn collect_subjects(
    program: &crate::resolve::Program,
) -> Result<Vec<Subject<'_>>, crate::resolve::LocatedError> {
    let runtime = application::RuntimeGraph::new(program);
    let definitions = program
        .types()
        .iter()
        .map(|definition| (definition.id(), definition))
        .collect::<BTreeMap<_, _>>();
    let methods = program
        .methods()
        .iter()
        .map(|method| (method.id(), method))
        .collect::<BTreeMap<_, _>>();
    let mut subjects = Vec::new();

    for shell in program.items() {
        match &shell.kind {
            resolve::item::Kind::Function(function) => {
                let syntaxes = previews(&function.header);
                if !syntaxes.is_empty() {
                    subjects.push(Subject {
                        candidate: function_candidate(shell, function, &definitions, &runtime),
                        syntaxes,
                    });
                }
            }
            resolve::item::Kind::Impl(item) => {
                reject_header_previews(shell.source, &item.header, "impl block")?;
                for function in &item.functions {
                    let syntaxes = previews(&function.header);
                    if syntaxes.is_empty() {
                        continue;
                    }
                    let resolve::item::Lowering::NominalMethod(id) = function.body.lowering else {
                        return Err(invalid(
                            shell.source,
                            None,
                            function.name.text.clone(),
                            "preview subject has no exact resolved method",
                            syntaxes[0].span,
                            Some(function.name.span),
                        ));
                    };
                    let method = methods[&id];
                    subjects.push(Subject {
                        candidate: method_candidate(method, &definitions, &runtime),
                        syntaxes,
                    });
                }
            }
            resolve::item::Kind::Use(item) => {
                reject_header_previews(shell.source, &item.header, "use declaration")?;
            }
            resolve::item::Kind::Struct(item) => {
                reject_header_previews(shell.source, &item.header, "struct declaration")?;
            }
            resolve::item::Kind::Enum(item) => {
                reject_header_previews(shell.source, &item.header, "enum declaration")?;
            }
            resolve::item::Kind::Trait(item) => {
                reject_header_previews(shell.source, &item.header, "trait declaration")?;
            }
        }
    }
    Ok(subjects)
}

fn collect_candidates(
    program: &crate::resolve::Program,
    runtime: &application::RuntimeGraph,
) -> BTreeMap<item::Key, Candidate> {
    let definitions = program
        .types()
        .iter()
        .map(|definition| (definition.id(), definition))
        .collect::<BTreeMap<_, _>>();
    let mut candidates = BTreeMap::new();
    for shell in program.items() {
        if let resolve::item::Kind::Function(function) = &shell.kind {
            let candidate = function_candidate(shell, function, &definitions, runtime);
            candidates.insert(candidate.key.clone(), candidate);
        }
    }
    for method in program.methods() {
        let candidate = method_candidate(method, &definitions, runtime);
        candidates.insert(candidate.key.clone(), candidate);
    }
    candidates
}

fn function_candidate(
    shell: &resolve::item::Shell,
    function: &resolve::item::Function,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &application::RuntimeGraph,
) -> Candidate {
    Candidate {
        source: shell.source,
        key: shell
            .key
            .clone()
            .expect("resolved functions retain a stable item key"),
        value: item::Value::new(
            shell
                .key
                .clone()
                .expect("resolved functions retain a stable item key"),
            item::Kind::Function,
            None,
            None,
            signature::Fingerprint::function(
                false,
                None,
                &function.parameters,
                function.output.as_ref(),
            ),
            capability::Fingerprint::default(),
        ),
        public: function.header.visibility.is_some(),
        declaration: function.name.span,
        signature: Signature {
            receiver: None,
            owner: None,
            parameters: function
                .parameters
                .iter()
                .map(|parameter| {
                    application::Type::from_resolved(&parameter.ty, definitions, runtime)
                })
                .collect(),
            output: function.output.as_ref().map_or_else(
                || unit_type(function.body.span, definitions, runtime),
                |output| application::Type::from_resolved(output, definitions, runtime),
            ),
            receiver_span: None,
            parameter_spans: function
                .parameters
                .iter()
                .map(|parameter| parameter.span)
                .collect(),
            output_span: function
                .output
                .as_ref()
                .map_or(function.body.span, type_span),
        },
    }
}

fn method_candidate(
    method: &crate::method::Definition,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &application::RuntimeGraph,
) -> Candidate {
    let owner = definitions[&method.owner()];
    let kind = if method.receiver().is_some() {
        item::Kind::Method
    } else {
        item::Kind::AssociatedFunction
    };
    Candidate {
        source: method.source(),
        key: method.key().clone(),
        value: item::Value::new(
            method.key().clone(),
            kind,
            Some(method.owner_identity().clone()),
            method.receiver(),
            method.fingerprint().clone(),
            owner.capabilities().fingerprint(),
        ),
        public: method.is_public(),
        declaration: method.name_span(),
        signature: Signature {
            receiver: method.receiver(),
            owner: Some(application::Type::from_definition(owner, runtime)),
            parameters: method
                .parameters()
                .iter()
                .map(|parameter| {
                    application::Type::from_resolved(&parameter.ty, definitions, runtime)
                })
                .collect(),
            output: application::Type::from_resolved(method.output(), definitions, runtime),
            receiver_span: method.receiver_span(),
            parameter_spans: method
                .parameters()
                .iter()
                .map(|parameter| parameter.span)
                .collect(),
            output_span: type_span(method.output()),
        },
    }
}

fn unit_type(
    span: Span,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &application::RuntimeGraph,
) -> application::Type {
    application::Type::from_resolved(
        &crate::resolve::type_ref::Type::Unit(span),
        definitions,
        runtime,
    )
}

fn type_span(ty: &crate::resolve::type_ref::Type) -> Span {
    match ty {
        crate::resolve::type_ref::Type::Unit(span)
        | crate::resolve::type_ref::Type::Tuple { span, .. }
        | crate::resolve::type_ref::Type::Reference { span, .. } => *span,
        crate::resolve::type_ref::Type::Path(path) => path.span,
    }
}

fn previews(header: &item::Header) -> Vec<&Syntax> {
    header
        .attributes
        .iter()
        .filter_map(item::Attribute::preview)
        .collect()
}

fn reject_header_previews(
    source: crate::program::source::Id,
    header: &item::Header,
    subject: &'static str,
) -> Result<(), crate::resolve::LocatedError> {
    if let Some(preview) = previews(header).first() {
        Err(invalid(
            source,
            None,
            subject,
            "`#[preview]` is supported only on functions and methods",
            preview.span,
            None,
        ))
    } else {
        Ok(())
    }
}

fn require_message_capabilities(
    subject: &Candidate,
    syntax: &Syntax,
    message: &application::Type,
) -> Result<(), crate::resolve::LocatedError> {
    for capability in [
        capability::Kind::Clone,
        capability::Kind::Send,
        capability::Kind::Static,
    ] {
        if !message.provides(capability) {
            return Err(subject_error(
                subject,
                syntax,
                format!(
                    "preview Message `{}` does not provide `{}`",
                    message.display_name(),
                    capability.name()
                ),
                subject.signature.output_span,
            ));
        }
    }
    Ok(())
}

fn normalize_name(name: &str) -> Option<ImmutableString> {
    let mut normalized = String::with_capacity(name.len());
    let mut separator = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    (!normalized.is_empty()).then(|| normalized.into())
}

fn path_error(
    subject: &Candidate,
    path: &item::type_ref::Path,
    callback: &Candidate,
    reason: impl Into<ImmutableString>,
) -> crate::resolve::LocatedError {
    invalid(
        subject.source,
        Some(callback.source),
        callback.key.to_string(),
        reason,
        path.span,
        Some(callback.declaration),
    )
}

fn subject_error(
    subject: &Candidate,
    _syntax: &Syntax,
    reason: impl Into<ImmutableString>,
    reference: Span,
) -> crate::resolve::LocatedError {
    invalid(
        subject.source,
        None,
        subject.key.to_string(),
        reason,
        reference,
        Some(subject.declaration),
    )
}

fn invalid(
    primary: crate::program::source::Id,
    secondary: Option<crate::program::source::Id>,
    preview: impl Into<ImmutableString>,
    reason: impl Into<ImmutableString>,
    reference: Span,
    declaration: Option<Span>,
) -> crate::resolve::LocatedError {
    let error = crate::resolve::Error::InvalidPreview {
        preview: preview.into(),
        reason: reason.into(),
        reference,
        declaration,
    };
    match secondary {
        Some(secondary) => crate::resolve::LocatedError::related(primary, secondary, error),
        None => crate::resolve::LocatedError::primary(primary, error),
    }
}
