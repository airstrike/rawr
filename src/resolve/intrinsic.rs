//! Proven compiler-private operations over a static host facade.

pub(super) use crate::host::runtime::type_ref::Type;
use crate::{host, Dynamic, EvalAltResult, ImmutableString, RawrResult, Span};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Operand {
    ty: Type,
    span: Span,
    receiver: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Proof {
    facade: host::facade::Fingerprint,
    owner: host::facade::Dependency,
    item: host::item::Fingerprint,
    path: ImmutableString,
    kind: host::item::Kind,
    receiver: Option<crate::receiver::Mode>,
    operands: Box<[Operand]>,
    output: Type,
    span: Span,
    /// The instance of a generic imported item this call uses.
    instance: Option<[u8; 32]>,
    /// Whether the call gives a `DerefMut` borrow guard's place, which Rawr
    /// reads and stores as a `RefCell` borrow, in place of its `output`.
    place: bool,
}

impl Operand {
    pub(super) const fn new(ty: Type, span: Span, receiver: bool) -> Self {
        Self { ty, span, receiver }
    }

    pub(super) const fn ty(&self) -> &Type {
        &self.ty
    }
}

impl Proof {
    pub(super) fn new(
        facade: host::facade::Fingerprint,
        owner: host::facade::Dependency,
        item: &host::item::Definition,
        operands: Box<[Operand]>,
        output: Type,
        span: Span,
    ) -> Self {
        Self {
            facade,
            owner,
            item: item.fingerprint(),
            path: item.path().into(),
            kind: item.kind(),
            receiver: item.receiver(),
            operands,
            output,
            span,
            instance: None,
            place: item.guard() == Some(host::item::Guard::Mutable),
        }
    }

    /// This call of one instance of a generic imported item.
    pub(super) const fn with_instance(mut self, instance: Option<[u8; 32]>) -> Self {
        self.instance = instance;
        self
    }

    pub(super) const fn operands(&self) -> &[Operand] {
        &self.operands
    }

    pub(super) const fn span(&self) -> Span {
        self.span
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }

    #[allow(dead_code)]
    pub(super) const fn output(&self) -> &Type {
        &self.output
    }

    /// This proof with each generic parameter replaced by its argument.
    pub(super) fn instantiate(&self, arguments: &[Type]) -> Self {
        Self {
            operands: self
                .operands
                .iter()
                .map(|operand| Operand {
                    ty: operand.ty.instantiate(arguments),
                    ..operand.clone()
                })
                .collect(),
            output: self.output.instantiate(arguments),
            ..self.clone()
        }
    }

    pub(super) fn runtime_requirement(&self) -> host::runtime::Requirement {
        let requirement = host::runtime::Requirement::new(
            self.owner.clone(),
            self.item.clone(),
            self.path.clone(),
            self.operands
                .iter()
                .map(|operand| operand.ty.clone())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            self.output.clone(),
            self.receiver,
        );
        #[cfg(feature = "rust-import")]
        let requirement = requirement.with_instance(self.instance);
        requirement
    }

    /// Whether this is a method with a `&mut self` receiver.
    pub(super) fn mutates_receiver(&self) -> bool {
        self.receiver == Some(crate::receiver::Mode::Mutable)
    }

    pub(super) fn symbol_id(&self) -> u64 {
        crate::program::schema::symbol_id(self)
    }

    pub(super) fn dependencies(&self) -> BTreeSet<host::facade::Dependency> {
        let mut dependencies = BTreeSet::from([self.owner.clone()]);
        for operand in &self.operands {
            operand.ty.collect_dependencies(&mut dependencies);
        }
        self.output.collect_dependencies(&mut dependencies);
        dependencies
    }

    #[cfg(test)]
    pub(super) fn invoke(
        &self,
        runtime: &host::runtime::Registry,
        invocation: Option<&host::runtime::invocation::Proof>,
        args: &mut [&mut Dynamic],
        model: &crate::program::model::Model,
    ) -> RawrResult {
        self.invoke_prepared(
            runtime,
            &self.runtime_requirement(),
            invocation,
            args,
            model,
            None,
        )
    }

    pub(super) fn invoke_prepared(
        &self,
        runtime: &host::runtime::Registry,
        requirement: &host::runtime::Requirement,
        invocation: Option<&host::runtime::invocation::Proof>,
        args: &mut [&mut Dynamic],
        model: &crate::program::model::Model,
        equal: Option<&host::runtime::Equal<'_>>,
    ) -> RawrResult {
        if args.len() != self.operands.len() {
            return Err(EvalAltResult::ErrorRuntime(
                format!(
                    "runtime binding for `{}` received {} operand(s), expected {}",
                    self.path,
                    args.len(),
                    self.operands.len()
                )
                .into(),
                self.span.start(),
            )
            .into());
        }
        for (index, (operand, value)) in self.operands.iter().zip(args.iter()).enumerate() {
            if let Err(error) = operand.ty.validate(value, model, runtime) {
                return Err(EvalAltResult::ErrorRuntime(
                    format!(
                        "runtime binding for `{}` received invalid operand {index}, expected `{}`: {error}",
                        self.path, operand.ty
                    )
                    .into(),
                    operand.span.start(),
                )
                .into());
            }
        }

        let result = runtime.invoke(requirement, args, invocation, equal);

        for (index, (operand, value)) in self.operands.iter().zip(args.iter()).enumerate() {
            let observable =
                operand.receiver && !matches!(self.receiver, Some(crate::receiver::Mode::Owned));
            if observable && let Err(error) = operand.ty.validate(value, model, runtime) {
                return Err(EvalAltResult::ErrorRuntime(
                        format!(
                            "runtime binding for `{}` left mutated operand {index} outside exact `{}`: {error}",
                            self.path, operand.ty
                        )
                        .into(),
                        operand.span.start(),
                    )
                    .into());
            }
        }

        let result = result?;

        #[cfg(feature = "rust-import")]
        if self.place && result.is::<crate::island::place::Place>() {
            return Ok(result);
        }
        // A scope frame the call made (D558 tier 1): what it makes is
        // checked by the adapter that makes it, in the frame.
        if crate::scope::is_opening(&result) {
            return Ok(result);
        }
        if let Err(error) = self.output.validate(&result, model, runtime) {
            return Err(EvalAltResult::ErrorRuntime(
                format!(
                    "runtime binding for `{}` returned `{}`, expected `{}`: {error}",
                    self.path,
                    result.type_name(),
                    self.output
                )
                .into(),
                self.span.start(),
            )
            .into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::{Operand, Proof, Type};
    use crate::host::{facade, item, signature, type_ref};
    use crate::program::{model::Model, GenerationId};
    use crate::{host, resolve, Dynamic, Engine, Span};
    use std::any::TypeId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone)]
    struct MutableValue;

    #[test]
    fn private_operation_symbols_include_the_exact_facade_fingerprint() {
        let first = list_facade(1);
        let second = list_facade(2);
        let first_item = first.items().next().unwrap().clone();
        let second_item = second.items().next().unwrap().clone();
        let operand = Operand::new(
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
            false,
        );
        let first = Proof::new(
            first.fingerprint().clone(),
            crate::host::facade::Dependency::new(first.root_path(), first.fingerprint().clone()),
            &first_item,
            vec![operand.clone()].into_boxed_slice(),
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
        );
        let second = Proof::new(
            second.fingerprint().clone(),
            crate::host::facade::Dependency::new(second.root_path(), second.fingerprint().clone()),
            &second_item,
            vec![operand].into_boxed_slice(),
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
        );

        assert_ne!(first.symbol_id(), second.symbol_id());
    }

    #[test]
    fn operation_proofs_retain_item_and_structural_type_facade_owners() {
        let item_facade = list_facade(1);
        let type_facade = named_list_facade("theme", 1);
        let item_owner = crate::host::facade::Dependency::new(
            item_facade.root_path(),
            item_facade.fingerprint().clone(),
        );
        let type_owner = crate::host::facade::Dependency::new(
            type_facade.root_path(),
            type_facade.fingerprint().clone(),
        );
        let item = item_facade.items().next().unwrap();
        let proof = Proof::new(
            item_facade.fingerprint().clone(),
            item_owner.clone(),
            item,
            vec![Operand::new(
                Type::host(
                    "theme::Value".into(),
                    TypeId::of::<i64>(),
                    Box::default(),
                    Some(type_owner.clone()),
                ),
                Span::NONE,
                false,
            )]
            .into_boxed_slice(),
            Type::unit(),
            Span::NONE,
        );

        assert_eq!(
            proof.dependencies(),
            [item_owner, type_owner].into_iter().collect()
        );
    }

    #[test]
    fn invocation_validates_operands_before_callback_and_results_afterward() {
        let facade = list_facade(1);
        let item = facade.items().next().unwrap().clone();
        let owner =
            crate::host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let proof = Proof::new(
            facade.fingerprint().clone(),
            owner,
            &item,
            vec![Operand::new(
                Type::builtin("i64", TypeId::of::<crate::INT>()),
                Span::NONE,
                false,
            )]
            .into_boxed_slice(),
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let mut runtime = host::runtime::Builder::new(facade);
        let callback_calls = calls.clone();
        runtime
            .bind("ui::list", move |_| {
                callback_calls.fetch_add(1, Ordering::Relaxed);
                Ok(Dynamic::from(false))
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();
        let model = empty_model();

        let mut wrong_operand = Dynamic::from(false);
        let error = proof
            .invoke(&registry, None, &mut [&mut wrong_operand], &model)
            .unwrap_err();
        assert!(error.to_string().contains("ui::list"));
        assert!(error.to_string().contains("invalid operand 0"));
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        let mut operand = Dynamic::from(1_i64);
        let error = proof
            .invoke(&registry, None, &mut [&mut operand], &model)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("returned `bool`, expected `i64`"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn invocation_revalidates_an_observable_mutated_receiver() {
        let facade = mutable_facade();
        let item = facade.items().next().unwrap().clone();
        let owner =
            crate::host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let proof = Proof::new(
            facade.fingerprint().clone(),
            owner.clone(),
            &item,
            vec![Operand::new(
                Type::host(
                    "mutable::Value".into(),
                    TypeId::of::<MutableValue>(),
                    Box::default(),
                    Some(owner),
                ),
                Span::NONE,
                true,
            )]
            .into_boxed_slice(),
            Type::unit(),
            Span::NONE,
        );
        let mut runtime = host::runtime::Builder::new(facade);
        runtime
            .bind("mutable::Value::change", |args| {
                *args[0] = Dynamic::from(false);
                Ok(Dynamic::UNIT)
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();
        let mut value = Dynamic::from(MutableValue);
        let error = proof
            .invoke(&registry, None, &mut [&mut value], &empty_model())
            .unwrap_err();

        assert!(error.to_string().contains("mutable::Value::change"));
        assert!(error.to_string().contains("left mutated operand 0"));
    }

    #[test]
    fn selected_overload_fingerprints_dispatch_exact_runtime_contracts() {
        let facade = overload_facade(1);
        let owner =
            crate::host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let integer = facade
            .items()
            .find(|item| {
                item.signature().parameters() == [type_ref::Type::builtin(type_ref::Builtin::I64)]
            })
            .unwrap();
        let string = facade
            .items()
            .find(|item| {
                item.signature().parameters()
                    == [type_ref::Type::builtin(type_ref::Builtin::String)]
            })
            .unwrap();
        let integer = Proof::new(
            facade.fingerprint().clone(),
            owner.clone(),
            integer,
            vec![Operand::new(
                Type::builtin("i64", TypeId::of::<crate::INT>()),
                Span::NONE,
                false,
            )]
            .into_boxed_slice(),
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
        );
        let string = Proof::new(
            facade.fingerprint().clone(),
            owner,
            string,
            vec![Operand::new(
                Type::builtin("String", TypeId::of::<crate::ImmutableString>()),
                Span::NONE,
                false,
            )]
            .into_boxed_slice(),
            Type::builtin("String", TypeId::of::<crate::ImmutableString>()),
            Span::NONE,
        );
        assert_ne!(integer.runtime_requirement(), string.runtime_requirement());

        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = calls.clone();
        let mut runtime = host::runtime::Builder::new(facade);
        runtime
            .bind("overloaded::select", move |args| {
                callback_calls.fetch_add(1, Ordering::Relaxed);
                Ok(args[0].take())
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();

        let mut integer_value = Dynamic::from(42_i64);
        let result = integer
            .invoke(&registry, None, &mut [&mut integer_value], &empty_model())
            .unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
        let mut string_value = Dynamic::from(crate::ImmutableString::from("forty-two"));
        let result = string
            .invoke(&registry, None, &mut [&mut string_value], &empty_model())
            .unwrap();
        assert_eq!(result.cast::<crate::ImmutableString>(), "forty-two");

        let mut wrong = Dynamic::from(crate::ImmutableString::from("wrong"));
        let error = integer
            .invoke(&registry, None, &mut [&mut wrong], &empty_model())
            .unwrap_err();
        assert!(error.to_string().contains("invalid operand 0"));
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_requirement_remembers_its_binding_only_for_the_registry_that_served_it() {
        let facade = overload_facade(1);
        let owner =
            crate::host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let item = facade
            .items()
            .find(|item| {
                item.signature().parameters() == [type_ref::Type::builtin(type_ref::Builtin::I64)]
            })
            .unwrap();
        let proof = Proof::new(
            facade.fingerprint().clone(),
            owner,
            item,
            vec![Operand::new(
                Type::builtin("i64", TypeId::of::<crate::INT>()),
                Span::NONE,
                false,
            )]
            .into_boxed_slice(),
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
        );
        let registry = |offset: i64| {
            let mut runtime = host::runtime::Builder::new(overload_facade(1));
            runtime
                .bind("overloaded::select", move |args| {
                    Ok(Dynamic::from(args[0].as_int().unwrap() + offset))
                })
                .unwrap();
            let mut registry = host::runtime::Registry::new();
            registry.install(runtime.finish()).unwrap();
            registry
        };
        let (first, second) = (registry(0), registry(100));
        let requirement = proof.runtime_requirement();
        let call = |registry: &host::runtime::Registry| {
            let mut value = Dynamic::from(1_i64);
            proof
                .invoke_prepared(
                    registry,
                    &requirement,
                    None,
                    &mut [&mut value],
                    &empty_model(),
                    None,
                )
                .unwrap()
                .as_int()
                .unwrap()
        };

        assert_eq!(call(&first), 1);
        assert_eq!(call(&first), 1);
        assert_eq!(call(&second), 101);
        assert_eq!(call(&first), 1);
        assert_eq!(requirement.clone(), requirement);
    }

    #[test]
    fn overload_runtime_rejects_a_foreign_facade_fingerprint() {
        let compiled = overload_facade(1);
        let owner = crate::host::facade::Dependency::new(
            compiled.root_path(),
            compiled.fingerprint().clone(),
        );
        let item = compiled.items().next().unwrap();
        let proof = Proof::new(
            compiled.fingerprint().clone(),
            owner.clone(),
            item,
            vec![Operand::new(
                Type::builtin("i64", TypeId::of::<crate::INT>()),
                Span::NONE,
                false,
            )]
            .into_boxed_slice(),
            Type::builtin("i64", TypeId::of::<crate::INT>()),
            Span::NONE,
        );
        let foreign = overload_facade(2);
        let mut runtime = host::runtime::Builder::new(foreign);
        runtime
            .bind("overloaded::select", |args| Ok(args[0].take()))
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();

        assert!(matches!(
            registry.validate(
                &[owner],
                &[proof.runtime_requirement()],
                &[],
                &[],
                &[],
            ),
            Err(host::runtime::Error::FacadeMismatch { facade })
                if facade == "overloaded"
        ));
    }

    fn list_facade(abi: u32) -> facade::Definition {
        named_list_facade("ui", abi)
    }

    fn named_list_facade(root: &str, abi: u32) -> facade::Definition {
        let mut builder = facade::Builder::new(root, facade::Abi::new(abi).unwrap()).unwrap();
        let mut signature = signature::Builder::new();
        signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        signature.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(item::Definition::intrinsic_macro("list", signature.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    fn mutable_facade() -> facade::Definition {
        let mut builder = facade::Builder::new("mutable", facade::Abi::new(1).unwrap()).unwrap();
        builder.type_::<MutableValue>("Value", &[], &[]).unwrap();
        builder
            .item(
                item::Definition::intrinsic_method(
                    "Value::change",
                    crate::receiver::Mode::Mutable,
                    signature::Builder::new().finish(),
                )
                .unwrap(),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn overload_facade(abi: u32) -> facade::Definition {
        let mut builder =
            facade::Builder::new("overloaded", facade::Abi::new(abi).unwrap()).unwrap();
        for ty in [type_ref::Builtin::I64, type_ref::Builtin::String] {
            let mut signature = signature::Builder::new();
            let ty = type_ref::Type::builtin(ty);
            signature.parameter(ty.clone());
            signature.output(ty);
            builder
                .item(item::Definition::function("select", signature.finish()).unwrap())
                .unwrap();
        }
        builder.finish().unwrap()
    }

    fn empty_model() -> Model {
        let engine = Engine::new();
        let program =
            resolve::resolve(engine.parse_unit("").unwrap(), &host::Registry::new()).unwrap();
        Model::new(GenerationId::issue(), &program)
    }
}
