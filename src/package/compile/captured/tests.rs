use super::{Error, Output};
use crate::{capability, host, package, Engine};
use rawr_import::classify::representability::convert::{Builtin, Type};
use rawr_import::surface::identity::{Definition, Selector};

#[derive(Clone)]
struct ResultValue;
#[derive(Clone)]
struct Element;
#[derive(Clone)]
struct Task;
#[derive(Clone)]
struct Theme;

fn iced_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<ResultValue>("Result", &[], &[]).unwrap();
    builder.type_::<Task>("Task", &["Message"], &[]).unwrap();
    builder.type_::<Theme>("Theme", &[], &[]).unwrap();
    builder
        .type_::<Element>("Element", &["Message"], &[])
        .unwrap();
    let mut run = host::signature::Builder::new();
    run.type_parameter("State", &[]).unwrap();
    run.type_parameter(
        "Message",
        &[
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )
    .unwrap();
    run.type_parameter("Update", &[]).unwrap();
    run.type_parameter("View", &[]).unwrap();
    run.parameter(host::type_ref::Type::parameter("Update").unwrap());
    run.parameter(host::type_ref::Type::parameter("View").unwrap());
    run.output(host::type_ref::Type::registered("Result").unwrap());
    builder
        .item(host::item::Definition::intrinsic_function("run", run.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

pub(crate) fn preview_runtime() -> host::runtime::Registry {
    let mut builder = host::runtime::Builder::new(iced_facade());
    builder
        .bind_type_attestation("iced::Element", |value, expected| {
            expected
                .attest(value.is::<Element>() && expected.argument_name(0).as_deref() == Some("()"))
        })
        .unwrap();
    let mut runtime = host::runtime::Registry::new();
    runtime.install(builder.finish()).unwrap();
    runtime
}

pub(crate) fn compiler() -> Engine {
    let facade = iced_facade();
    let mut engine = Engine::new();
    engine.register_host_facade(facade).unwrap();

    let mut native =
        host::facade::Builder::new("native", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut echo = host::signature::Builder::new();
    echo.type_parameter("T", &[]).unwrap();
    echo.parameter(host::type_ref::Type::parameter("T").unwrap());
    echo.output(host::type_ref::Type::parameter("T").unwrap());
    native
        .item(
            host::item::Definition::function("echo", echo.finish())
                .unwrap()
                .imported(Selector::Function {
                    item: Definition::authenticated([41; 32]),
                }),
        )
        .unwrap();
    engine
        .register_host_facade(native.finish().unwrap())
        .unwrap();
    engine
}

pub(crate) fn source(label: &str) -> String {
    format!(
        r#"
#[derive(Default)] struct State {{ value: i64 }}
#[derive(Clone)] enum Message {{ Tick }}
impl State {{
    fn update(&mut self, message: Message) {{ self.value = native::echo(7); }}
    fn view(&self) -> iced::Element<Message> {{ panic!("{label}") }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
    )
}

pub(crate) fn snapshot(source: &str) -> package::snapshot::Snapshot {
    let path = std::path::PathBuf::from("/resident-fixture/main.rs");
    package::snapshot::Snapshot::new(path.clone(), vec![(path, source.into())]).unwrap()
}

pub(crate) fn native_snapshot(source: &str) -> package::snapshot::Snapshot {
    let root = std::path::Path::new("/resident-fixture");
    let entry = root.join("src/main.rs");
    package::snapshot::Snapshot::new(
        entry.clone(),
        vec![
            (
                root.join("Cargo.toml"),
                "[package]\nname = \"resident\"\n[dependencies]\nnative = { path = \"native\" }\n"
                    .into(),
            ),
            (entry, source.into()),
        ],
    )
    .unwrap()
}

#[test]
fn snapshot_pass_retains_candidate_and_native_uses_together() {
    let engine = compiler();
    let text = source("Foo");
    let snapshot = snapshot(&text);
    let mut allocation = host::TestImportAllocation::default();
    let output = Output::transform_plain(&engine, &snapshot, &mut allocation).unwrap();
    assert_eq!(output.candidate.source().entry_source().text(), text);
    assert_eq!(output.imported.uses.len(), 1);
    let use_ = &output.imported.uses[0];
    assert_eq!(use_.path, "native::echo");
    assert_eq!(&*use_.types, &[Type::Builtin(Builtin::I64)]);
    assert!(allocation.usage().0 > 0 && allocation.usage().1 > 0);

    let generation = output.candidate.id();
    let (candidate, _ast) = output.candidate.with_top_level();
    assert_eq!(candidate.id(), generation);
}

#[test]
fn body_edit_keeps_native_requirements_but_retains_fresh_source() {
    let engine = compiler();
    let first = Output::transform_plain(
        &engine,
        &snapshot(&source("Foo")),
        &mut host::TestImportAllocation::default(),
    )
    .unwrap();
    let edited = source("Bar");
    let next = Output::transform_plain(
        &engine,
        &snapshot(&edited),
        &mut host::TestImportAllocation::default(),
    )
    .unwrap();
    assert_eq!(first.imported.uses, next.imported.uses);
    assert_ne!(first.candidate.id(), next.candidate.id());
    assert_eq!(next.candidate.source().entry_source().text(), edited);
}

#[test]
fn resource_refusal_is_not_a_source_or_native_preparation_miss() {
    let engine = compiler();
    let error = match Output::transform_plain(
        &engine,
        &snapshot(&source("Foo")),
        &mut host::TestImportAllocation::new(0, 0),
    ) {
        Ok(_) => panic!("zero retained budget must refuse imported-use capture"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::Resource(_)));
}

#[test]
fn type_error_keeps_the_actual_snapshot_diagnostic() {
    let engine = compiler();
    let text = source("Foo").replace("native::echo(7)", "native::echo(true)");
    let error = match Output::transform_plain(
        &engine,
        &snapshot(&text),
        &mut host::TestImportAllocation::default(),
    ) {
        Ok(_) => panic!("bool cannot replace an i64 field"),
        Err(error) => error,
    };
    let Error::Source(error) = error else {
        panic!("type error must remain a source diagnostic")
    };
    let report = error.report().unwrap();
    assert!(report.sources().iter().any(|source| source.text() == text));
    assert!(report.message().contains("type"));
}
