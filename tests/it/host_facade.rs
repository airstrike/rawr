use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{Engine, ParseErrorType, Position, capability, receiver, resolve};

#[test]
fn facade_finish_produces_order_independent_structural_proofs() {
    let source_order = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Shared);
    let reverse_order = counter_facade(true, facade::Abi::new(1).unwrap(), receiver::Mode::Shared);

    assert_eq!(source_order.fingerprint(), reverse_order.fingerprint());
    assert_eq!(source_order.types().len(), 2);
    assert_eq!(source_order.items().len(), 2);
}

#[test]
fn facade_module_and_value_namespaces_coexist_and_fingerprint_independently() {
    let module_first = function_module_facade(false);
    let item_first = function_module_facade(true);

    assert_eq!(module_first.fingerprint(), item_first.fingerprint());
    assert!(module_first.items().any(|item| item.path() == "api::widget::button"));
    assert!(module_first.items().any(|item| item.path() == "api::widget::button::primary"));

    let mut changed = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    changed.module("widget").unwrap();
    changed.module("widget::button").unwrap();
    let mut button = signature::Builder::new();
    button
        .parameter(type_ref::Type::builtin(type_ref::Builtin::String))
        .output(type_ref::Type::builtin(type_ref::Builtin::I64));
    changed.item(item::Definition::function("widget::button", button.finish()).unwrap()).unwrap();
    changed
        .item(item::Definition::value("widget::button::primary", type_ref::Type::builtin(type_ref::Builtin::I64)).unwrap())
        .unwrap();
    assert_ne!(module_first.fingerprint(), changed.finish().unwrap().fingerprint());
}

#[test]
fn one_import_uses_the_item_directly_and_the_module_for_qualified_members() {
    let mut engine = Engine::new();
    engine.register_host_facade(function_module_facade(false)).unwrap();

    for source in [
        r#"
use api::widget::button;
pub fn styled() -> i64 {
    let style = button::primary;
    button(style)
}
"#,
        r#"
use api::widget::{button};
pub fn styled() -> i64 {
    let style = button::primary;
    button(style)
}
"#,
        r#"
use api::widget::button::{primary};
use api::widget::button;
pub fn styled() -> i64 { button(primary) }
"#,
    ] {
        engine.compile(source).unwrap();
    }
}

#[test]
fn one_name_is_a_macro_a_function_and_a_module_by_call_syntax() {
    let mut engine = Engine::new();
    engine.register_host_facade(three_namespace_facade()).unwrap();
    let facade = three_namespace_facade();
    assert_eq!(facade.items().filter(|item| item.path() == "api::widget::thing").count(), 2);

    // The macro yields a String, the function an i64, the module a constant:
    // each line type-checks only when its syntax picks its own namespace.
    for source in [
        r#"
use api::widget::thing;
pub fn all() -> i64 {
    let label: String = thing![1, 2, 3];
    let sum: i64 = thing(1, 2);
    sum + thing::LIMIT + label.len() as i64
}
"#,
        r#"
pub fn qualified() -> i64 {
    let label: String = api::widget::thing![1];
    api::widget::thing(1, 2) + api::widget::thing::LIMIT + label.len() as i64
}
"#,
    ] {
        engine.compile(source).unwrap();
    }

    // A name in one namespace says which syntax it takes.
    let mut builder = facade::Builder::new("ui", facade::Abi::new(1).unwrap()).unwrap();
    let mut list = signature::Builder::new();
    list.parameter(type_ref::Type::builtin(type_ref::Builtin::I64)).output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::intrinsic_macro("list", list.finish()).unwrap()).unwrap();
    let mut call = signature::Builder::new();
    call.parameter(type_ref::Type::builtin(type_ref::Builtin::I64)).output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::function("make", call.finish()).unwrap()).unwrap();
    let mut engine = Engine::new();
    engine.register_host_facade(builder.finish().unwrap()).unwrap();
    let error = engine.compile("use ui::list; pub fn f() -> i64 { list(1) }").unwrap_err();
    assert!(error.to_string().contains("`list` is a macro here; write `list![..]`"), "{error}");
    let error = engine.compile("use ui::make; pub fn f() -> i64 { make![1] }").unwrap_err();
    assert!(error.to_string().contains("`make` is a function here; write `make(..)`"), "{error}");
}

#[test]
fn a_macro_and_a_function_of_one_name_are_not_overloads() {
    let mut builder = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    builder.module("widget").unwrap();
    // The same inputs in both namespaces are no duplicate.
    let mut list = signature::Builder::new();
    list.parameter(type_ref::Type::builtin(type_ref::Builtin::I64)).output(type_ref::Type::builtin(type_ref::Builtin::I64));
    let list = list.finish();
    builder.item(item::Definition::intrinsic_macro("widget::thing", list.clone()).unwrap()).unwrap();
    builder.item(item::Definition::function("widget::thing", list.clone()).unwrap()).unwrap();
    builder.finish().unwrap();

    // Within the value namespace the overload rules still hold.
    let mut builder = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    builder.module("widget").unwrap();
    builder.item(item::Definition::intrinsic_macro("widget::thing", list.clone()).unwrap()).unwrap();
    builder.item(item::Definition::function("widget::thing", list.clone()).unwrap()).unwrap();
    assert!(matches!(
        builder.item(item::Definition::intrinsic_function("widget::thing", list).unwrap()),
        Err(host::Error::IncompatibleFacadeOverload { .. })
    ));
}

fn three_namespace_facade() -> facade::Definition {
    let mut builder = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    builder.module("widget").unwrap();
    builder.module("widget::thing").unwrap();
    let mut list = signature::Builder::new();
    list.parameter(type_ref::Type::builtin(type_ref::Builtin::I64)).output(type_ref::Type::builtin(type_ref::Builtin::String));
    builder.item(item::Definition::intrinsic_macro("widget::thing", list.finish()).unwrap()).unwrap();
    let mut call = signature::Builder::new();
    call.parameter(type_ref::Type::builtin(type_ref::Builtin::I64))
        .parameter(type_ref::Type::builtin(type_ref::Builtin::I64))
        .output(type_ref::Type::builtin(type_ref::Builtin::I64));
    builder.item(item::Definition::function("widget::thing", call.finish()).unwrap()).unwrap();
    builder
        .item(item::Definition::value("widget::thing::LIMIT", type_ref::Type::builtin(type_ref::Builtin::I64)).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

#[test]
fn module_type_and_type_item_collisions_remain_rejected() {
    let mut module_type = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    module_type.module("widget").unwrap();
    assert!(matches!(module_type.type_::<Value>("widget", &[], &[]), Err(host::Error::DuplicateFacadeEntry { existing: "module", incoming: "type", .. })));

    let mut type_item = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    type_item.type_::<Value>("button", &[], &[]).unwrap();
    assert!(matches!(
        type_item.item(item::Definition::value("button", type_ref::Type::builtin(type_ref::Builtin::I64),).unwrap()),
        Err(host::Error::DuplicateFacadeEntry { existing: "type", incoming: "item", .. })
    ));
}

#[test]
fn facade_overloads_are_order_independent_and_retain_each_exact_input() {
    let integer_first = overload_facade(false);
    let string_first = overload_facade(true);

    assert_eq!(integer_first.fingerprint(), string_first.fingerprint());
    let inputs = integer_first
        .items()
        .filter(|item| item.path() == "iced::Value::key")
        .map(|item| item.signature().parameters()[0].clone())
        .collect::<Vec<_>>();
    assert_eq!(inputs.len(), 2);
    assert!(inputs.contains(&type_ref::Type::builtin(type_ref::Builtin::I64)));
    assert!(inputs.contains(&type_ref::Type::builtin(type_ref::Builtin::String)));
}

#[test]
fn facade_overloads_reject_duplicate_return_only_and_incompatible_declarations() {
    let duplicate = overload_builder(false);
    assert!(matches!(
        duplicate.finish(),
        Err(host::Error::DuplicateFacadeOverloadInputs { item })
            if item == "iced::Value::key"
    ));

    let return_only = overload_builder(true);
    assert!(matches!(
        return_only.finish(),
        Err(host::Error::DuplicateFacadeOverloadInputs { item })
            if item == "iced::Value::key"
    ));

    let mut incompatible = overload_builder_base();
    let mut integer = signature::Builder::new();
    integer.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    incompatible.item(item::Definition::method("Value::key", receiver::Mode::Owned, integer.finish()).unwrap()).unwrap();
    let mut input = signature::Builder::new();
    input.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    assert!(matches!(
        incompatible.item(
            item::Definition::intrinsic_method(
                "Value::key",
                receiver::Mode::Owned,
                input.finish(),
            )
            .unwrap()
        ),
        Err(host::Error::IncompatibleFacadeOverload { item, .. })
            if item == "iced::Value::key"
    ));
}

#[test]
fn facade_overloads_treat_exact_parameter_capabilities_as_input_contracts() {
    let mut builder = overload_builder_base();
    for requirements in [&[capability::Kind::Clone][..], &[capability::Kind::Clone, capability::Kind::Copy][..]] {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Key", requirements).unwrap();
        signature.parameter(type_ref::Type::parameter("Key").unwrap()).output(type_ref::Type::registered("Value").unwrap());
        builder.item(item::Definition::method("Value::key", receiver::Mode::Owned, signature.finish()).unwrap()).unwrap();
    }

    let facade = builder.finish().unwrap();
    assert_eq!(facade.items().filter(|item| item.path() == "iced::Value::key").count(), 2);
}

#[test]
fn facade_proofs_change_with_abi_signature_receiver_kind_and_capabilities() {
    let baseline = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Shared);
    let changed_abi = counter_facade(false, facade::Abi::new(2).unwrap(), receiver::Mode::Shared);
    let changed_receiver = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Mutable);
    let owned_receiver = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Owned);
    let changed_signature = counter_facade_with(false, true, false);
    let changed_capabilities = counter_facade_with(false, false, true);

    assert_ne!(baseline.fingerprint(), changed_abi.fingerprint());
    assert_ne!(baseline.fingerprint(), changed_receiver.fingerprint());
    assert_ne!(baseline.fingerprint(), owned_receiver.fingerprint());
    assert_ne!(baseline.fingerprint(), changed_signature.fingerprint());
    assert_ne!(baseline.fingerprint(), changed_capabilities.fingerprint());

    let associated = associated_counter_item(item::Kind::AssociatedFunction);
    let associated_value = associated_counter_item(item::Kind::AssociatedValue);
    let method = associated_counter_item(item::Kind::Method);
    assert_ne!(associated.fingerprint(), associated_value.fingerprint());
    assert_ne!(associated.fingerprint(), method.fingerprint());

    let signature = signature::Builder::new().finish();
    let function = item::Definition::function("run", signature.clone()).unwrap();
    let intrinsic = item::Definition::intrinsic_function("run", signature).unwrap();
    assert_ne!(function.fingerprint(), intrinsic.fingerprint());

    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::unit());
    let operation = item::Definition::operation("run", signature.finish()).unwrap();
    assert_ne!(function.fingerprint(), operation.fingerprint());

    let signature = signature::Builder::new().finish();
    let method = item::Definition::method("Application::run", receiver::Mode::Owned, signature.clone()).unwrap();
    let intrinsic = item::Definition::intrinsic_method("Application::run", receiver::Mode::Owned, signature).unwrap();
    assert_ne!(method.fingerprint(), intrinsic.fingerprint());
}

#[test]
fn facade_operations_retain_exact_zero_to_many_parameter_proofs() {
    for actual in [0, 1, 2] {
        let mut signature = signature::Builder::new();
        for _ in 0..actual {
            signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        }
        signature.output(type_ref::Type::builtin(type_ref::Builtin::String));
        let operation = item::Definition::operation("load", signature.finish()).unwrap();
        assert_eq!(operation.signature().parameters().len(), actual);
    }

    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::unit()).output(type_ref::Type::builtin(type_ref::Builtin::String));
    let operation = item::Definition::operation("load", signature.finish()).unwrap();
    let mut builder = facade::Builder::new("effects", facade::Abi::new(1).unwrap()).unwrap();
    builder.item(operation).unwrap();
    let facade = builder.finish().unwrap();

    assert_eq!(facade.items().next().unwrap().kind(), item::Kind::Operation);
    assert_eq!(facade.items().next().unwrap().path(), "effects::load");

    let mut changed = signature::Builder::new();
    changed.parameter(type_ref::Type::unit()).output(type_ref::Type::builtin(type_ref::Builtin::I64));
    let mut builder = facade::Builder::new("effects", facade::Abi::new(1).unwrap()).unwrap();
    builder.item(item::Definition::operation("load", changed.finish()).unwrap()).unwrap();
    assert_ne!(facade.fingerprint(), builder.finish().unwrap().fingerprint());
}

#[test]
fn invocation_storage_is_exported_and_fingerprinted() {
    let mut owned = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    owned.type_::<Column>("Column", &[], &[]).unwrap();
    let owned = owned.finish().unwrap();
    let mut scoped = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    scoped.invocation_type::<Column>("Column", &[]).unwrap();
    let scoped = scoped.finish().unwrap();

    assert_ne!(owned.fingerprint(), scoped.fingerprint());
    assert_eq!(scoped.export_type("Column").unwrap().storage(), host::type_def::Storage::Invocation);
}

#[test]
fn invocation_storage_rejects_structural_type_parameters() {
    let mut facade = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();

    assert!(matches!(
        facade.invocation_type::<Column>("Renderer", &["Message"]),
        Err(host::Error::InvocationTypeParameters { ty }) if ty == "iced::Renderer"
    ));
}

#[test]
fn facade_finish_rejects_missing_modules_types_parameters_and_owners() {
    let mut invalid_capability = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    assert!(matches!(invalid_capability.type_::<Column>("Column", &[], &[capability::Kind::Copy]), Err(host::Error::InvalidCapabilitySet { capability: "Copy", required: "Clone", .. })));

    let mut duplicate = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    duplicate.module("widget").unwrap();
    assert!(matches!(
        duplicate.type_::<Column>("widget", &[], &[]),
        Err(host::Error::DuplicateFacadeEntry {
            path,
            existing: "module",
            incoming: "type",
        }) if path == "iced::widget"
    ));
    assert!(matches!(
        duplicate.module("widget"),
        Err(host::Error::DuplicateFacadeEntry {
            path,
            existing: "module",
            incoming: "module",
        }) if path == "iced::widget"
    ));

    let mut missing_module = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    missing_module.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    assert!(matches!(
        missing_module.finish(),
        Err(host::Error::MissingFacadeModule { path, module })
            if path == "iced::widget::Column" && module == "iced::widget"
    ));

    let mut unknown_type = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::registered("Missing").unwrap());
    unknown_type.item(item::Definition::function("run", signature.finish()).unwrap()).unwrap();
    assert!(matches!(
        unknown_type.finish(),
        Err(host::Error::UnknownFacadeType { item, ty })
            if item == "iced::run" && ty == "iced::Missing"
    ));

    let mut unknown_parameter = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::parameter("Message").unwrap());
    unknown_parameter.item(item::Definition::function("run", signature.finish()).unwrap()).unwrap();
    assert!(matches!(
        unknown_parameter.finish(),
        Err(host::Error::UnknownTypeParameter { item, parameter })
            if item == "iced::run" && parameter == "Message"
    ));

    let mut wrong_arity = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    wrong_arity.module("widget").unwrap();
    wrong_arity.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::registered("widget::Column").unwrap());
    wrong_arity.item(item::Definition::function("column", signature.finish()).unwrap()).unwrap();
    assert!(matches!(
        wrong_arity.finish(),
        Err(host::Error::InvalidFacadeTypeArguments {
            item,
            ty,
            expected: 1,
            actual: 0,
        }) if item == "iced::column" && ty == "iced::widget::Column"
    ));

    let mut invalid_owner = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    invalid_owner.module("widget").unwrap();
    invalid_owner
        .item(item::Definition::method("widget::Missing::push", receiver::Mode::Mutable, signature::Builder::new().finish()).unwrap())
        .unwrap();
    assert!(matches!(
        invalid_owner.finish(),
        Err(host::Error::InvalidAssociatedOwner { item, owner })
            if item == "iced::widget::Missing::push" && owner == "iced::widget::Missing"
    ));

    let mut invalid_macro = facade::Builder::new("ui", facade::Abi::new(1).unwrap()).unwrap();
    invalid_macro.item(item::Definition::intrinsic_macro("list", signature::Builder::new().finish()).unwrap()).unwrap();
    assert!(matches!(
        invalid_macro.finish(),
        Err(host::Error::InvalidIntrinsicMacroSignature { item, actual: 0 })
            if item == "ui::list"
    ));
}

#[test]
fn failed_signature_registration_does_not_replace_the_prior_parameter_contract() {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[capability::Kind::Clone]).unwrap();
    assert!(matches!(
        signature.type_parameter("Message", &[capability::Kind::Send]),
        Err(host::Error::DuplicateTypeParameter { parameter, .. })
            if parameter == "Message"
    ));

    let signature = signature.finish();
    let requirements = signature.requirements("Message").unwrap();
    assert!(requirements.contains(capability::Kind::Clone));
    assert!(!requirements.contains(capability::Kind::Send));
}

#[test]
fn facade_install_is_atomic_across_path_and_identity_conflicts() {
    let original = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Shared);
    let original_fingerprint = original.fingerprint().clone();
    let mut registry = host::Registry::new();
    registry.install(original).unwrap();

    let conflict = counter_facade(false, facade::Abi::new(2).unwrap(), receiver::Mode::Shared);
    assert!(matches!(
        registry.install(conflict),
        Err(host::Error::RegistryConflict { path, .. }) if path == "iced"
    ));
    assert_eq!(registry.facade_fingerprint("iced"), Some(&original_fingerprint));
    assert!(registry.type_("iced::widget::Column").is_some());
    assert!(registry.item("iced::run").is_some());

    let mut legacy = host::Registry::new();
    legacy.register_type::<ResultMarker>("other::Result").unwrap();
    let mut facade = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    facade.type_::<ResultMarker>("Result", &[], &[capability::Kind::Debug]).unwrap();
    let error = legacy.install(facade.finish().unwrap()).unwrap_err();
    assert!(matches!(error, host::Error::ConflictingHostIdentity { .. }));
    assert!(legacy.facade_fingerprint("iced").is_none());
    assert!(legacy.type_("iced::Result").is_none());
}

#[test]
fn registry_records_finished_facade_owners_without_inventing_legacy_owners() {
    let facade = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Shared);
    let fingerprint = facade.fingerprint().clone();
    let mut registry = host::Registry::new();
    registry.install(facade).unwrap();

    for owner in [registry.type_owner("iced::widget::Column").unwrap(), registry.item_owner("iced::run").unwrap()] {
        assert_eq!(owner.root(), "iced");
        assert_eq!(owner.fingerprint(), &fingerprint);
    }

    let mut legacy = host::Registry::new();
    legacy.register_type::<Value>("iced::Element").unwrap();
    assert!(legacy.type_owner("iced::Element").is_none());
}

#[test]
fn host_capabilities_are_explicit_and_signature_requirements_are_queryable() {
    let facade = counter_facade(false, facade::Abi::new(1).unwrap(), receiver::Mode::Shared);
    let result = facade.types().find(|definition| definition.path() == "iced::Result").unwrap();
    let column = facade.types().find(|definition| definition.path() == "iced::widget::Column").unwrap();
    let run = facade.items().find(|definition| definition.path() == "iced::run").unwrap();

    assert!(result.capabilities().contains(capability::Kind::Debug));
    assert!(!column.capabilities().contains(capability::Kind::Clone));
    let requirements = run.signature().requirements("Message").unwrap();
    assert!(requirements.contains(capability::Kind::Clone));
    assert!(requirements.contains(capability::Kind::Send));
    assert!(requirements.contains(capability::Kind::Static));
}

#[test]
fn direct_type_projections_are_validated_and_fingerprinted() {
    let projected = widget_facade(true, facade::Abi::new(1).unwrap());
    let plain = widget_facade(false, facade::Abi::new(1).unwrap());
    assert_ne!(projected.fingerprint(), plain.fingerprint());

    let column = projected.types().find(|definition| definition.path() == "iced::widget::Column").unwrap();
    assert_eq!(column.projections().len(), 1);
    assert_eq!(column.projections()[0].target(), "iced::Element");
    assert_eq!(column.projections()[0].arguments(), &[host::type_def::Argument::source("Message").unwrap()]);

    let reordered = {
        let mut builder = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
        builder.project("widget::Column", "Element", &["Message"]).unwrap();
        builder.type_::<Value>("Element", &["Message"], &[]).unwrap();
        builder.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
        builder.module("widget").unwrap();
        builder.finish().unwrap()
    };
    assert_eq!(projected.fingerprint(), reordered.fingerprint());

    let fresh = {
        let mut builder = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
        builder.module("widget").unwrap();
        builder.type_::<Value>("Element", &["Message"], &[]).unwrap();
        builder.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
        builder.project_with("widget::Column", "Element", &[host::type_def::Argument::fresh()]).unwrap();
        builder.finish().unwrap()
    };
    assert_ne!(projected.fingerprint(), fresh.fingerprint());
}

#[test]
fn projection_finish_rejects_unknown_mappings_duplicates_and_cycles() {
    let mut unknown = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    unknown.module("widget").unwrap();
    unknown.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    unknown.type_::<Value>("Element", &["Message"], &[]).unwrap();
    unknown.project("widget::Column", "Element", &["Missing"]).unwrap();
    assert!(matches!(
        unknown.finish(),
        Err(host::Error::InvalidProjectionParameter { parameter, .. })
            if parameter == "Missing"
    ));

    let mut duplicate = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    duplicate.module("widget").unwrap();
    duplicate.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    duplicate.type_::<Value>("Element", &["Message"], &[]).unwrap();
    duplicate.project("widget::Column", "Element", &["Message"]).unwrap();
    duplicate.project("widget::Column", "Element", &["Message"]).unwrap();
    assert!(matches!(duplicate.finish(), Err(host::Error::DuplicateProjection { .. })));

    let mut cycle = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    cycle.module("widget").unwrap();
    cycle.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    cycle.type_::<Value>("Element", &["Message"], &[]).unwrap();
    cycle.project("widget::Column", "Element", &["Message"]).unwrap();
    cycle.project("Element", "widget::Column", &["Message"]).unwrap();
    assert!(matches!(cycle.finish(), Err(host::Error::ProjectionCycle { .. })));
}

#[test]
fn cross_facade_projection_requires_the_exact_exported_target() {
    let iced_v1 = widget_facade(false, facade::Abi::new(1).unwrap());
    let value = iced_v1.export_type("Element").unwrap();
    let charts = charts_facade(value.clone());

    let mut missing = host::Registry::new();
    assert!(matches!(
        missing.install(charts_facade(value.clone())),
        Err(host::Error::MissingFacadeDependency { facade, .. }) if facade == "iced"
    ));

    let mut mismatched = host::Registry::new();
    mismatched.install(widget_facade(false, facade::Abi::new(2).unwrap())).unwrap();
    assert!(matches!(
        mismatched.install(charts_facade(value)),
        Err(host::Error::FacadeDependencyMismatch { target, .. })
            if target == "iced::Element"
    ));
    assert!(mismatched.type_("charts::Plot").is_none());

    let mut installed = host::Registry::new();
    installed.install(iced_v1).unwrap();
    installed.install(charts).unwrap();
    let plot = installed.type_("charts::Plot").unwrap();
    assert_eq!(plot.projections()[0].target(), "iced::Element");
}

#[test]
fn external_signature_types_require_and_retain_the_exact_export() {
    let iced_v1 = widget_facade(false, facade::Abi::new(1).unwrap());
    let exported = iced_v1.export_type("Element").unwrap();
    let controls = external_signature_facade(exported.clone());
    let changed = external_signature_facade(widget_facade(false, facade::Abi::new(2).unwrap()).export_type("Element").unwrap());
    assert_ne!(controls.fingerprint(), changed.fingerprint());

    let mut missing = host::Registry::new();
    assert!(matches!(
        missing.install(external_signature_facade(exported.clone())),
        Err(host::Error::MissingFacadeDependency { source, facade })
            if source == "controls::view" && facade == "iced"
    ));

    let mut mismatched = host::Registry::new();
    mismatched.install(widget_facade(false, facade::Abi::new(2).unwrap())).unwrap();
    assert!(matches!(
        mismatched.install(external_signature_facade(exported)),
        Err(host::Error::FacadeDependencyMismatch { source, target })
            if source == "controls::view" && target == "iced::Element"
    ));
    assert!(mismatched.item("controls::view").is_none());

    let mut engine = Engine::new();
    engine.register_host_facade(iced_v1).unwrap();
    engine.register_host_facade(controls).unwrap();
    engine
        .compile_executable(
            r#"
use iced::Element;
use controls::view;
enum Message { Tick }
pub fn screen() -> Element<Message> { view() }
"#,
        )
        .unwrap();
}

#[test]
fn private_facade_types_are_inferred_but_cannot_be_named_or_imported() {
    let public = carrier_facade(false);
    let private = carrier_facade(true);
    assert_ne!(public.fingerprint(), private.fingerprint());

    let mut engine = Engine::new();
    engine.register_host_facade(private).unwrap();
    engine
        .compile_executable(
            r#"
use hidden::{Value, builder};
pub fn finish() -> Value { builder().finish() }
"#,
        )
        .unwrap();

    let named = "pub fn expose(value: hidden::Builder) {}";
    let error = engine.compile_executable(named).unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownType { name, .. })
            if name == "hidden::Builder"
    ));
    assert_eq!(error.primary_span().start(), position_of(named, "hidden::Builder"));

    let imported = "use hidden::Builder;";
    let error = engine.compile_executable(imported).unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownImport { path, .. })
            if path == "hidden::Builder"
    ));
    assert_eq!(error.primary_span().start(), position_of(imported, "Builder"));
}

fn counter_facade(reverse: bool, abi: facade::Abi, receiver: receiver::Mode) -> facade::Definition {
    counter_facade_options(reverse, abi, receiver, false, false)
}

fn counter_facade_with(reverse: bool, changed_signature: bool, changed_capabilities: bool) -> facade::Definition {
    counter_facade_options(reverse, facade::Abi::new(1).unwrap(), receiver::Mode::Shared, changed_signature, changed_capabilities)
}

fn overload_facade(reverse: bool) -> facade::Definition {
    let mut builder = overload_builder_base();
    let mut integer = signature::Builder::new();
    integer.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    integer.output(type_ref::Type::registered("Value").unwrap());
    let integer = item::Definition::method("Value::key", receiver::Mode::Owned, integer.finish()).unwrap();
    let mut string = signature::Builder::new();
    string.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    string.output(type_ref::Type::registered("Value").unwrap());
    let string = item::Definition::method("Value::key", receiver::Mode::Owned, string.finish()).unwrap();
    for item in if reverse { [string, integer] } else { [integer, string] } {
        builder.item(item).unwrap();
    }
    builder.finish().unwrap()
}

fn overload_builder(return_only: bool) -> facade::Builder {
    let mut builder = overload_builder_base();
    for index in 0..2 {
        let mut signature = signature::Builder::new();
        signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        if return_only && index == 1 {
            signature.output(type_ref::Type::builtin(type_ref::Builtin::Bool));
        } else {
            signature.output(type_ref::Type::registered("Value").unwrap());
        }
        builder.item(item::Definition::method("Value::key", receiver::Mode::Owned, signature.finish()).unwrap()).unwrap();
    }
    builder
}

fn overload_builder_base() -> facade::Builder {
    let mut builder = facade::Builder::new("iced", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Value>("Value", &[], &[]).unwrap();
    builder
}

fn counter_facade_options(reverse: bool, abi: facade::Abi, receiver: receiver::Mode, changed_signature: bool, changed_capabilities: bool) -> facade::Definition {
    let mut builder = facade::Builder::new("iced", abi).unwrap();
    let mut run = signature::Builder::new();
    run.type_parameter("State", &[]).unwrap();
    run.type_parameter("Message", &[capability::Kind::Clone, capability::Kind::Send, capability::Kind::Static]).unwrap();
    run.parameter(type_ref::Type::parameter("State").unwrap());
    if changed_signature {
        run.parameter(type_ref::Type::parameter("Message").unwrap());
    }
    run.output(type_ref::Type::registered("Result").unwrap());

    let mut push = signature::Builder::new();
    push.type_parameter("Message", &[]).unwrap();
    push.parameter(type_ref::Type::parameter("Message").unwrap());
    push.output(type_ref::Type::applied("widget::Column", [type_ref::Type::parameter("Message").unwrap()]).unwrap());

    let result_capabilities = if changed_capabilities { &[capability::Kind::Debug, capability::Kind::Clone][..] } else { &[capability::Kind::Debug][..] };

    if reverse {
        builder.item(item::Definition::method("widget::Column::push", receiver, push.finish()).unwrap()).unwrap();
        builder.item(item::Definition::function("run", run.finish()).unwrap()).unwrap();
        builder.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
        builder.type_::<ResultMarker>("Result", &[], result_capabilities).unwrap();
        builder.module("widget").unwrap();
    } else {
        builder.module("widget").unwrap();
        builder.type_::<ResultMarker>("Result", &[], result_capabilities).unwrap();
        builder.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
        builder.item(item::Definition::function("run", run.finish()).unwrap()).unwrap();
        builder.item(item::Definition::method("widget::Column::push", receiver, push.finish()).unwrap()).unwrap();
    }

    builder.finish().unwrap()
}

fn associated_counter_item(kind: item::Kind) -> item::Definition {
    let signature = signature::Builder::new().finish();
    match kind {
        item::Kind::AssociatedValue => item::Definition::associated_value("Counter::new", type_ref::Type::builtin(type_ref::Builtin::I64)).unwrap(),
        item::Kind::AssociatedFunction => item::Definition::associated_function("Counter::new", signature).unwrap(),
        item::Kind::Method => item::Definition::method("Counter::new", receiver::Mode::Shared, signature).unwrap(),
        item::Kind::Value | item::Kind::Function | item::Kind::Operation | item::Kind::StreamOperation | item::Kind::IntrinsicFunction | item::Kind::IntrinsicMacro | item::Kind::IntrinsicMethod => panic!("expected associated item kind"),
    }
}

struct ResultMarker;
struct Column;
struct Value;
struct Plot;
struct Carrier;

fn function_module_facade(item_first: bool) -> facade::Definition {
    let mut builder = facade::Builder::new("api", facade::Abi::new(1).unwrap()).unwrap();
    builder.module("widget").unwrap();

    let mut button = signature::Builder::new();
    button
        .parameter(type_ref::Type::builtin(type_ref::Builtin::I64))
        .output(type_ref::Type::builtin(type_ref::Builtin::I64));
    let button = item::Definition::function("widget::button", button.finish()).unwrap();
    if item_first {
        builder.item(button).unwrap();
        builder.module("widget::button").unwrap();
    } else {
        builder.module("widget::button").unwrap();
        builder.item(button).unwrap();
    }
    builder
        .item(item::Definition::value("widget::button::primary", type_ref::Type::builtin(type_ref::Builtin::I64)).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn widget_facade(projected: bool, abi: facade::Abi) -> facade::Definition {
    let mut builder = facade::Builder::new("iced", abi).unwrap();
    builder.module("widget").unwrap();
    builder.type_::<Value>("Element", &["Message"], &[]).unwrap();
    builder.type_::<Column>("widget::Column", &["Message"], &[]).unwrap();
    if projected {
        builder.project("widget::Column", "Element", &["Message"]).unwrap();
    }
    builder.finish().unwrap()
}

fn charts_facade(value: host::type_def::Export) -> facade::Definition {
    let mut builder = facade::Builder::new("charts", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Plot>("Plot", &["Message"], &[]).unwrap();
    builder.project_to("Plot", value, &["Message"]).unwrap();
    builder.finish().unwrap()
}

fn external_signature_facade(value: host::type_def::Export) -> facade::Definition {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[]).unwrap();
    signature.output(type_ref::Type::external(value, [type_ref::Type::parameter("Message").unwrap()]).unwrap());
    let mut builder = facade::Builder::new("controls", facade::Abi::new(1).unwrap()).unwrap();
    builder.item(item::Definition::function("view", signature.finish()).unwrap()).unwrap();
    builder.finish().unwrap()
}

fn carrier_facade(private: bool) -> facade::Definition {
    let mut builder = facade::Builder::new("hidden", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Value>("Value", &[], &[]).unwrap();
    if private {
        builder.private_type::<Carrier>("Builder", &[], &[]).unwrap();
    } else {
        builder.type_::<Carrier>("Builder", &[], &[]).unwrap();
    }

    let mut create = signature::Builder::new();
    create.output(type_ref::Type::registered("Builder").unwrap());
    builder.item(item::Definition::intrinsic_function("builder", create.finish()).unwrap()).unwrap();
    let mut finish = signature::Builder::new();
    finish.output(type_ref::Type::registered("Value").unwrap());
    builder
        .item(item::Definition::intrinsic_method("Builder::finish", receiver::Mode::Owned, finish.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn position_of(source: &str, marker: &str) -> Position {
    let offset = source.find(marker).expect("diagnostic marker must exist");
    let prefix = &source[..offset];
    let line = prefix.chars().filter(|character| *character == '\n').count() + 1;
    let column = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line).chars().count() + 1;
    Position::new(u16::try_from(line).expect("test line must fit"), u16::try_from(column).expect("test column must fit"))
}
