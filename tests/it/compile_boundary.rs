use rawr::{Dynamic, Engine, ParseErrorType, Scope};

#[derive(Clone)]
struct HostMessage;

#[derive(Clone)]
enum Message {
    Tick,
}

#[test]
fn ordinary_scripts_compile_and_evaluate_through_resolution() {
    let engine = Engine::new();
    let ast = engine.compile("let answer = 40 + 2; answer").unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn scoped_compilation_preserves_constant_propagation() {
    let engine = Engine::new();
    let mut scope = Scope::new();
    scope.push_constant("answer", 42_i64);
    let ast = engine.compile_with_scope(&scope, "answer").unwrap();

    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}

#[test]
fn parsed_units_expose_syntax_but_not_an_executable_ast() {
    let engine = Engine::new();
    let unit = engine.parse_unit("struct Counter; 40 + 2").unwrap();

    assert_eq!(unit.items().len(), 1);
    assert_eq!(unit.source().text(), "struct Counter; 40 + 2");
}

#[test]
fn unavailable_item_lowering_is_a_positioned_compile_error() {
    let engine = Engine::new();
    let error = engine.compile("let answer = 42;\npub fn pending() { if true { 42 } }").unwrap_err();

    // As in Rust, an `if` without `else` has type `()`.
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(rawr::resolve::Error::TypeMismatch { .. })));
    assert_eq!(error.position().line(), Some(2));
    assert!(error.to_string().contains("expected `()`, found `{integer}`"), "{error}");
}

#[test]
fn canonical_compile_reports_duplicate_items_and_unknown_header_types() {
    let engine = Engine::new();

    let duplicate = engine.compile("struct Counter;\nstruct Counter;").unwrap_err();
    assert!(matches!(
        duplicate.err_type(),
        ParseErrorType::Resolution(rawr::resolve::Error::DuplicateItem { name, .. })
            if name == "Counter"
    ));
    assert_eq!(duplicate.position().line(), Some(2));

    let unknown = engine.compile("struct Envelope {\n    message: Missing,\n}").unwrap_err();
    assert!(matches!(
        unknown.err_type(),
        ParseErrorType::Resolution(rawr::resolve::Error::UnknownType { name, .. })
            if name == "Missing"
    ));
    assert_eq!(unknown.position().line(), Some(2));
    assert_eq!(unknown.position().position(), Some(14));
}

#[test]
fn engine_type_registration_is_visible_to_header_resolution() {
    let mut engine = Engine::new();
    engine.register_type_with_name::<HostMessage>("HostMessage");

    let ast = engine.compile("pub fn handle(message: HostMessage) { () }").unwrap();
    assert!(ast.iter_functions().any(|function| function.name == "handle"));
}

#[test]
fn enum_switch_and_new_function_behavior_remain_available() {
    let mut engine = Engine::new();
    engine.register_enum::<Message>("Message", &[("Tick", 0)], |message| match message {
        Message::Tick => (0, Vec::<Dynamic>::new()),
    });
    let mut scope = Scope::new();
    scope.push("message", Message::Tick);

    assert_eq!(engine.eval_with_scope::<i64>(&mut scope, "fn new() { 42 } switch message { Message::Tick => new(), _ => 0 }",).unwrap(), 42);
}

/// `use rawr::preview;` is for rustc: it is no item, and the legacy script
/// does not see it.
#[test]
fn an_import_of_rawr_attributes_is_neither_item_nor_script() {
    let engine = Engine::new();
    let unit = engine.parse_unit("use rawr::{preview, widget};\n40 + 2").unwrap();

    assert!(unit.items().is_empty());
    let ast = engine.compile("use rawr::preview;\n40 + 2").unwrap();
    assert_eq!(engine.eval_ast::<i64>(&ast).unwrap(), 42);
}
