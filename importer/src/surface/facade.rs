//! Bounded runner-owned compiler facade plan.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::classify::representability::{self, Representability};

pub(crate) mod cached;

pub use cached::Cached;

const SCHEMA: u32 = 7;
const ALIAS_SITE_DOMAIN: &[u8] = b"rawr.import.surface.facade.alias-site/v2";
const PROJECTION_SOURCE_SITE_DOMAIN: &[u8] =
    b"rawr.import.surface.facade.projection-source-site/v1";
const ABI_SITE_DOMAIN: &[u8] = b"rawr.import.surface.facade.abi-site/v1";
const OPERATION_SITE_DOMAIN: &[u8] = b"rawr.import.surface.facade.operation-site/v1";

/// Exact static imported facades required to recompile a Rawr snapshot.
#[derive(Deserialize, Serialize)]
pub struct Plan {
    schema: u32,
    aliases: Box<[AliasSite]>,
    sources: Box<[ProjectionSourceSite]>,
    abis: Box<[AbiSite]>,
    operations: Box<[OperationSite]>,
    facades: Box<[Facade]>,
}

/// One compiler-authenticated source alias retained only for facade binding.
#[derive(Deserialize, Serialize)]
pub struct AliasSite {
    root: Box<str>,
    path: Box<str>,
    namespace: Namespace,
    export: [u8; 32],
    alias: [u8; 32],
    target: [u8; 32],
    parameters: Box<[AliasParameter]>,
    selector: [u8; 32],
}

/// One compiler-authenticated alias site used only to prove a projection source.
#[derive(Deserialize, Serialize)]
pub struct ProjectionSourceSite {
    root: Box<str>,
    path: Box<str>,
    namespace: Namespace,
    export: [u8; 32],
    alias: [u8; 32],
    source: [u8; 32],
    shape: SourceShape,
    parameters: Box<[AliasParameter]>,
    selector: [u8; 32],
}

/// One compiler-authenticated support-only nominal ABI site.
///
/// This record proves an exact public Rust shape without granting a Rawr-visible
/// declaration or operation. Framework profiles may assign closed roles to
/// selected sites after checking their own pinned source law.
#[derive(Deserialize, Serialize)]
pub struct AbiSite {
    root: Box<str>,
    path: Box<str>,
    namespace: Namespace,
    export: [u8; 32],
    definition: [u8; 32],
    shape: [u8; 32],
    selector: [u8; 32],
}

/// One compiler-authenticated support-only free-operation site.
///
/// The shape digest seals the complete canonical compiler definition,
/// including the unit, full signature type trees, predicates, span, and
/// origin. The ordered formal ledger remains explicit so a framework profile
/// can prove one total specialization without making the item Rawr-nameable.
#[derive(Deserialize, Serialize)]
pub struct OperationSite {
    root: Box<str>,
    path: Box<str>,
    namespace: Namespace,
    export: [u8; 32],
    definition: [u8; 32],
    formals: Box<[OperationParameter]>,
    shape: [u8; 32],
    selector: [u8; 32],
}

/// One exact compiler formal retained by a support-only operation site.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationParameter {
    owner: [u8; 32],
    index: u32,
    kind: ParameterKind,
}

/// Closed source shape admitted by the projection proof vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SourceShape {
    /// A nominal Rust struct. The complete generic shape is carried by `parameters`.
    Struct,
}

/// Closed Rust namespace carried by a support-only alias site.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Namespace {
    /// Module, type, trait, or type-alias namespace.
    Type,
    /// Function, constant, static, variant, or constructor namespace.
    Value,
    /// Macro namespace.
    Macro,
}

/// One ordered formal-parameter mapping from alias definition to target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AliasParameter {
    kind: ParameterKind,
    alias: u32,
    target: u32,
}

/// Closed generic-parameter class used by an alias mapping.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ParameterKind {
    /// Lifetime parameter.
    Lifetime,
    /// Type parameter.
    Type,
    /// Const parameter.
    Const,
}

/// One Rawr-visible imported root and its exact declarations.
#[derive(Deserialize, Serialize)]
pub struct Facade {
    root: Box<str>,
    types: Box<[TypeDefinition]>,
    items: Box<[Item]>,
    /// The traits Rawr code names (`use rand::Rng;`), in path order. Each
    /// is a module to Rawr. The bridge brings each into scope, so a method
    /// a trait provides resolves in Rust as it does in the program.
    traits: Box<[Box<str>]>,
}

/// One imported nominal definition and complete admitted shape.
#[derive(Deserialize, Serialize)]
pub struct TypeDefinition {
    path: Box<str>,
    identity: [u8; 32],
    parameters: Box<[Box<str>]>,
    copy: bool,
    shape: Shape,
}

/// Closed imported nominal shape.
#[derive(Deserialize, Serialize)]
pub enum Shape {
    /// Struct form, constructor, and fields.
    Struct {
        form: Form,
        constructor: Option<super::identity::Selector>,
        fields: Box<[Field]>,
        constructible: bool,
    },
    /// Enum variants in declaration order.
    Enum { variants: Box<[Variant]> },
}

/// Closed nominal field grammar.
#[derive(Clone, Copy, Deserialize, Serialize)]
pub enum Form {
    /// No fields.
    Unit,
    /// Positional fields.
    Tuple,
    /// Named fields.
    Record,
}

/// One compiler-visible structural field.
#[derive(Deserialize, Serialize)]
pub struct Field {
    name: Box<str>,
    ty: Type,
    selector: super::identity::Selector,
}

/// One compiler-visible enum variant.
#[derive(Deserialize, Serialize)]
pub struct Variant {
    name: Box<str>,
    fields: Box<[Field]>,
    constructible: bool,
    selector: super::identity::Selector,
}

/// One exact imported item overload.
#[derive(Deserialize, Serialize)]
pub struct Item {
    path: Box<str>,
    kind: ItemKind,
    selector: super::identity::Selector,
    signature: Signature,
}

/// Closed static item family.
#[derive(Clone, Copy, Deserialize, Serialize)]
pub enum ItemKind {
    /// Free function.
    Function,
    /// Associated function.
    AssociatedFunction,
    /// Owned receiver method.
    OwnedMethod,
    /// Shared receiver method.
    SharedMethod,
    /// Mutable receiver method.
    MutableMethod,
    /// Free constant.
    Constant,
    /// Associated constant.
    AssociatedConstant,
}

/// Exact generic parameters, inputs, and output for one item.
#[derive(Deserialize, Serialize)]
pub struct Signature {
    parameters: Box<[Box<str>]>,
    inputs: Box<[Type]>,
    output: Type,
}

/// Closed Rawr compiler type reference vocabulary.
#[derive(Deserialize, Serialize)]
pub enum Type {
    /// Unit.
    Unit,
    /// Boolean.
    Bool,
    /// Character.
    Char,
    /// Floating-point number.
    F64,
    /// Integer.
    I64,
    /// Inclusive integer range.
    InclusiveRange,
    /// Immutable string.
    String,
    /// Another Rust numeric primitive by name (`u32`, `f32`, ...).
    Primitive(Box<str>),
    /// Imported nominal type with exact semantic identity.
    Nominal {
        identity: [u8; 32],
        arguments: Box<[Type]>,
    },
    /// Direct compile-time imported function item with no runtime value.
    FunctionItem { input: Box<Type>, output: Box<Type> },
    /// Declared generic parameter by exact name.
    Parameter(Box<str>),
    /// Rust's `Option<T>`, a Rawr `Option`.
    Option(Box<Type>),
    /// Rust's `Result<T, E>`, a Rawr `Result`.
    Result(Box<Type>, Box<Type>),
}

impl Signature {
    /// Whether every imported type the signature names is in `declared`.
    fn declared(&self, declared: &std::collections::BTreeSet<[u8; 32]>) -> bool {
        self.inputs.iter().all(|ty| ty.declared(declared)) && self.output.declared(declared)
    }
}

impl Type {
    fn declared(&self, declared: &std::collections::BTreeSet<[u8; 32]>) -> bool {
        match self {
            Self::Nominal {
                identity,
                arguments,
            } => declared.contains(identity) && arguments.iter().all(|ty| ty.declared(declared)),
            Self::FunctionItem { input, output } | Self::Result(input, output) => {
                input.declared(declared) && output.declared(declared)
            }
            Self::Option(element) => element.declared(declared),
            _ => true,
        }
    }
}

impl Plan {
    /// Reuse the exact facade selected from a current content-addressed API graph.
    pub fn encode_cached(
        contract: &crate::Contract<crate::Classified>,
        limits: &crate::cargo::Limits,
    ) -> crate::Result<Cached> {
        cached::encode(contract, limits)
    }

    /// Encode one classified contract into the runner's bounded facade plan.
    pub fn encode(
        contract: &crate::Contract<crate::Classified>,
        limits: &crate::cargo::Limits,
    ) -> Result<Box<[u8]>, super::Error> {
        Self::encode_outcomes(contract.raw_graph(), contract.outcomes(), limits)
    }

    pub(crate) fn encode_parts(
        graph: &crate::api::Graph<impl crate::api::Phase>,
        parts: &crate::classify::ClassifiedParts,
        limits: &crate::cargo::Limits,
    ) -> Result<Box<[u8]>, super::Error> {
        Self::encode_outcomes(graph, parts.outcomes(), limits)
    }

    fn encode_outcomes<'a>(
        graph: &crate::api::Graph<impl crate::api::Phase>,
        outcomes: impl ExactSizeIterator<Item = crate::classify::Outcome<'a>>,
        limits: &crate::cargo::Limits,
    ) -> Result<Box<[u8]>, super::Error> {
        let mut facades = Vec::<Facade>::new();
        for outcome in outcomes {
            let named_trait = matches!(
                outcome.representability(),
                Representability::Rejected(reason)
                    if reason.kind() == representability::reject::Kind::Trait
            );
            if !named_trait
                && matches!(
                    outcome.representability(),
                    Representability::Excluded(_) | Representability::Rejected(_)
                )
            {
                continue;
            }
            let root = outcome.export().root_alias();
            let index = match facades.binary_search_by(|facade| facade.root.as_ref().cmp(root)) {
                Ok(index) => index,
                Err(index) => {
                    facades
                        .try_reserve(1)
                        .map_err(|_| super::Error::Allocation)?;
                    facades.insert(
                        index,
                        Facade {
                            root: copy(root)?,
                            types: Box::default(),
                            items: Box::default(),
                            traits: Box::default(),
                        },
                    );
                    index
                }
            };
            let relative = path(outcome.export().path())?;
            let facade = &mut facades[index];
            if named_trait {
                let mut traits = std::mem::take(&mut facade.traits).into_vec();
                if let Err(position) = traits.binary_search(&relative) {
                    traits.try_reserve(1).map_err(|_| super::Error::Allocation)?;
                    traits.insert(position, relative);
                }
                facade.traits = traits.into_boxed_slice();
                continue;
            }
            if let Some(ty) = type_definition(&relative, outcome)? {
                let mut types = std::mem::take(&mut facade.types).into_vec();
                types.try_reserve(1).map_err(|_| super::Error::Allocation)?;
                types.push(ty);
                facade.types = types.into_boxed_slice();
            }
            if let Some(item) = item(&relative, outcome)? {
                let mut items = std::mem::take(&mut facade.items).into_vec();
                items.try_reserve(1).map_err(|_| super::Error::Allocation)?;
                items.push(item);
                facade.items = items.into_boxed_slice();
            }
            for associated in outcome
                .implementations()
                .flat_map(crate::classify::ImplementationOutcome::associated)
            {
                let associated_path = format!("{relative}::{}", associated.name());
                if let Some(item) = item(&associated_path, associated)? {
                    let mut items = std::mem::take(&mut facade.items).into_vec();
                    items.try_reserve(1).map_err(|_| super::Error::Allocation)?;
                    items.push(item);
                    facade.items = items.into_boxed_slice();
                }
            }
        }
        // An item whose signature names a type no facade declares (one
        // the importer could not model) is unavailable by itself; the rest
        // of its facade stays.
        let declared = facades
            .iter()
            .flat_map(|facade| facade.types.iter().map(|ty| ty.identity))
            .collect::<std::collections::BTreeSet<_>>();
        for facade in &mut facades {
            facade
                .types
                .sort_by(|left, right| left.path.cmp(&right.path));
            let mut items = std::mem::take(&mut facade.items).into_vec();
            items.retain(|item| item.signature.declared(&declared));
            items.sort_by(|left, right| left.path.cmp(&right.path));
            facade.items = items.into_boxed_slice();
        }
        let mut aliases = Vec::new();
        let mut sources = Vec::<ProjectionSourceSite>::new();
        let mut abis = Vec::<AbiSite>::new();
        let mut operations = Vec::<OperationSite>::new();
        for export in graph.exports() {
            let crate::api::ledger::Path::Imported(export) = export else {
                continue;
            };
            if let Some(site) = abi_site(graph, export)? {
                match abis.binary_search_by_key(&site.selector, |candidate| candidate.selector) {
                    Ok(_) => return Err(super::Error::ForeignIdentity),
                    Err(position) => {
                        abis.try_reserve_exact(1)
                            .map_err(|_| super::Error::Allocation)?;
                        abis.insert(position, site);
                    }
                }
            }
            if let Some(site) = operation_site(graph, export)? {
                match operations
                    .binary_search_by_key(&site.selector, |candidate| candidate.selector)
                {
                    Ok(_) => return Err(super::Error::ForeignIdentity),
                    Err(position) => {
                        operations
                            .try_reserve_exact(1)
                            .map_err(|_| super::Error::Allocation)?;
                        operations.insert(position, site);
                    }
                }
            }
            let Some(alias) = alias_site(graph, export)? else {
                continue;
            };
            match aliases
                .binary_search_by_key(&alias.selector, |candidate: &AliasSite| candidate.selector)
            {
                Ok(_) => return Err(super::Error::ForeignIdentity),
                Err(position) => {
                    aliases
                        .try_reserve_exact(1)
                        .map_err(|_| super::Error::Allocation)?;
                    aliases.insert(position, alias);
                }
            }
            if let Some(source) = projection_source_site(graph, export)? {
                match sources.binary_search_by_key(&source.selector, |candidate| candidate.selector)
                {
                    Ok(_) => return Err(super::Error::ForeignIdentity),
                    Err(position) => {
                        sources
                            .try_reserve_exact(1)
                            .map_err(|_| super::Error::Allocation)?;
                        sources.insert(position, source);
                    }
                }
            }
        }
        let bytes = serde_json::to_vec(&Self {
            schema: SCHEMA,
            aliases: aliases.into_boxed_slice(),
            sources: sources.into_boxed_slice(),
            abis: abis.into_boxed_slice(),
            operations: operations.into_boxed_slice(),
            facades: facades.into_boxed_slice(),
        })
        .map_err(|_| super::Error::Encoding)?;
        // The encoded plan size is an accounting target recorded on a
        // surface account; the physical bound is the node content ceiling.
        super::account::Account::surface().exceeded(
            "runner facade plan bytes",
            limits.json_bytes,
            bytes.len(),
        );
        Ok(bytes.into_boxed_slice())
    }

    /// Decode one generated plan under the active importer bounds.
    pub fn decode(bytes: &[u8], limits: &crate::cargo::Limits) -> Result<Self, super::Error> {
        Self::decode_accounted(bytes, limits, &mut super::account::Account::surface())
    }

    pub(crate) fn decode_accounted(
        bytes: &[u8],
        limits: &crate::cargo::Limits,
        account: &mut super::account::Account,
    ) -> Result<Self, super::Error> {
        account.exceeded("runner facade plan bytes", limits.json_bytes, bytes.len());
        let plan: Self = serde_json::from_slice(bytes).map_err(|_| super::Error::Encoding)?;
        plan.validate()?;
        Ok(plan)
    }

    /// Verify one complete intended facade surface without installing or
    /// publishing it into a production engine.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub fn verify_exact_surface(
        bytes: &[u8],
        limits: &crate::cargo::Limits,
        root: &str,
        expected_types: &[&str],
        expected_items: &[&str],
        rejected_paths: &[&str],
    ) -> Result<(), super::Error> {
        let plan = Self::decode(bytes, limits)?;
        let mut facades = plan.facades().filter(|facade| facade.root() == root);
        let facade = facades.next().ok_or(super::Error::InstanceMismatch)?;
        if facades.next().is_some()
            || facade.types().len() != expected_types.len()
            || facade.items().len() != expected_items.len()
            || expected_types
                .iter()
                .any(|path| !facade.types().any(|ty| ty.path() == *path))
            || expected_items
                .iter()
                .any(|path| !facade.items().any(|item| item.path() == *path))
            || rejected_paths.iter().any(|path| {
                facade.types().any(|ty| ty.path() == *path)
                    || facade.items().any(|item| item.path() == *path)
            })
        {
            return Err(super::Error::InstanceMismatch);
        }
        Ok(())
    }

    /// Iterate exact facade roots in canonical order.
    pub fn facades(&self) -> impl ExactSizeIterator<Item = &Facade> {
        self.facades.iter()
    }

    /// Iterate compiler-authenticated support-only alias sites in selector order.
    pub fn aliases(&self) -> impl ExactSizeIterator<Item = &AliasSite> {
        self.aliases.iter()
    }

    /// Iterate compiler-authenticated support-only projection source sites.
    pub fn projection_sources(&self) -> impl ExactSizeIterator<Item = &ProjectionSourceSite> {
        self.sources.iter()
    }

    /// Iterate compiler-authenticated support-only ABI sites.
    pub fn abi_sites(&self) -> impl ExactSizeIterator<Item = &AbiSite> {
        self.abis.iter()
    }

    /// Iterate compiler-authenticated support-only free-operation sites.
    pub fn operation_sites(&self) -> impl ExactSizeIterator<Item = &OperationSite> {
        self.operations.iter()
    }

    fn validate(&self) -> Result<(), super::Error> {
        if self.schema != SCHEMA
            || !self
                .facades
                .windows(2)
                .all(|pair| pair[0].root < pair[1].root)
            || !self
                .aliases
                .windows(2)
                .all(|pair| pair[0].selector < pair[1].selector)
            || !self
                .sources
                .windows(2)
                .all(|pair| pair[0].selector < pair[1].selector)
            || !self
                .abis
                .windows(2)
                .all(|pair| pair[0].selector < pair[1].selector)
            || !self
                .operations
                .windows(2)
                .all(|pair| pair[0].selector < pair[1].selector)
        {
            return Err(super::Error::ForeignIdentity);
        }
        for facade in &self.facades {
            if facade.root.is_empty()
                || !facade
                    .types
                    .windows(2)
                    .all(|pair| pair[0].path <= pair[1].path)
                || !facade
                    .items
                    .windows(2)
                    .all(|pair| pair[0].path <= pair[1].path)
            {
                return Err(super::Error::ForeignIdentity);
            }
        }
        for alias in &self.aliases {
            if alias.root.is_empty()
                || alias.path.is_empty()
                || alias.export == [0; 32]
                || alias.alias == [0; 32]
                || alias.target == [0; 32]
                || alias.selector != alias_selector(alias)
                || !alias
                    .parameters
                    .windows(2)
                    .all(|pair| pair[0].target < pair[1].target)
                || self
                    .facades
                    .iter()
                    .filter(|facade| facade.root.as_ref() == alias.root.as_ref())
                    .count()
                    != 1
            {
                return Err(super::Error::ForeignIdentity);
            }
        }
        for source in &self.sources {
            if source.root.is_empty()
                || source.path.is_empty()
                || source.export == [0; 32]
                || source.alias == [0; 32]
                || source.source == [0; 32]
                || source.namespace != Namespace::Type
                || source.shape != SourceShape::Struct
                || source.selector != projection_source_selector(source)
                || !source
                    .parameters
                    .windows(2)
                    .all(|pair| pair[0].target < pair[1].target)
                || self
                    .facades
                    .iter()
                    .filter(|facade| facade.root.as_ref() == source.root.as_ref())
                    .count()
                    != 1
            {
                return Err(super::Error::ForeignIdentity);
            }
        }
        for site in &self.abis {
            if site.root.is_empty()
                || site.path.is_empty()
                || site.namespace != Namespace::Type
                || site.export == [0; 32]
                || site.definition == [0; 32]
                || site.shape == [0; 32]
                || site.selector != abi_selector(site)
            {
                return Err(super::Error::ForeignIdentity);
            }
        }
        for site in &self.operations {
            if site.root.is_empty()
                || site.path.is_empty()
                || site.namespace != Namespace::Value
                || site.export == [0; 32]
                || site.definition == [0; 32]
                || site.shape == [0; 32]
                || site.selector != operation_selector(site)
                || !site
                    .formals
                    .windows(2)
                    .all(|pair| pair[0].index < pair[1].index)
                || site
                    .formals
                    .iter()
                    .any(|formal| formal.owner != site.definition)
            {
                return Err(super::Error::ForeignIdentity);
            }
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "test-fixtures"))]
pub(crate) fn iced_semantic_fixture() -> Box<[u8]> {
    let types = [
        ("Application", 2_usize, 1_u8),
        ("Element", 1, 3),
        ("Rectangle", 1, 0xb2),
        ("Result", 0, 6),
        ("Subscription", 1, 7),
        ("Task", 1, 8),
        ("Theme", 0, 9),
        ("Vector", 1, 0xb3),
    ]
    .into_iter()
    .map(|(path, parameters, identity)| TypeDefinition {
        path: path.into(),
        identity: [identity; 32],
        parameters: (0..parameters)
            .map(|parameter| format!("T{parameter}").into_boxed_str())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        copy: false,
        shape: Shape::Struct {
            form: Form::Unit,
            constructor: None,
            fields: Box::default(),
            constructible: false,
        },
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let mut abis = [
        fixture_abi("Color", 0),
        fixture_abi("Length", 1),
        fixture_abi("Rectangle", 2),
        fixture_abi("Vector", 3),
    ];
    abis.sort_unstable_by_key(|site| site.selector);
    assert!(abis
        .windows(2)
        .all(|pair| pair[0].selector < pair[1].selector));
    serde_json::to_vec(&Plan {
        schema: SCHEMA,
        aliases: Box::from([fixture_alias()]),
        sources: Box::from([fixture_source("Text", [0xe3; 32], [0xa3; 32], [0x22; 32])]),
        abis: Box::from(abis),
        operations: Box::from([fixture_operation()]),
        facades: Box::from([Facade {
            root: ROOT_FIXTURE.into(),
            types,
            items: Box::default(),
            traits: Box::default(),
        }]),
    })
    .expect("the static iced semantic fixture encodes")
    .into_boxed_slice()
}

#[cfg(any(test, feature = "test-fixtures"))]
pub(crate) fn iced_text_module_fixture(bytes: &[u8]) -> Box<[u8]> {
    let limits = crate::cargo::Limits::standard();
    let mut plan = Plan::decode(bytes, &limits).expect("the iced facade fixture decodes");
    let facade = plan
        .facades
        .iter_mut()
        .find(|facade| facade.root.as_ref() == ROOT_FIXTURE)
        .expect("the iced facade fixture retains one iced root");
    let mut types = std::mem::take(&mut facade.types).into_vec();
    {
        let (path, identity) = ("widget::label::Style", [0xf4; 32]);
        let index = types
            .binary_search_by(|definition| definition.path.as_ref().cmp(path))
            .expect_err("the module-collision fixture adds distinct child paths");
        types.insert(
            index,
            TypeDefinition {
                path: path.into(),
                identity,
                parameters: Box::default(),
                copy: false,
                shape: Shape::Struct {
                    form: Form::Unit,
                    constructor: None,
                    fields: Box::default(),
                    constructible: false,
                },
            },
        );
    }
    facade.types = types.into_boxed_slice();
    let primary = facade
        .items
        .iter_mut()
        .find(|item| item.path.as_ref() == "widget::button::primary")
        .expect("the iced fixture retains one ordinary qualified member");
    primary.path = "widget::text::primary".into();
    facade
        .items
        .sort_by(|left, right| left.path.cmp(&right.path));
    plan.validate()
        .expect("the text-module collision fixture remains canonical");
    serde_json::to_vec(&plan)
        .expect("the text-module collision fixture encodes")
        .into_boxed_slice()
}

#[cfg(any(test, feature = "test-fixtures"))]
pub(crate) fn iced_projection_fixture() -> Box<[u8]> {
    let dial = TypeDefinition {
        path: "Dial".into(),
        identity: [0xd1; 32],
        parameters: Box::from([Box::<str>::from("Message")]),
        copy: false,
        shape: Shape::Struct {
            form: Form::Unit,
            constructor: None,
            fields: Box::default(),
            constructible: false,
        },
    };
    let iced = iced_semantic_fixture();
    let limits = crate::cargo::Limits::standard();
    let iced = Plan::decode(&iced, &limits).expect("the static iced facade fixture decodes");
    let Plan {
        aliases,
        sources: _,
        abis,
        operations,
        facades,
        schema: _,
    } = iced;
    let iced = facades
        .into_vec()
        .pop()
        .expect("the static iced facade fixture has one owner");
    serde_json::to_vec(&Plan {
        schema: SCHEMA,
        aliases,
        sources: Box::from([fixture_source("Dial", [0xe4; 32], [0xa4; 32], [0xd1; 32])]),
        abis,
        operations,
        facades: Box::from([
            Facade {
                root: "dial".into(),
                types: Box::from([dial]),
                items: Box::default(),
                traits: Box::default(),
            },
            iced,
        ]),
    })
    .expect("the static iced cross-facade projection fixture encodes")
    .into_boxed_slice()
}

#[cfg(any(test, feature = "test-fixtures"))]
const ROOT_FIXTURE: &str = "iced";

#[cfg(any(test, feature = "test-fixtures"))]
fn fixture_alias() -> AliasSite {
    let mut alias = AliasSite {
        root: ROOT_FIXTURE.into(),
        path: "Element".into(),
        namespace: Namespace::Type,
        export: [0xe1; 32],
        alias: [0xa1; 32],
        target: [3; 32],
        parameters: Box::from([AliasParameter {
            kind: ParameterKind::Type,
            alias: 0,
            target: 0,
        }]),
        selector: [0; 32],
    };
    alias.selector = alias_selector(&alias);
    alias
}

#[cfg(any(test, feature = "test-fixtures"))]
fn fixture_source(
    path: &str,
    export: [u8; 32],
    alias: [u8; 32],
    source: [u8; 32],
) -> ProjectionSourceSite {
    let mut site = ProjectionSourceSite {
        root: if source == [0xd1; 32] {
            "dial"
        } else {
            ROOT_FIXTURE
        }
        .into(),
        path: path.into(),
        namespace: Namespace::Type,
        export,
        alias,
        source,
        shape: SourceShape::Struct,
        parameters: Box::from([
            AliasParameter {
                kind: ParameterKind::Lifetime,
                alias: 0,
                target: 0,
            },
            AliasParameter {
                kind: ParameterKind::Type,
                alias: 1,
                target: 1,
            },
        ]),
        selector: [0; 32],
    };
    site.selector = projection_source_selector(&site);
    site
}

#[cfg(any(test, feature = "test-fixtures"))]
fn fixture_abi(path: &str, index: u8) -> AbiSite {
    let mut site = AbiSite {
        root: ROOT_FIXTURE.into(),
        path: path.into(),
        namespace: Namespace::Type,
        export: [0xf0 + index; 32],
        definition: [0xb0 + index; 32],
        shape: [0xc0 + index; 32],
        selector: [0; 32],
    };
    site.selector = abi_selector(&site);
    site
}

#[cfg(any(test, feature = "test-fixtures"))]
fn fixture_operation() -> OperationSite {
    let mut site = OperationSite {
        root: ROOT_FIXTURE.into(),
        path: "widget::text".into(),
        namespace: Namespace::Value,
        export: [0xe5; 32],
        definition: [0xa5; 32],
        formals: Box::from([
            OperationParameter {
                owner: [0xa5; 32],
                index: 0,
                kind: ParameterKind::Lifetime,
            },
            OperationParameter {
                owner: [0xa5; 32],
                index: 1,
                kind: ParameterKind::Type,
            },
            OperationParameter {
                owner: [0xa5; 32],
                index: 2,
                kind: ParameterKind::Type,
            },
        ]),
        shape: [0x25; 32],
        selector: [0; 32],
    };
    site.selector = operation_selector(&site);
    site
}

impl Facade {
    /// Return the Rawr-visible facade root.
    pub fn root(&self) -> &str {
        &self.root
    }
    /// Iterate exact nominal declarations.
    pub fn types(&self) -> impl ExactSizeIterator<Item = &TypeDefinition> {
        self.types.iter()
    }
    /// Iterate exact item overloads.
    pub fn items(&self) -> impl ExactSizeIterator<Item = &Item> {
        self.items.iter()
    }
    /// Iterate the traits Rawr code names, relative to the root.
    pub fn traits(&self) -> impl ExactSizeIterator<Item = &str> {
        self.traits.iter().map(AsRef::as_ref)
    }
}

impl AliasSite {
    pub(crate) fn checked(
        export: &crate::api::ledger::Export,
        alias: &crate::api::model::Definition,
        target: &crate::api::model::Definition,
    ) -> Result<Option<Self>, super::Error> {
        use crate::api::{identity, model};

        if export.target() != alias.id()
            || export.key().namespace() != identity::Namespace::Type
            || alias.id().namespace() != identity::Namespace::Type
            || target.id().namespace() != identity::Namespace::Type
        {
            return Ok(None);
        }
        let model::Kind::TypeAlias { ty, generics } = alias.kind() else {
            return Ok(None);
        };
        let model::Type::Definition { id, arguments } = ty else {
            return Ok(None);
        };
        if id != target.id() {
            return Ok(None);
        }
        let target_generics = match target.kind() {
            model::Kind::Struct { generics, .. } | model::Kind::Enum { generics, .. } => generics,
            _ => return Ok(None),
        };
        let Some(parameters) =
            alias_parameters(alias, generics, target, target_generics, arguments)
        else {
            return Ok(None);
        };
        let mut site = Self {
            root: copy(export.key().root_alias())?,
            path: path(export.key().path())?,
            namespace: Namespace::Type,
            export: *super::identity::Export::imported(export).as_bytes(),
            alias: *super::identity::Definition::of(alias.id()).as_bytes(),
            target: *super::identity::Definition::of(target.id()).as_bytes(),
            parameters,
            selector: [0; 32],
        };
        site.selector = alias_selector(&site);
        Ok(Some(site))
    }

    pub(crate) fn try_copy(&self) -> Result<Self, super::Error> {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| super::Error::Allocation)?;
        parameters.extend_from_slice(&self.parameters);
        Ok(Self {
            root: copy(&self.root)?,
            path: copy(&self.path)?,
            namespace: self.namespace,
            export: self.export,
            alias: self.alias,
            target: self.target,
            parameters: parameters.into_boxed_slice(),
            selector: self.selector,
        })
    }

    /// Return the facade root containing this source-visible alias.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Return the exact facade-relative source path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the exact Rust namespace.
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }

    /// Borrow the exact export identity.
    pub const fn export_identity(&self) -> &[u8; 32] {
        &self.export
    }

    /// Borrow the source alias definition identity.
    pub const fn alias_identity(&self) -> &[u8; 32] {
        &self.alias
    }

    /// Borrow the normalized semantic target identity.
    pub const fn target_identity(&self) -> &[u8; 32] {
        &self.target
    }

    /// Borrow the complete ordered formal-parameter mapping.
    pub const fn parameters(&self) -> &[AliasParameter] {
        &self.parameters
    }

    /// Borrow the domain-separated alias-site selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl ProjectionSourceSite {
    pub(crate) fn checked(
        export: &crate::api::ledger::Export,
        alias: &crate::api::model::Definition,
        source: &crate::api::model::Definition,
    ) -> Result<Option<Self>, super::Error> {
        let Some(alias) = AliasSite::checked(export, alias, source)? else {
            return Ok(None);
        };
        if !matches!(source.kind(), crate::api::model::Kind::Struct { .. }) {
            return Ok(None);
        }
        let mut site = Self {
            root: alias.root,
            path: alias.path,
            namespace: alias.namespace,
            export: alias.export,
            alias: alias.alias,
            source: alias.target,
            shape: SourceShape::Struct,
            parameters: alias.parameters,
            selector: [0; 32],
        };
        site.selector = projection_source_selector(&site);
        Ok(Some(site))
    }

    pub(crate) fn try_copy(&self) -> Result<Self, super::Error> {
        let mut parameters = Vec::new();
        parameters
            .try_reserve_exact(self.parameters.len())
            .map_err(|_| super::Error::Allocation)?;
        parameters.extend_from_slice(&self.parameters);
        Ok(Self {
            root: copy(&self.root)?,
            path: copy(&self.path)?,
            namespace: self.namespace,
            export: self.export,
            alias: self.alias,
            source: self.source,
            shape: self.shape,
            parameters: parameters.into_boxed_slice(),
            selector: self.selector,
        })
    }

    pub fn root(&self) -> &str {
        &self.root
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }
    pub const fn export_identity(&self) -> &[u8; 32] {
        &self.export
    }
    pub const fn alias_identity(&self) -> &[u8; 32] {
        &self.alias
    }
    pub const fn source_identity(&self) -> &[u8; 32] {
        &self.source
    }
    pub const fn shape(&self) -> SourceShape {
        self.shape
    }
    pub const fn parameters(&self) -> &[AliasParameter] {
        &self.parameters
    }
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl AbiSite {
    pub(crate) fn checked(
        export: &crate::api::ledger::Export,
        definition: &crate::api::model::Definition,
    ) -> Result<Option<Self>, super::Error> {
        if export.target() != definition.id() {
            return Ok(None);
        }
        abi_site_from_definition(export, definition)
    }

    /// Return the selected facade root.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Return the exact facade-relative public path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the exact Rust namespace.
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }

    /// Borrow the exact public export identity.
    pub const fn export_identity(&self) -> &[u8; 32] {
        &self.export
    }

    /// Borrow the exact underlying definition identity.
    pub const fn definition_identity(&self) -> &[u8; 32] {
        &self.definition
    }

    /// Borrow the complete compiler-shape digest.
    pub const fn shape_digest(&self) -> &[u8; 32] {
        &self.shape
    }

    /// Borrow the domain-separated ABI-site selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) fn try_copy(&self) -> Result<Self, super::Error> {
        Ok(Self {
            root: copy(&self.root)?,
            path: copy(&self.path)?,
            namespace: self.namespace,
            export: self.export,
            definition: self.definition,
            shape: self.shape,
            selector: self.selector,
        })
    }
}

impl OperationSite {
    pub(crate) fn checked(
        export: &crate::api::ledger::Export,
        definition: &crate::api::model::Definition,
    ) -> Result<Option<Self>, super::Error> {
        operation_site_from_definition(export, definition)
    }

    /// Return the selected facade root.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Return the exact facade-relative public operation path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Return the exact Rust namespace.
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }

    /// Borrow the exact public export identity.
    pub const fn export_identity(&self) -> &[u8; 32] {
        &self.export
    }

    /// Borrow the exact underlying function identity.
    pub const fn definition_identity(&self) -> &[u8; 32] {
        &self.definition
    }

    /// Borrow the complete ordered compiler formal ledger.
    pub const fn formals(&self) -> &[OperationParameter] {
        &self.formals
    }

    /// Borrow the digest of the complete compiler function definition.
    pub const fn shape_digest(&self) -> &[u8; 32] {
        &self.shape
    }

    /// Borrow the domain-separated operation-site selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) fn try_copy(&self) -> Result<Self, super::Error> {
        let mut formals = Vec::new();
        formals
            .try_reserve_exact(self.formals.len())
            .map_err(|_| super::Error::Allocation)?;
        formals.extend_from_slice(&self.formals);
        Ok(Self {
            root: copy(&self.root)?,
            path: copy(&self.path)?,
            namespace: self.namespace,
            export: self.export,
            definition: self.definition,
            formals: formals.into_boxed_slice(),
            shape: self.shape,
            selector: self.selector,
        })
    }
}

impl OperationParameter {
    /// Borrow the exact semantic owner of this compiler formal.
    pub const fn owner(&self) -> &[u8; 32] {
        &self.owner
    }

    /// Return the compiler generic index.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Return the exact generic-parameter class.
    pub const fn kind(&self) -> ParameterKind {
        self.kind
    }
}

impl AliasParameter {
    /// Return the closed generic-parameter class.
    pub const fn kind(&self) -> ParameterKind {
        self.kind
    }

    /// Return the alias definition's formal index.
    pub const fn alias_index(&self) -> u32 {
        self.alias
    }

    /// Return the normalized target's formal index.
    pub const fn target_index(&self) -> u32 {
        self.target
    }
}

fn alias_site(
    graph: &crate::api::Graph<impl crate::api::Phase>,
    export: &crate::api::ledger::Export,
) -> Result<Option<AliasSite>, super::Error> {
    let mut aliases = graph
        .definitions()
        .filter(|definition| definition.id() == export.target());
    let Some(alias) = aliases.next() else {
        return Ok(None);
    };
    if aliases.next().is_some() {
        return Err(super::Error::ForeignIdentity);
    }
    let crate::api::model::Kind::TypeAlias {
        ty: crate::api::model::Type::Definition { id, .. },
        ..
    } = alias.kind()
    else {
        return Ok(None);
    };
    let mut targets = graph
        .definitions()
        .filter(|definition| definition.id() == id);
    let Some(target) = targets.next() else {
        return Ok(None);
    };
    if targets.next().is_some() {
        return Err(super::Error::ForeignIdentity);
    }
    AliasSite::checked(export, alias, target)
}

fn abi_site(
    graph: &crate::api::Graph<impl crate::api::Phase>,
    export: &crate::api::ledger::Export,
) -> Result<Option<AbiSite>, super::Error> {
    let mut definitions = graph
        .definitions()
        .filter(|definition| definition.id() == export.target());
    let Some(definition) = definitions.next() else {
        return Ok(None);
    };
    if definitions.next().is_some() {
        return Err(super::Error::ForeignIdentity);
    }
    abi_site_from_definition(export, definition)
}

fn operation_site(
    graph: &crate::api::Graph<impl crate::api::Phase>,
    export: &crate::api::ledger::Export,
) -> Result<Option<OperationSite>, super::Error> {
    let mut definitions = graph
        .definitions()
        .filter(|definition| definition.id() == export.target());
    let Some(definition) = definitions.next() else {
        return Ok(None);
    };
    if definitions.next().is_some() {
        return Err(super::Error::ForeignIdentity);
    }
    operation_site_from_definition(export, definition)
}

fn operation_site_from_definition(
    export: &crate::api::ledger::Export,
    definition: &crate::api::model::Definition,
) -> Result<Option<OperationSite>, super::Error> {
    use crate::api::{identity, model};

    if export.target() != definition.id()
        || export.key().namespace() != identity::Namespace::Value
        || definition.owner() != &model::Owner::Free
        || definition.visibility() != &model::Visibility::Public
    {
        return Ok(None);
    }
    let model::Kind::Function {
        signature: _,
        generics,
    } = definition.kind()
    else {
        return Ok(None);
    };
    let definition_identity = *super::identity::Definition::of(definition.id()).as_bytes();
    let mut formals = Vec::new();
    formals
        .try_reserve_exact(generics.parameters().len())
        .map_err(|_| super::Error::Allocation)?;
    for parameter in generics.parameters() {
        let owner = *super::identity::Definition::of(parameter.owner()).as_bytes();
        if owner != definition_identity {
            return Err(super::Error::ForeignIdentity);
        }
        formals.push(OperationParameter {
            owner,
            index: parameter.index(),
            kind: parameter_kind(parameter.kind()),
        });
    }
    if !formals.windows(2).all(|pair| pair[0].index < pair[1].index) {
        return Err(super::Error::ForeignIdentity);
    }
    let canonical = serde_json::to_vec(definition).map_err(|_| super::Error::Encoding)?;
    let mut site = OperationSite {
        root: copy(export.key().root_alias())?,
        path: path(export.key().path())?,
        namespace: Namespace::Value,
        export: *super::identity::Export::imported(export).as_bytes(),
        definition: definition_identity,
        formals: formals.into_boxed_slice(),
        shape: Sha256::digest(canonical).into(),
        selector: [0; 32],
    };
    site.selector = operation_selector(&site);
    Ok(Some(site))
}

fn abi_site_from_definition(
    export: &crate::api::ledger::Export,
    definition: &crate::api::model::Definition,
) -> Result<Option<AbiSite>, super::Error> {
    use crate::api::{identity, model};

    if export.key().namespace() != identity::Namespace::Type
        || definition.owner() != &model::Owner::Free
        || definition.visibility() != &model::Visibility::Public
        || !matches!(
            definition.kind(),
            model::Kind::Struct { .. } | model::Kind::Enum { .. }
        )
    {
        return Ok(None);
    }
    let canonical = serde_json::to_vec(&(
        definition.id(),
        definition.owner(),
        definition.visibility(),
        definition.kind(),
    ))
    .map_err(|_| super::Error::Encoding)?;
    let mut site = AbiSite {
        root: copy(export.key().root_alias())?,
        path: path(export.key().path())?,
        namespace: Namespace::Type,
        export: *super::identity::Export::imported(export).as_bytes(),
        definition: *super::identity::Definition::of(definition.id()).as_bytes(),
        shape: Sha256::digest(canonical).into(),
        selector: [0; 32],
    };
    site.selector = abi_selector(&site);
    Ok(Some(site))
}

fn projection_source_site(
    graph: &crate::api::Graph<impl crate::api::Phase>,
    export: &crate::api::ledger::Export,
) -> Result<Option<ProjectionSourceSite>, super::Error> {
    let mut aliases = graph
        .definitions()
        .filter(|definition| definition.id() == export.target());
    let Some(alias) = aliases.next() else {
        return Ok(None);
    };
    if aliases.next().is_some() {
        return Err(super::Error::ForeignIdentity);
    }
    let crate::api::model::Kind::TypeAlias {
        ty: crate::api::model::Type::Definition { id, .. },
        ..
    } = alias.kind()
    else {
        return Ok(None);
    };
    let mut sources = graph
        .definitions()
        .filter(|definition| definition.id() == id);
    let Some(source) = sources.next() else {
        return Ok(None);
    };
    if sources.next().is_some() {
        return Err(super::Error::ForeignIdentity);
    }
    ProjectionSourceSite::checked(export, alias, source)
}

fn alias_parameters(
    alias: &crate::api::model::Definition,
    alias_generics: &crate::api::model::Generics,
    target: &crate::api::model::Definition,
    target_generics: &crate::api::model::Generics,
    arguments: &[crate::api::model::Argument],
) -> Option<Box<[AliasParameter]>> {
    use crate::api::model;

    if alias_generics.parameters().len() != arguments.len()
        || target_generics.parameters().len() != arguments.len()
    {
        return None;
    }
    let mut output = Vec::new();
    output.try_reserve_exact(arguments.len()).ok()?;
    for (target_parameter, argument) in target_generics.parameters().zip(arguments) {
        if target_parameter.owner() != target.id() {
            return None;
        }
        let (owner, index, kind) = match argument {
            model::Argument::Lifetime(model::Lifetime::Early { owner, index }) => {
                (owner, *index, ParameterKind::Lifetime)
            }
            model::Argument::Type(model::Type::Parameter { owner, index }) => {
                (owner, *index, ParameterKind::Type)
            }
            model::Argument::Const(model::Const::Parameter { owner, index }) => {
                (owner, *index, ParameterKind::Const)
            }
            _ => return None,
        };
        if owner != alias.id()
            || parameter_kind(target_parameter.kind()) != kind
            || !alias_generics.parameters().any(|parameter| {
                parameter.owner() == alias.id()
                    && parameter.index() == index
                    && parameter_kind(parameter.kind()) == kind
            })
        {
            return None;
        }
        output.push(AliasParameter {
            kind,
            alias: index,
            target: target_parameter.index(),
        });
    }
    Some(output.into_boxed_slice())
}

fn parameter_kind(kind: &crate::api::model::ParameterKind) -> ParameterKind {
    match kind {
        crate::api::model::ParameterKind::Lifetime { .. } => ParameterKind::Lifetime,
        crate::api::model::ParameterKind::Type { .. } => ParameterKind::Type,
        crate::api::model::ParameterKind::Const { .. } => ParameterKind::Const,
    }
}

fn alias_selector(alias: &AliasSite) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(ALIAS_SITE_DOMAIN);
    framed_bytes(&mut digest, alias.root.as_bytes());
    framed_bytes(&mut digest, alias.path.as_bytes());
    digest.update([match alias.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(alias.export);
    digest.update(alias.alias);
    digest.update(alias.target);
    digest.update(
        u64::try_from(alias.parameters.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for parameter in &alias.parameters {
        digest.update([match parameter.kind {
            ParameterKind::Lifetime => 0,
            ParameterKind::Type => 1,
            ParameterKind::Const => 2,
        }]);
        digest.update(parameter.alias.to_le_bytes());
        digest.update(parameter.target.to_le_bytes());
    }
    digest.finalize().into()
}

fn projection_source_selector(source: &ProjectionSourceSite) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(PROJECTION_SOURCE_SITE_DOMAIN);
    framed_bytes(&mut digest, source.root.as_bytes());
    framed_bytes(&mut digest, source.path.as_bytes());
    digest.update([match source.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(source.export);
    digest.update(source.alias);
    digest.update(source.source);
    digest.update([match source.shape {
        SourceShape::Struct => 0,
    }]);
    digest.update(
        u64::try_from(source.parameters.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for parameter in &source.parameters {
        digest.update([match parameter.kind {
            ParameterKind::Lifetime => 0,
            ParameterKind::Type => 1,
            ParameterKind::Const => 2,
        }]);
        digest.update(parameter.alias.to_le_bytes());
        digest.update(parameter.target.to_le_bytes());
    }
    digest.finalize().into()
}

fn abi_selector(site: &AbiSite) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(ABI_SITE_DOMAIN);
    framed_bytes(&mut digest, site.root.as_bytes());
    framed_bytes(&mut digest, site.path.as_bytes());
    digest.update([match site.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(site.export);
    digest.update(site.definition);
    digest.update(site.shape);
    digest.finalize().into()
}

fn operation_selector(site: &OperationSite) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(OPERATION_SITE_DOMAIN);
    framed_bytes(&mut digest, site.root.as_bytes());
    framed_bytes(&mut digest, site.path.as_bytes());
    digest.update([match site.namespace {
        Namespace::Type => 0,
        Namespace::Value => 1,
        Namespace::Macro => 2,
    }]);
    digest.update(site.export);
    digest.update(site.definition);
    digest.update(
        u64::try_from(site.formals.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for formal in &site.formals {
        digest.update(formal.owner);
        digest.update(formal.index.to_le_bytes());
        digest.update([match formal.kind {
            ParameterKind::Lifetime => 0,
            ParameterKind::Type => 1,
            ParameterKind::Const => 2,
        }]);
    }
    digest.update(site.shape);
    digest.finalize().into()
}

fn framed_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    digest.update(bytes);
}

impl TypeDefinition {
    /// Return the facade-relative path.
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Return the stable imported identity.
    pub const fn identity(&self) -> &[u8; 32] {
        &self.identity
    }
    /// Return generic parameter names.
    pub fn parameters(&self) -> impl ExactSizeIterator<Item = &str> {
        self.parameters.iter().map(AsRef::as_ref)
    }
    /// Return whether the native type is copyable.
    pub const fn copy(&self) -> bool {
        self.copy
    }
    /// Return the exact nominal shape.
    pub const fn shape(&self) -> &Shape {
        &self.shape
    }
}

impl Shape {
    /// Decompose a struct shape.
    pub const fn struct_parts(
        &self,
    ) -> Option<(Form, Option<&super::identity::Selector>, &[Field], bool)> {
        match self {
            Self::Struct {
                form,
                constructor,
                fields,
                constructible,
            } => Some((*form, constructor.as_ref(), fields, *constructible)),
            Self::Enum { .. } => None,
        }
    }
    /// Borrow enum variants.
    pub const fn variants(&self) -> Option<&[Variant]> {
        match self {
            Self::Enum { variants } => Some(variants),
            Self::Struct { .. } => None,
        }
    }
}

impl Field {
    /// Return the source field name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Return the exact field type.
    pub const fn ty(&self) -> &Type {
        &self.ty
    }
    /// Return the semantic field selector.
    pub const fn selector(&self) -> &super::identity::Selector {
        &self.selector
    }
}

impl Variant {
    /// Return the source variant name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Return ordered variant fields.
    pub const fn fields(&self) -> &[Field] {
        &self.fields
    }
    /// Return whether source construction is admitted.
    pub const fn constructible(&self) -> bool {
        self.constructible
    }
    /// Return the semantic variant selector.
    pub const fn selector(&self) -> &super::identity::Selector {
        &self.selector
    }
}

impl Item {
    /// Return the facade-relative item path.
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Return the closed item family.
    pub const fn kind(&self) -> ItemKind {
        self.kind
    }
    /// Return the semantic operation selector.
    pub const fn selector(&self) -> &super::identity::Selector {
        &self.selector
    }
    /// Return the exact item signature.
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }
}

impl Signature {
    /// Return generic parameter names.
    pub fn parameters(&self) -> impl ExactSizeIterator<Item = &str> {
        self.parameters.iter().map(AsRef::as_ref)
    }
    /// Return ordered input types.
    pub const fn inputs(&self) -> &[Type] {
        &self.inputs
    }
    /// Return the output type.
    pub const fn output(&self) -> &Type {
        &self.output
    }
}

fn type_definition(
    path: &str,
    outcome: crate::classify::Outcome<'_>,
) -> Result<Option<TypeDefinition>, super::Error> {
    let definition = outcome
        .identity()
        .definition()
        .ok_or(super::Error::ForeignIdentity)?;
    let (parameters, copyable, shape) = match outcome.representability() {
        Representability::Struct(plan) => (
            parameters(plan.parameters())?,
            plan.copy(),
            Shape::Struct {
                form: form(plan.form()),
                constructor: plan
                    .constructor()
                    .map(super::identity::Selector::try_clone)
                    .transpose()?,
                fields: fields(plan.fields(), plan.parameters())?,
                constructible: plan.constructible(),
            },
        ),
        Representability::Enum(plan) => {
            let mut variants = Vec::new();
            variants
                .try_reserve_exact(plan.variants().len())
                .map_err(|_| super::Error::Allocation)?;
            for variant in plan.variants() {
                variants.push(Variant {
                    name: copy(variant.name())?,
                    fields: fields(variant.fields(), plan.parameters())?,
                    constructible: variant.constructible(),
                    selector: variant.selector().try_clone()?,
                });
            }
            (
                parameters(plan.parameters())?,
                plan.copy(),
                Shape::Enum {
                    variants: variants.into_boxed_slice(),
                },
            )
        }
        Representability::Value(_)
        | Representability::Function(_)
        | Representability::Method(_)
        | Representability::Constant(_)
        | Representability::FunctionItem(_)
        | Representability::Invocation(_)
        | Representability::Excluded(_)
        | Representability::Rejected(_) => return Ok(None),
    };
    Ok(Some(TypeDefinition {
        path: copy(path)?,
        identity: *definition.as_bytes(),
        parameters,
        copy: copyable,
        shape,
    }))
}

fn item(path: &str, outcome: crate::classify::Outcome<'_>) -> Result<Option<Item>, super::Error> {
    use super::identity::Selector;
    let (kind, signature) = match outcome.representability() {
        Representability::Function(plan) => (
            match outcome.identity().selector() {
                Selector::Function { .. } => ItemKind::Function,
                Selector::AssociatedFunction { .. } => ItemKind::AssociatedFunction,
                _ => return Err(super::Error::ForeignIdentity),
            },
            signature(plan.signature())?,
        ),
        Representability::Method(plan) => (
            match plan.receiver() {
                representability::ownership::Receiver::Owned => ItemKind::OwnedMethod,
                representability::ownership::Receiver::Shared => ItemKind::SharedMethod,
                representability::ownership::Receiver::Mutable => ItemKind::MutableMethod,
            },
            signature(plan.signature())?,
        ),
        Representability::Constant(plan) => (
            match outcome.identity().selector() {
                Selector::Constant { .. } => ItemKind::Constant,
                Selector::AssociatedConstant { .. } => ItemKind::AssociatedConstant,
                _ => return Err(super::Error::ForeignIdentity),
            },
            Signature {
                parameters: Box::default(),
                inputs: Box::default(),
                output: ty(plan.ty(), &[])?,
            },
        ),
        Representability::Value(_)
        | Representability::Struct(_)
        | Representability::Enum(_)
        | Representability::Excluded(_)
        | Representability::Rejected(_) => return Ok(None),
        Representability::FunctionItem(plan) => (
            match outcome.identity().selector() {
                Selector::Function { .. } => ItemKind::Function,
                Selector::AssociatedFunction { .. } => ItemKind::AssociatedFunction,
                // A generic method Rawr calls at the types it infers: every
                // parameter a type, none a callable.
                Selector::Method { .. }
                    if plan.class().is_none()
                        && plan.signature().parameters().iter().all(|parameter| {
                            parameter.kind() == representability::signature::Kind::Type
                        }) =>
                {
                    match plan.receiver() {
                        Some(representability::ownership::Receiver::Owned) => {
                            ItemKind::OwnedMethod
                        }
                        Some(representability::ownership::Receiver::Shared) => {
                            ItemKind::SharedMethod
                        }
                        Some(representability::ownership::Receiver::Mutable) => {
                            ItemKind::MutableMethod
                        }
                        None => return Ok(None),
                    }
                }
                Selector::Method { .. } => return Ok(None),
                _ => return Err(super::Error::ForeignIdentity),
            },
            signature(plan.signature())?,
        ),
        Representability::Invocation(plan) => (
            match outcome.identity().selector() {
                Selector::AssociatedFunction { .. } => ItemKind::AssociatedFunction,
                Selector::Method { .. } => match plan.receiver() {
                    Some(representability::ownership::Receiver::Owned) => ItemKind::OwnedMethod,
                    Some(representability::ownership::Receiver::Shared) => ItemKind::SharedMethod,
                    Some(representability::ownership::Receiver::Mutable) => {
                        ItemKind::MutableMethod
                    }
                    None => return Err(super::Error::InstanceMismatch),
                },
                _ => return Err(super::Error::ForeignIdentity),
            },
            signature(plan.signature())?,
        ),
    };
    Ok(Some(Item {
        path: copy(path)?,
        kind,
        selector: outcome.identity().selector().try_clone()?,
        signature,
    }))
}

fn signature(source: &representability::signature::Signature) -> Result<Signature, super::Error> {
    let parameters = parameters(source.parameters())?;
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(source.inputs().len())
        .map_err(|_| super::Error::Allocation)?;
    for input in source.inputs() {
        inputs.push(ty(input, source.parameters())?);
    }
    Ok(Signature {
        parameters,
        inputs: inputs.into_boxed_slice(),
        output: ty(source.output(), source.parameters())?,
    })
}

fn fields(
    source: &[representability::structure::Field],
    parameters: &[representability::signature::Parameter],
) -> Result<Box<[Field]>, super::Error> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(source.len())
        .map_err(|_| super::Error::Allocation)?;
    for field in source {
        output.push(Field {
            name: copy(field.name())?,
            ty: ty(field.ty(), parameters)?,
            selector: field.selector().try_clone()?,
        });
    }
    Ok(output.into_boxed_slice())
}

fn parameters(
    source: &[representability::signature::Parameter],
) -> Result<Box<[Box<str>]>, super::Error> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(source.len())
        .map_err(|_| super::Error::Allocation)?;
    for parameter in source {
        output.push(copy(parameter.name())?);
    }
    Ok(output.into_boxed_slice())
}

fn ty(
    source: &representability::convert::Type,
    parameters: &[representability::signature::Parameter],
) -> Result<Type, super::Error> {
    use representability::convert::{Builtin, Type as Source};
    Ok(match source {
        Source::Unit => Type::Unit,
        Source::Builtin(Builtin::Bool) => Type::Bool,
        Source::Builtin(Builtin::Char) => Type::Char,
        Source::Builtin(Builtin::F64) => Type::F64,
        Source::Builtin(Builtin::I64) => Type::I64,
        Source::Builtin(Builtin::InclusiveRange) => Type::InclusiveRange,
        Source::Builtin(Builtin::String) => Type::String,
        Source::Builtin(builtin) => Type::Primitive(builtin.rust_name().unwrap_or_default().into()),
        Source::Nominal {
            definition,
            arguments,
        } => {
            let mut converted = Vec::new();
            converted
                .try_reserve_exact(arguments.len())
                .map_err(|_| super::Error::Allocation)?;
            for argument in arguments {
                converted.push(ty(argument, parameters)?);
            }
            Type::Nominal {
                identity: *definition.as_bytes(),
                arguments: converted.into_boxed_slice(),
            }
        }
        Source::FunctionItem { input, output } => Type::FunctionItem {
            input: Box::new(ty(input, parameters)?),
            output: Box::new(ty(output, parameters)?),
        },
        Source::ProfileMessage { payload, .. } => ty(payload, parameters)?,
        Source::Option(element) => Type::Option(Box::new(ty(element, parameters)?)),
        Source::Result(ok, err) => Type::Result(
            Box::new(ty(ok, parameters)?),
            Box::new(ty(err, parameters)?),
        ),
        Source::Parameter(index) => {
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.index() == *index)
                .ok_or(super::Error::ForeignIdentity)?;
            Type::Parameter(copy(parameter.name())?)
        }
    })
}

fn form(source: crate::api::model::FieldForm) -> Form {
    match source {
        crate::api::model::FieldForm::Unit => Form::Unit,
        crate::api::model::FieldForm::Tuple => Form::Tuple,
        crate::api::model::FieldForm::Record => Form::Record,
    }
}

fn path<'a>(segments: impl ExactSizeIterator<Item = &'a str>) -> Result<Box<str>, super::Error> {
    let mut output = String::new();
    for segment in segments {
        if !output.is_empty() {
            output.push_str("::");
        }
        output.push_str(segment);
    }
    if output.is_empty() {
        return Err(super::Error::ForeignIdentity);
    }
    Ok(output.into_boxed_str())
}

fn copy(source: &str) -> Result<Box<str>, super::Error> {
    let mut output = String::new();
    output
        .try_reserve_exact(source.len())
        .map_err(|_| super::Error::Allocation)?;
    output.push_str(source);
    Ok(output.into_boxed_str())
}

#[cfg(test)]
mod alias_schema_tests {
    use super::Plan;

    #[test]
    fn facade_schema_one_and_missing_alias_ledger_reject() {
        let limits = crate::cargo::Limits::standard();
        let bytes = super::iced_semantic_fixture();
        let mut stale: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        stale["schema"] = serde_json::json!(1);
        assert!(Plan::decode(&serde_json::to_vec(&stale).unwrap(), &limits).is_err());
        let mut missing: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        missing.as_object_mut().unwrap().remove("aliases");
        assert!(Plan::decode(&serde_json::to_vec(&missing).unwrap(), &limits).is_err());
    }

    /// The facade plan size is an accounting target. Exactly at the
    /// target decodes silently; one over decodes whole and records the axis.
    #[test]
    fn facade_plan_past_the_json_target_decodes_and_records() {
        let bytes = super::iced_semantic_fixture();
        let decode = |json_bytes: usize| {
            let mut limits = crate::cargo::Limits::standard();
            limits.json_bytes = json_bytes;
            let mut account = crate::surface::account::Account::surface();
            let plan = Plan::decode_accounted(&bytes, &limits, &mut account)
                .expect("the facade plan decodes under every JSON target");
            assert!(plan.facades().next().is_some());
            account.exceedances()
        };
        assert!(!decode(bytes.len()).is_exceeded());
        let axis = *decode(bytes.len() - 1)
            .axis("runner facade plan bytes")
            .expect("the plan byte target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (bytes.len() - 1) as u64,
                bytes.len() as u64,
                bytes.len() as u64
            )
        );
    }
}
