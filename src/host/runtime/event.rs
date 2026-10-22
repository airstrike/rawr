//! Private contextual finalization of authenticated event-widget replay.

use super::{type_ref::Type, Requirement};
use crate::{host, Span};
use std::collections::BTreeSet;

/// The exact private conversion selected by a checked mapper-bearing value.
///
/// This key is not an ordinary facade projection and cannot register a callback.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Key {
    owner: host::facade::Dependency,
    source: host::type_def::Contract,
    target_owner: host::facade::Dependency,
    target: host::type_def::Contract,
    mapper: [u8; 16],
}

/// A contextual use with separate native-source and script-target types.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Site {
    key: Key,
    source: Type,
    target: Type,
    span: Span,
}

/// A generated private replay binding after manifest and profile authentication.
/// Raw projectability evidence cannot construct this registration capability.
#[doc(hidden)]
pub struct Adapter {
    key: Key,
}

impl Adapter {
    pub(crate) const fn authenticated(key: Key) -> Self {
        Self { key }
    }
    pub(crate) fn matches(&self, key: &Key) -> bool {
        &self.key == key
    }

    /// Return the source facade that owns this authenticated callback.
    #[doc(hidden)]
    pub fn root(&self) -> &str {
        self.key.owner.root()
    }
}

impl super::Builder {
    /// Bind an authenticated generated replay finalizer without a facade edge.
    #[doc(hidden)]
    pub fn bind_event_replay(
        &mut self,
        adapter: &Adapter,
        callback: impl Fn(
                &super::Call<'_>,
                &mut [&mut crate::Dynamic],
            ) -> Result<crate::Dynamic, Box<crate::EvalAltResult>>
            + crate::func::SendSync
            + 'static,
    ) -> Result<&mut Self, super::Error> {
        let owner = host::facade::Dependency::new(
            self.definition.root_path(),
            self.definition.fingerprint().clone(),
        );
        if adapter.key.owner != owner
            || !self
                .definition
                .types()
                .any(|ty| ty.contract() == adapter.key.source)
            || self.events.contains_key(&adapter.key)
        {
            return Err(super::Error::profile(
                "private event binding owner, source or duplication",
            ));
        }
        self.events.insert(
            adapter.key.clone(),
            crate::Shared::new(super::Contextual(callback)),
        );
        Ok(self)
    }
}

impl super::Registry {
    pub(crate) fn invoke_event(
        &self,
        site: &Site,
        requirement: &Requirement,
        args: &mut [&mut crate::Dynamic],
        model: &crate::program::model::Model,
    ) -> Result<crate::Dynamic, Box<crate::EvalAltResult>> {
        let error = |message: String| -> Box<crate::EvalAltResult> {
            crate::EvalAltResult::ErrorRuntime(message.into(), site.span.start()).into()
        };
        if args.len() != 1
            || !matches!(&requirement.binding, super::Binding::Event(key) if key == &site.key)
            || requirement.operands.as_ref() != std::slice::from_ref(&site.source)
            || requirement.output != site.target
        {
            return Err(error(
                "private event conversion operand/requirement mismatch".into(),
            ));
        }
        site.source
            .validate(args[0], model, self)
            .map_err(|cause| error(format!("private event source: {cause}")))?;
        let output = self.invoke(requirement, args, None)?;
        site.target
            .validate(&output, model, self)
            .map_err(|cause| error(format!("private event target: {cause}")))?;
        Ok(output)
    }
}

impl Key {
    pub(crate) const fn selected(
        owner: host::facade::Dependency,
        source: host::type_def::Contract,
        target_owner: host::facade::Dependency,
        target: host::type_def::Contract,
        mapper: [u8; 16],
    ) -> Self {
        Self {
            owner,
            source,
            target_owner,
            target,
            mapper,
        }
    }

    pub(crate) const fn mapper(&self) -> &[u8; 16] {
        &self.mapper
    }
}

impl Site {
    pub(crate) const fn selected(key: Key, source: Type, target: Type, span: Span) -> Self {
        Self {
            key,
            source,
            target,
            span,
        }
    }

    pub(crate) const fn source(&self) -> &Type {
        &self.source
    }

    pub(crate) fn requirement(&self) -> Requirement {
        Requirement {
            owner: self.key.owner.clone(),
            binding: super::Binding::Event(self.key.clone()),
            path: self.key.source.path().into(),
            operands: vec![self.source.clone()].into_boxed_slice(),
            output: self.target.clone(),
            receiver: None,
            instance: None,
            bound: super::Bound::default(),
        }
    }

    pub(crate) fn dependencies(&self) -> BTreeSet<host::facade::Dependency> {
        let mut dependencies =
            BTreeSet::from([self.key.owner.clone(), self.key.target_owner.clone()]);
        self.source.collect_dependencies(&mut dependencies);
        self.target.collect_dependencies(&mut dependencies);
        dependencies
    }

    pub(crate) fn symbol_id(&self) -> u64 {
        crate::program::schema::symbol_id(self)
    }
}

#[cfg(all(test, not(feature = "no_function")))]
mod tests {
    use super::*;
    use crate::host::runtime::{Builder, Registry};
    use crate::{Dynamic, Engine};
    use std::any::TypeId;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn private_conversion_requires_binding_and_checks_both_value_types() {
        let mut builder =
            host::facade::Builder::new("native", host::facade::Abi::new(1).unwrap()).unwrap();
        builder.type_::<i64>("Source", &[], &[]).unwrap();
        builder.type_::<bool>("Target", &[], &[]).unwrap();
        let facade = builder.finish().unwrap();
        assert!(facade.types().all(|ty| ty.projections().is_empty()));
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let source = facade
            .types()
            .find(|ty| ty.path() == "native::Source")
            .unwrap()
            .contract();
        let target = facade
            .types()
            .find(|ty| ty.path() == "native::Target")
            .unwrap()
            .contract();
        let key = Key::selected(owner.clone(), source, owner.clone(), target, [7; 16]);
        let site = Site::selected(
            key.clone(),
            Type::builtin("i64", TypeId::of::<i64>()),
            Type::builtin("bool", TypeId::of::<bool>()),
            Span::NONE,
        );
        let requirement = site.requirement();
        let mut missing = Registry::new();
        missing
            .install(Builder::new(facade.clone()).finish())
            .unwrap();
        assert!(missing
            .validate(
                std::slice::from_ref(&owner),
                std::slice::from_ref(&requirement),
                &[],
                &[],
                &[]
            )
            .is_err());

        let count = Arc::new(AtomicUsize::new(0));
        let called = count.clone();
        let mut builder = Builder::new(facade);
        builder
            .bind_event_replay(&Adapter::authenticated(key), move |call, values| {
                assert_eq!(call.event_mapper(), Some(&[7; 16]));
                called.fetch_add(1, Ordering::SeqCst);
                let value = values[0].take().cast::<i64>();
                Ok(if value == 9 {
                    Dynamic::from("wrong target")
                } else {
                    Dynamic::from(value > 0)
                })
            })
            .unwrap();
        let mut registry = Registry::new();
        registry.install(builder.finish()).unwrap();
        registry
            .validate(&[owner], std::slice::from_ref(&requirement), &[], &[], &[])
            .unwrap();
        let program = crate::resolve::resolve(
            Engine::new().parse_unit("()").unwrap(),
            &host::Registry::new(),
        )
        .unwrap();
        let model =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
        let mut value = Dynamic::from(1_i64);
        assert!(registry
            .invoke_event(&site, &requirement, &mut [&mut value], &model)
            .unwrap()
            .cast::<bool>());
        let mut wrong_source = Dynamic::from("wrong source");
        assert!(registry
            .invoke_event(&site, &requirement, &mut [&mut wrong_source], &model)
            .is_err());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut wrong_target = Dynamic::from(9_i64);
        assert!(registry
            .invoke_event(&site, &requirement, &mut [&mut wrong_target], &model)
            .is_err());
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let mut foreign = site.clone();
        foreign.key.mapper = [8; 16];
        assert!(registry
            .validate(&[], &[foreign.requirement()], &[], &[], &[])
            .is_err());
        let mut value = Dynamic::from(1_i64);
        assert!(registry
            .invoke_event(&foreign, &requirement, &mut [&mut value], &model)
            .is_err());
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
