use rawr::{capability, host, Engine, Shared};

#[test]
fn referenced_source_facade_abi_and_unrelated_declarations_change_the_contract() {
    let base = compile_projection(charts_facade(1, false, false));
    let abi = compile_projection(charts_facade(2, false, false));
    let unrelated = compile_projection(charts_facade(1, true, false));

    assert_ne!(base.fingerprint(), abi.fingerprint());
    assert_ne!(base.fingerprint(), unrelated.fingerprint());
    assert_ne!(dependency(&base, "charts"), dependency(&abi, "charts"));
    assert_ne!(
        dependency(&base, "charts"),
        dependency(&unrelated, "charts")
    );
}

#[test]
fn dependency_sets_are_canonical_repeated_and_ignore_unreferenced_facades() {
    let ordered = compile_projection(charts_facade(1, true, false));
    let reordered = compile_projection(charts_facade(1, true, true));
    assert_eq!(ordered.fingerprint(), reordered.fingerprint());
    assert_eq!(ordered.dependencies(), reordered.dependencies());

    let mut first = Engine::new();
    rawr_iced::install(&mut first).unwrap();
    first.register_host_facade(auxiliary_facade(1)).unwrap();
    let first = first
        .compile_application_descriptor(direct_value_source())
        .unwrap();

    let mut second = Engine::new();
    rawr_iced::install(&mut second).unwrap();
    second.register_host_facade(auxiliary_facade(2)).unwrap();
    let second = second
        .compile_application_descriptor(direct_value_source())
        .unwrap();

    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(roots(&first), ["iced"]);
    assert_eq!(first.dependencies(), second.dependencies());

    let mut repeated = Engine::new();
    rawr_iced::install(&mut repeated).unwrap();
    repeated
        .register_host_facade(charts_facade(1, false, false))
        .unwrap();
    let first = repeated
        .compile_application_descriptor(projection_source())
        .unwrap();
    let second = repeated
        .compile_application_descriptor(projection_source())
        .unwrap();
    assert_eq!(first.dependencies(), second.dependencies());
    assert_eq!(first.fingerprint(), second.fingerprint());
}

#[test]
fn projection_and_nested_type_dependencies_are_exact_and_deduplicated() {
    let projected = compile_projection(charts_facade(1, false, false));
    assert_eq!(roots(&projected), ["charts", "iced"]);

    let mut direct = Engine::new();
    rawr_iced::install(&mut direct).unwrap();
    direct
        .register_host_facade(charts_facade(1, false, false))
        .unwrap();
    let direct = direct
        .compile_application_descriptor(direct_value_source())
        .unwrap();
    assert_eq!(roots(&direct), ["iced"]);

    let mut nested = Engine::new();
    rawr_iced::install(&mut nested).unwrap();
    nested.register_host_facade(message_facade()).unwrap();
    let nested = nested
        .compile_application_descriptor(nested_message_source())
        .unwrap();
    assert_eq!(roots(&nested), ["iced", "native"]);

    let mut exact = Engine::new();
    rawr_iced::install(&mut exact).unwrap();
    let exact = exact
        .compile_application_descriptor(include_str!(
            "../../examples/scripts/application_counter.rs"
        ))
        .unwrap();
    assert_eq!(roots(&exact), ["iced"]);
}

#[test]
fn retained_intrinsic_operations_contribute_their_owning_facade() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(metrics_facade()).unwrap();

    let descriptor = engine
        .compile_application_descriptor(intrinsic_dependency_source())
        .unwrap();
    assert_eq!(roots(&descriptor), ["charts", "iced"]);
}

#[test]
fn dependencies_survive_candidate_descriptor_and_launch_without_executable_access() {
    let mut engine = Engine::new();
    let iced = rawr_iced::facade().unwrap();
    let charts = charts_facade_from(&iced, 1, false, false);
    engine.register_host_facade(iced).unwrap();
    engine.register_host_facade(charts).unwrap();
    let engine = Shared::new(engine);

    let candidate = engine.compile_application(projection_source()).unwrap();
    let descriptor = candidate.validate().unwrap();
    let retained = descriptor.dependencies().to_vec();
    assert_eq!(roots(&descriptor), ["charts", "iced"]);

    let launch = engine.launch_application(descriptor).unwrap();
    assert_eq!(launch.dependencies(), retained);
}

fn compile_projection(charts: host::facade::Definition) -> rawr::application::Descriptor {
    let mut engine = Engine::new();
    let iced = rawr_iced::facade().unwrap();
    engine.register_host_facade(iced).unwrap();
    engine.register_host_facade(charts).unwrap();
    engine
        .compile_application_descriptor(projection_source())
        .unwrap()
}

fn charts_facade(abi: u32, unrelated: bool, reverse: bool) -> host::facade::Definition {
    let iced = rawr_iced::facade().unwrap();
    charts_facade_from(&iced, abi, unrelated, reverse)
}

fn charts_facade_from(
    iced: &host::facade::Definition,
    abi: u32,
    unrelated: bool,
    reverse: bool,
) -> host::facade::Definition {
    let target = iced.export_type("Element").unwrap();
    let mut builder =
        host::facade::Builder::new("charts", host::facade::Abi::new(abi).unwrap()).unwrap();
    if reverse && unrelated {
        builder
            .item(
                host::item::Definition::value(
                    "unused",
                    host::type_ref::Type::builtin(host::type_ref::Builtin::I64),
                )
                .unwrap(),
            )
            .unwrap();
    }
    builder.type_::<Plot>("Plot", &["Message"], &[]).unwrap();
    builder.project_to("Plot", target, &["Message"]).unwrap();
    if !reverse && unrelated {
        builder
            .item(
                host::item::Definition::value(
                    "unused",
                    host::type_ref::Type::builtin(host::type_ref::Builtin::I64),
                )
                .unwrap(),
            )
            .unwrap();
    }
    builder.finish().unwrap()
}

fn auxiliary_facade(abi: u32) -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("aux", host::facade::Abi::new(abi).unwrap()).unwrap();
    builder.type_::<Aux>("Aux", &[], &[]).unwrap();
    builder.finish().unwrap()
}

fn message_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("native", host::facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<Opaque>(
            "Opaque",
            &[],
            &[
                capability::Kind::Clone,
                capability::Kind::Send,
                capability::Kind::Static,
            ],
        )
        .unwrap();
    builder.finish().unwrap()
}

fn metrics_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("charts", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = host::signature::Builder::new();
    signature.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    signature.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::function("metric", signature.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn roots(descriptor: &rawr::application::Descriptor) -> Vec<&str> {
    descriptor
        .dependencies()
        .iter()
        .map(host::facade::Dependency::root)
        .collect()
}

fn dependency<'a>(
    descriptor: &'a rawr::application::Descriptor,
    root: &str,
) -> &'a host::facade::Dependency {
    descriptor
        .dependencies()
        .iter()
        .find(|dependency| dependency.root() == root)
        .unwrap()
}

fn projection_source() -> &'static str {
    r#"
#[derive(Default)] struct Counter;
#[derive(Clone)] enum Message { Tick }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> charts::Plot<Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn direct_value_source() -> &'static str {
    r#"
#[derive(Default)] struct Counter;
#[derive(Clone)] enum Message { Tick }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> iced::Element<Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn nested_message_source() -> &'static str {
    r#"
use native::Opaque;
#[derive(Default)] struct Counter;
#[derive(Clone)] enum Message { Data(Opaque) }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> iced::Element<Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn intrinsic_dependency_source() -> &'static str {
    r#"
use charts::metric;
use iced::widget::{Column, column, text};
#[derive(Default)] struct Counter { value: i64 }
#[derive(Clone)] enum Message { Tick }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> Column<'_, Message> {
        column![text(metric(self.value))]
    }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

struct Plot;
struct Aux;
struct Opaque;
