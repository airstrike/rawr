use rawr::host;

static PROJECTION_ARGUMENTS: [rawr_iced::profile::runner::Argument; 1] =
    [rawr_iced::profile::runner::Argument::fresh()];
static SOURCE_ARGUMENTS: [rawr_iced::profile::runner::Argument; 1] =
    [rawr_iced::profile::runner::Argument::source(0)];
static ALIAS_PARAMETERS: [rawr_iced::profile::runner::AliasParameter; 1] =
    [rawr_iced::profile::runner::AliasParameter::new(
        rawr_iced::profile::runner::ParameterKind::Type,
        0,
        0,
    )];
static SOURCE_PARAMETERS: [rawr_iced::profile::runner::AliasParameter; 2] = [
    rawr_iced::profile::runner::AliasParameter::new(
        rawr_iced::profile::runner::ParameterKind::Lifetime,
        0,
        0,
    ),
    rawr_iced::profile::runner::AliasParameter::new(
        rawr_iced::profile::runner::ParameterKind::Type,
        1,
        1,
    ),
];
static SOURCE_RELATIONS: [rawr_iced::profile::runner::SourceArgument; 2] = [
    rawr_iced::profile::runner::SourceArgument::Region {
        alias: 0,
        source: 0,
    },
    rawr_iced::profile::runner::SourceArgument::Parameter {
        kind: rawr_iced::profile::runner::ParameterKind::Type,
        alias: 1,
        source: 1,
    },
];
static ABI_DEFAULT_F32: [rawr_iced::profile::runner::ExtensionAbiArgument; 1] =
    [rawr_iced::profile::runner::ExtensionAbiArgument::DefaultF32 { parameter: 0 }];
static OPERATION_FORMALS: [rawr_iced::profile::runner::OperationFormal; 3] = [
    rawr_iced::profile::runner::OperationFormal::new(
        [0xa5; 32],
        0,
        rawr_iced::profile::runner::ParameterKind::Lifetime,
    ),
    rawr_iced::profile::runner::OperationFormal::new(
        [0xa5; 32],
        1,
        rawr_iced::profile::runner::ParameterKind::Type,
    ),
    rawr_iced::profile::runner::OperationFormal::new(
        [0xa5; 32],
        2,
        rawr_iced::profile::runner::ParameterKind::Type,
    ),
];
static ENDPOINT_ARGUMENTS: [rawr_iced::profile::runner::EndpointArgument; 3] = [
    rawr_iced::profile::runner::EndpointArgument::Region { parameter: 0 },
    rawr_iced::profile::runner::EndpointArgument::Definition {
        parameter: 1,
        definition: [0x31; 32],
    },
    rawr_iced::profile::runner::EndpointArgument::OwnedString {
        parameter: 2,
        lifetime: 0,
    },
];

fn aliases() -> [rawr_iced::profile::runner::AliasTarget<'static>; 1] {
    [rawr_iced::profile::runner::AliasTarget::checked(
        "iced",
        "Element",
        rawr_iced::profile::runner::Namespace::Type,
        [0xe1; 32],
        [0xa1; 32],
        [3; 32],
        &ALIAS_PARAMETERS,
    )]
}

fn projections() -> [rawr_iced::profile::runner::Projection<'static>; 1] {
    let aliases = aliases();
    let sources = sources();
    [rawr_iced::profile::runner::Projection::checked(
        *sources[0].selector(),
        *aliases[0].selector(),
        &PROJECTION_ARGUMENTS,
    )]
}

fn cross_projections() -> [rawr_iced::profile::runner::Projection<'static>; 1] {
    let aliases = aliases();
    let sources = cross_sources();
    [rawr_iced::profile::runner::Projection::checked(
        *sources[0].selector(),
        *aliases[0].selector(),
        &SOURCE_ARGUMENTS,
    )]
}

fn sources() -> [rawr_iced::profile::runner::ProjectionSourceSite<'static>; 1] {
    let aliases = aliases();
    [rawr_iced::profile::runner::ProjectionSourceSite::checked(
        "iced",
        "Text",
        rawr_iced::profile::runner::Namespace::Type,
        [0xe3; 32],
        [0xa3; 32],
        [0x22; 32],
        &SOURCE_PARAMETERS,
        &SOURCE_RELATIONS,
        [0x80; 32],
        [0x81; 32],
        *aliases[0].selector(),
    )]
}

fn cross_sources() -> [rawr_iced::profile::runner::ProjectionSourceSite<'static>; 1] {
    let aliases = aliases();
    [rawr_iced::profile::runner::ProjectionSourceSite::checked(
        "dial",
        "Dial",
        rawr_iced::profile::runner::Namespace::Type,
        [0xe4; 32],
        [0xa4; 32],
        [0xd1; 32],
        &SOURCE_PARAMETERS,
        &SOURCE_RELATIONS,
        [0x90; 32],
        [0x91; 32],
        *aliases[0].selector(),
    )]
}

fn abis() -> Vec<rawr_iced::profile::runner::ExtensionAbi<'static>> {
    use rawr_iced::profile::runner::{ExtensionAbi, ExtensionAbiRole, Namespace};

    let mut rows = [
        (ExtensionAbiRole::Color, "Color", 0_u8, &[][..]),
        (ExtensionAbiRole::Length, "Length", 1, &[][..]),
        (
            ExtensionAbiRole::Rectangle,
            "Rectangle",
            2,
            &ABI_DEFAULT_F32[..],
        ),
        (ExtensionAbiRole::Vector, "Vector", 3, &ABI_DEFAULT_F32[..]),
    ]
    .map(|(role, path, index, arguments)| {
        ExtensionAbi::checked(
            role,
            "iced",
            path,
            Namespace::Type,
            [0xf0 + index; 32],
            [0xb0 + index; 32],
            [0xc0 + index; 32],
            arguments,
        )
    })
    .to_vec();
    rows.sort_unstable_by(|left, right| left.selector().cmp(right.selector()));
    rows
}

fn endpoints(
    source: &rawr_iced::profile::runner::ProjectionSourceSite<'static>,
    projection: &rawr_iced::profile::runner::Projection<'static>,
) -> [rawr_iced::profile::runner::OperationEndpoint<'static>; 1] {
    [rawr_iced::profile::runner::OperationEndpoint::checked(
        "iced",
        "widget::text",
        rawr_iced::profile::runner::Namespace::Value,
        [0xe5; 32],
        [0xa5; 32],
        &OPERATION_FORMALS,
        [0x25; 32],
        *source.selector(),
        *projection.selector(),
        &ENDPOINT_ARGUMENTS,
    )]
}

#[test]
fn linked_profile_decorates_exact_semantics_and_rejects_a_second_owner() {
    let bytes = rawr_import::test_fixture::iced_semantic_facade_plan();
    let limits = rawr_import::cargo::Limits::standard();
    let plan = rawr_import::surface::facade::Plan::decode(&bytes, &limits)
        .expect("the complete semantic facade fixture decodes");
    let alias = plan.aliases().next().expect("one exact Element alias site");
    assert_eq!(plan.aliases().len(), 1);
    assert_eq!(alias.root(), "iced");
    assert_eq!(alias.path(), "Element");
    assert_eq!(alias.parameters().len(), ALIAS_PARAMETERS.len());
    let source = plan
        .projection_sources()
        .next()
        .expect("one Text source site");
    assert_eq!(plan.projection_sources().len(), 1);
    assert_eq!(source.path(), "Text");
    assert_eq!(source.parameters().len(), SOURCE_PARAMETERS.len());
    let iced_plan = plan
        .facades()
        .find(|facade| facade.root() == "iced")
        .expect("the complete fixture retains iced");
    assert!(!iced_plan.types().any(|ty| ty.path() == "Color"));
    assert!(!iced_plan.types().any(|ty| ty.path() == "Length"));
    for (role, identity) in [("Rectangle", [0xb2; 32]), ("Vector", [0xb3; 32])] {
        let ordinary = iced_plan
            .types()
            .find(|ty| ty.path() == role)
            .expect("the generic ordinary declaration coexists with support");
        assert_eq!(ordinary.identity(), &identity);
        assert_eq!(ordinary.parameters().len(), 1);
    }
    assert!(iced_plan.types().any(|ty| ty.path() == "Element"));
    assert_eq!(plan.abi_sites().len(), 4);
    for role in ["Color", "Length", "Rectangle", "Vector"] {
        assert_eq!(
            plan.abi_sites().filter(|site| site.path() == role).count(),
            1
        );
    }
    let aliases = aliases();
    let sources = sources();
    let abis = abis();
    let projections = projections();
    let endpoints = endpoints(&sources[0], &projections[0]);
    let mut engine = rawr::Engine::new();
    let compiler = rawr_iced::profile::runner::install_compiler(
        &mut engine,
        &bytes,
        [0x71; 32],
        [0x70; 32],
        &aliases,
        &sources,
        &abis,
        &endpoints,
        &projections,
    )
    .expect("the exact imported iced facade accepts its linked semantic host");

    let iced = compiler
        .definitions()
        .iter()
        .find(|definition| definition.root() == "iced")
        .expect("the generated owner retains the iced facade");
    let baseline = rawr_iced::facade().expect("the sealed compatibility facade builds");
    for baseline_type in baseline.types() {
        let retained = iced
            .types()
            .filter(|candidate| candidate.path() == baseline_type.path())
            .collect::<Vec<_>>();
        assert_eq!(
            retained.len(),
            1,
            "missing baseline type `{}`",
            baseline_type.path()
        );
        assert_eq!(
            retained[0],
            baseline_type,
            "changed baseline type `{}`",
            baseline_type.path()
        );
    }
    for baseline_item in baseline.items().filter(|item| {
        !matches!(
            item.path().as_str(),
            "iced::Length::FillPortion" | "iced::Length::Fit" | "iced::Length::Shrink"
        )
    }) {
        assert!(
            iced.items().any(|candidate| candidate == baseline_item),
            "missing or changed baseline item `{}`",
            baseline_item.path()
        );
    }
    assert_eq!(
        iced.types()
            .map(|definition| definition.projections().len())
            .sum::<usize>(),
        36,
        "the exact closed compatibility-projection cohort installs"
    );
    assert_eq!(
        iced.items()
            .find(|item| item.path() == "iced::run")
            .map(host::item::Definition::kind),
        Some(host::item::Kind::IntrinsicFunction)
    );
    assert!(compiler
        .definitions()
        .iter()
        .any(|definition| definition.root() == "rawr_iced"));
    {
        let role = "Color";
        assert_eq!(
            iced.types()
                .filter(|ty| ty.path() == format!("iced::{role}"))
                .count(),
            1,
            "the compatibility cohort keeps every linked ordinary ABI role"
        );
        assert!(iced.export_type(role).is_ok());
        assert!(iced.support_type(role).is_ok());
    }
    let length = iced
        .types()
        .find(|ty| ty.path() == "iced::Length")
        .expect("the exact profile-semantic Length is ordinary");
    assert!(length.identity().imported().is_none());
    assert!(length.native_identity().is_some());
    assert!(iced.export_type("Length").is_ok());
    assert!(iced.support_type("Length").is_ok());
    // The facade's whole Length surface is published, as in iced.
    for published in [
        "iced::Length::Fill",
        "iced::Length::Fixed",
        "iced::Length::Fit",
        "iced::Length::Shrink",
        "iced::Length::FillPortion",
    ] {
        assert!(
            iced.items().any(|item| item.path() == published),
            "missing Length item `{published}`"
        );
    }
    for role in ["Rectangle", "Vector"] {
        let ordinary = iced
            .types()
            .find(|ty| ty.path() == format!("iced::{role}"))
            .expect("the linked compatibility declaration remains visible");
        assert!(ordinary.identity().imported().is_none());
        assert!(ordinary.native_identity().is_some());
        assert!(ordinary.parameters().is_empty());
        assert!(iced.export_type(role).is_ok());
        assert!(iced.support_type(role).is_ok());
    }
    engine
        .compile_executable(
            r#"
use iced::Length;
pub fn fill() -> Length { Length::Fill }
pub fn fixed() -> Length { Length::Fixed(24) }
"#,
        )
        .expect("the exact ordinary type and two semantic items compile");
    engine
        .compile_executable(
            r#"
use iced::{Center, Color, Element, Length, Theme};
use iced::widget::{button, checkbox, column, container, row, scrollable, slider, space, text, text_input, toggler};
pub fn complete_gallery_import_surface() {}
"#,
        )
        .expect("the complete existing RawrGallery iced imports survive composition");
    engine
        .compile_executable(
            r#"
use iced::Element;
use iced::widget::text;
enum Message { Tick }
pub fn string() -> Element<Message> { text("Rawr") }
pub fn integer() -> Element<Message> { text(24) }
pub fn boolean() -> Element<Message> { text(true) }
pub fn character() -> Element<Message> { text('R') }
pub fn float() -> Element<Message> { text(2.5) }
pub fn chained() -> Element<Message> { text("Rawr").size(24) }
"#,
        )
        .expect("the endpoint is suppressed and the complete ordinary Text cohort retained");
    engine
        .compile_executable(
            r#"
use iced::{Color, Length};
use rawr_iced::widget::layout::Size;
struct Swatch {}
#[widget]
impl Swatch {
    fn size(&self) -> Size { Size::new(Length::Fill, Length::Fixed(40)) }
    fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
        let bounds = input.bounds();
        renderer.fill_quad(bounds, Color::from_rgb(0.95, 0.45, 0.10));
    }
}
"#,
        )
        .expect("scripted widgets use the ordinary imported-Rust iced types");
    for length in [
        "Length::Fit",
        "Length::Shrink",
        "Length::FillPortion(1)",
        "Length::Fixed(1.5)",
    ] {
        let source = format!("use iced::Length;\npub fn length() -> Length {{ {length} }}");
        assert!(
            engine.compile_executable(source).is_ok(),
            "an imported-Rust project must accept `{length}`"
        );
    }
    assert_eq!(
        iced.types().filter(|ty| ty.path() == "iced::Text").count(),
        0,
    );

    let mut runtime_builders = compiler
        .definitions()
        .iter()
        .cloned()
        .map(host::runtime::Builder::new)
        .collect::<Vec<_>>();
    rawr_iced::profile::runner::bind_runtime(
        compiler.definitions(),
        &mut runtime_builders,
        &projections,
    )
    .expect("the complete compatibility runtime ledger binds");
    let iced_runtime = compiler
        .definitions()
        .iter()
        .position(|definition| definition.root() == "iced")
        .expect("one iced runtime owner");
    runtime_builders[iced_runtime]
        .bind_type_attestation(
            "iced::Element",
            rawr_iced::profile::runner::attest_support_element,
        )
        .expect("the sole generated Element replacement installs");
    let mut registry = host::runtime::Registry::new();
    for builder in runtime_builders {
        registry
            .install(builder.finish())
            .expect("the composed runtime facades install in dependency order");
    }

    let duplicate = rawr_iced::profile::runner::install_compiler(
        &mut engine,
        &bytes,
        [0x71; 32],
        [0x70; 32],
        &aliases,
        &sources,
        &abis,
        &endpoints,
        &projections,
    );
    assert!(
        duplicate.is_err(),
        "a second iced compiler owner must fail closed"
    );
}

#[test]
fn linked_profile_rejects_a_wrong_same_path_ordinary_identity() {
    let bytes = rawr_import::test_fixture::iced_semantic_facade_plan();
    let mut document: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the fixture is JSON");
    let rectangle = document["facades"]
        .as_array_mut()
        .expect("facade rows")
        .iter_mut()
        .find(|facade| facade["root"] == "iced")
        .and_then(|facade| facade["types"].as_array_mut())
        .and_then(|types| types.iter_mut().find(|ty| ty["path"] == "Rectangle"))
        .expect("one ordinary Rectangle declaration");
    rectangle["identity"] = serde_json::Value::Array(
        [0x44_u8; 32]
            .into_iter()
            .map(serde_json::Value::from)
            .collect(),
    );
    let bytes = serde_json::to_vec(&document).expect("the forged plan re-encodes");
    rawr_import::surface::facade::Plan::decode(&bytes, &rawr_import::cargo::Limits::standard())
        .expect("the generic plan vocabulary admits a distinct imported identity");

    let mut engine = rawr::Engine::new();
    let sources = sources();
    let projections = projections();
    let endpoints = endpoints(&sources[0], &projections[0]);
    let error = match rawr_iced::profile::runner::install_compiler(
        &mut engine,
        &bytes,
        [0x73; 32],
        [0x70; 32],
        &aliases(),
        &sources,
        &abis(),
        &endpoints,
        &projections,
    ) {
        Err(error) => error,
        Ok(_) => panic!("the profile support proof must match the ordinary identity"),
    };
    let rawr_iced::profile::runner::Error::Import(
        rawr::package::import::runner::Error::FacadeEntry { source, .. },
    ) = &error
    else {
        panic!("{error:?}")
    };
    assert!(matches!(
        source.as_ref(),
        rawr::package::import::runner::Error::Host(host::Error::ConflictingHostIdentity { .. })
    ));
}

#[test]
fn neutral_profile_cannot_authorize_the_linked_host() {
    let mut engine = rawr::Engine::new();
    let bytes = rawr_import::test_fixture::iced_semantic_facade_plan();
    let aliases = aliases();
    let sources = sources();
    let abis = abis();
    let projections = projections();
    let endpoints = endpoints(&sources[0], &projections[0]);
    let neutral = *rawr_import::profile::Proof::neutral().fingerprint();
    assert!(matches!(
        rawr_iced::profile::runner::install_compiler(
            &mut engine,
            &bytes,
            neutral,
            [0x70; 32],
            &aliases,
            &sources,
            &abis,
            &endpoints,
            &projections,
        ),
        Err(rawr_iced::profile::runner::Error::NeutralProfile)
    ));
}

#[test]
fn exact_custom_widget_projection_orders_and_links_its_iced_target() {
    let bytes = rawr_import::test_fixture::iced_projection_facade_plan();
    let limits = rawr_import::cargo::Limits::standard();
    let plan = rawr_import::surface::facade::Plan::decode(&bytes, &limits)
        .expect("the complete cross-root facade fixture decodes");
    let alias = plan.aliases().next().expect("one exact Element alias site");
    assert_eq!(plan.aliases().len(), 1);
    assert_eq!(alias.root(), "iced");
    assert_eq!(alias.path(), "Element");
    assert_eq!(alias.parameters().len(), ALIAS_PARAMETERS.len());
    let source = plan
        .projection_sources()
        .next()
        .expect("one Dial source site");
    assert_eq!(plan.projection_sources().len(), 1);
    assert_eq!(source.path(), "Dial");
    assert_eq!(source.parameters().len(), SOURCE_PARAMETERS.len());
    let iced_plan = plan
        .facades()
        .find(|facade| facade.root() == "iced")
        .expect("the cross-root fixture retains iced");
    assert!(!iced_plan.types().any(|ty| ty.path() == "Color"));
    assert!(iced_plan.types().any(|ty| ty.path() == "Rectangle"));
    assert!(iced_plan.types().any(|ty| ty.path() == "Vector"));
    assert!(iced_plan.types().any(|ty| ty.path() == "Element"));
    assert_eq!(plan.abi_sites().len(), 4);
    let aliases = aliases();
    let sources = cross_sources();
    let abis = abis();
    let projections = cross_projections();
    let endpoints = endpoints(&sources[0], &projections[0]);
    let mut engine = rawr::Engine::new();
    let compiler = rawr_iced::profile::runner::install_compiler(
        &mut engine,
        &bytes,
        [0x72; 32],
        [0x70; 32],
        &aliases,
        &sources,
        &abis,
        &endpoints,
        &projections,
    )
    .expect("the exact cross-facade proof orders iced before its widget owner");

    let roots = compiler
        .definitions()
        .iter()
        .map(host::facade::Definition::root)
        .collect::<Vec<_>>();
    assert_eq!(roots, ["iced", "dial", "rawr_iced"]);
    let dial = compiler
        .definitions()
        .iter()
        .find(|definition| definition.root() == "dial")
        .expect("the generated custom widget facade exists");
    let projection = dial
        .types()
        .find(|ty| ty.path() == "dial::Dial")
        .and_then(|ty| ty.projections().first())
        .expect("the exact custom widget owns one direct projection");
    assert_eq!(projection.target(), "iced::Element");
}
