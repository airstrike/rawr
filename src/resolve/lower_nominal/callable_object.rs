//! A program's function or closure coerced to a crate trait's object
//! through the crate's impl of the trait for every callable, as rustc
//! coerces `&Code::highlight` to `&dyn Highlighter<Code>` (#1981).
//!
//! The value stays the program's callable, tagged as one (`object::CALLABLE`),
//! with the witness of the callable the impl's `Fn` bound takes: the
//! runner's adapter makes the Rust closure that calls it, whose inputs from
//! native code are attested by that witness, so script code reads each as
//! its type.

use super::{invariant, FnCallArgs, NativeCallContext};
use crate::host::runtime::type_ref::Type;
use crate::{Dynamic, ImmutableString, RawrResult};
use std::collections::BTreeSet;

/// The coercion of a callable to an object, by the callable's type.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(super) struct Coercion {
    call: Option<Type>,
}

impl Coercion {
    /// The coercion of a callable of type `call`, when it has a runtime
    /// type.
    pub(super) const fn new(call: Option<Type>) -> Self {
        Self { call }
    }

    pub(super) fn dependencies(&self) -> BTreeSet<crate::host::facade::Dependency> {
        let mut dependencies = BTreeSet::new();
        if let Some(call) = &self.call {
            call.collect_dependencies(&mut dependencies);
        }
        dependencies
    }

    /// This coercion with each program type's schema as `schema` gives it
    /// (`Type::renewed`).
    pub(super) fn renewed(
        &self,
        schema: &dyn Fn(&crate::type_def::Identity) -> Option<crate::program::schema::Fingerprint>,
    ) -> Option<Self> {
        Some(Self {
            call: match &self.call {
                Some(call) => Some(call.renewed(schema)?),
                None => None,
            },
        })
    }

    /// The private function's name, one per callable type.
    pub(super) fn name(&self) -> String {
        format!(
            "\0rawr.object.callable.{:016x}",
            crate::program::schema::symbol_id(&self.call)
        )
    }

    /// The program's callable `arguments[0]` as the object, tagged, with
    /// the witness of its type.
    pub(super) fn invoke(
        &self,
        context: Option<&NativeCallContext<'_>>,
        arguments: &mut FnCallArgs,
    ) -> RawrResult {
        let value = arguments
            .first_mut()
            .ok_or_else(|| invariant("a callable's coercion lost its operand"))?
            .take();
        let witness = match (
            &self.call,
            context.and_then(|context| context.host_runtime()),
        ) {
            (Some(call), Some(runtime)) => Some(runtime.witness_for(call)),
            _ => None,
        };
        let tag = ImmutableString::from(rawr_import::classify::representability::object::CALLABLE);
        Ok(Dynamic::from(crate::object::Coerced::called(
            value, tag, witness,
        )))
    }
}
