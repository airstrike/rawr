use rawr::host::{facade, item, signature, stream, type_ref};
use rawr::{Engine, ParseErrorType, resolve};

#[test]
fn direct_stream_helpers_lower_exact_zero_to_n_overloads() {
    let engine = engine();
    let source = r#"
use iced::Subscription;
use effects::{ticks, combine};
pub fn zero() -> Subscription<String> { ticks() }
pub fn one() -> Subscription<i64> { ticks(7) }
pub fn many() -> Subscription<String> { combine(7, " ticks") }
"#;

    engine.compile_executable(source).unwrap();
}

#[test]
fn stream_helpers_reject_wrong_types_and_arity() {
    let engine = engine();
    for source in [
        "use iced::Subscription; use effects::ticks; pub fn bad() -> Subscription<i64> { ticks(true) }",
        "use iced::Subscription; use effects::combine; pub fn bad() -> Subscription<String> { combine(1) }",
        "use iced::Subscription; use effects::combine; pub fn bad() -> Subscription<String> { combine(1, 2) }",
    ] {
        let error = engine.compile_executable(source).unwrap_err();
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn one_shot_and_stream_operation_positions_remain_distinct() {
    let engine = engine();
    let direct_one_shot = engine.compile_executable("use effects::load; pub fn bad() { load(1); }").unwrap_err();
    assert!(matches!(
        direct_one_shot.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { item, .. })
            if item == "effects::load"
    ));

    let streamed_task = engine
        .compile_executable(
            r#"
use iced::Task;
use effects::ticks;
enum Message { Tick(i64) }
pub fn tick(value: i64) -> Message { Message::Tick(value) }
pub fn bad() -> Task<Message> { Task::perform(ticks(1), tick) }
"#,
        )
        .unwrap_err();
    assert!(matches!(streamed_task.err_type(), ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { .. })));
    assert!(!streamed_task.primary_span().is_none());
}

#[test]
fn stream_calls_require_one_authenticated_plan_role() {
    let mut engine = Engine::new();
    engine.register_host_facade(streams_without_plan()).unwrap();
    let error = engine.compile_executable("use effects::ticks; pub fn bad() { ticks(); }").unwrap_err();

    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { reason, .. })
            if reason.contains("one exact facade-authenticated stream plan type")
    ));
}

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine.register_host_facade(iced_facade()).unwrap();
    engine.register_host_facade(effects_facade()).unwrap();
    engine
}

fn iced_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    builder.stream_plan_type("Subscription", "Message").unwrap();
    builder.required_type::<Task>("Task", &["Message"], &[]).unwrap();
    let mut perform = signature::Builder::new();
    perform.type_parameter("Output", &[]).unwrap();
    perform.type_parameter("Message", &[]).unwrap();
    perform.parameter(type_ref::Type::parameter("Output").unwrap());
    perform.parameter(type_ref::Type::callable(type_ref::Type::parameter("Output").unwrap(), type_ref::Type::parameter("Message").unwrap()));
    perform.output(type_ref::Type::applied("Task", [type_ref::Type::parameter("Message").unwrap()]).unwrap());
    builder.item(item::Definition::associated_function("Task::perform", perform.finish()).unwrap()).unwrap();
    builder.finish().unwrap()
}

fn effects_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("effects", facade::Abi::new(1).unwrap()).unwrap();
    declarations(&mut builder);
    builder.finish().unwrap()
}

fn streams_without_plan() -> facade::Definition {
    let mut builder = facade::Builder::new("effects", facade::Abi::new(1).unwrap()).unwrap();
    declarations(&mut builder);
    builder.finish().unwrap()
}

fn declarations(builder: &mut facade::Builder) {
    let mut ticks = signature::Builder::new();
    ticks.output(type_ref::Type::builtin(type_ref::Builtin::String));
    builder.item(item::Definition::stream_operation("ticks", ticks.finish(), stream::Version::new(1)).unwrap()).unwrap();

    let mut ticks = signature::Builder::new();
    ticks.parameter(type_ref::Type::builtin(type_ref::Builtin::I64)).output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::stream_operation("ticks", ticks.finish(), stream::Version::new(1)).unwrap()).unwrap();

    let mut combine = signature::Builder::new();
    combine.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    combine.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    combine.output(type_ref::Type::builtin(type_ref::Builtin::String));
    builder.item(item::Definition::stream_operation("combine", combine.finish(), stream::Version::new(3)).unwrap()).unwrap();

    let mut load = signature::Builder::new();
    load.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    load.output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::operation("load", load.finish()).unwrap()).unwrap();
}

struct Task;
