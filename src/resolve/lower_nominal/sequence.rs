//! Checked native sequence construction, preserving the concrete Rust shape
//! separately from the interpreter's normalized logical type witness.

use super::{invariant, origin, Error, FnCallArgs, NativeCallContext, Op, Parser, Ty};
use crate::host::runtime::type_ref::Type;
use crate::{Dynamic, RawrResult, Span};
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct Constructor {
    key: Option<Box<[u8]>>,
    target: Option<Type>,
}

impl Constructor {
    pub(super) fn checked(
        concrete: Option<&rawr_import::classify::representability::convert::Type>,
        target: Option<Type>,
    ) -> Result<Self, Error> {
        let key = concrete
            .filter(|_| target.is_some())
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| Error::InvalidHostOperation {
                item: "native sequence constructor".into(),
                reason: "the checked concrete sequence shape could not be encoded".into(),
                operation: Span::NONE,
            })?
            .map(Vec::into_boxed_slice);
        Ok(Self { key, target })
    }

    pub(super) fn dependencies(&self) -> BTreeSet<crate::host::facade::Dependency> {
        let mut dependencies = BTreeSet::new();
        if let Some(target) = &self.target {
            target.collect_dependencies(&mut dependencies);
        }
        dependencies
    }

    pub(super) fn name(&self) -> String {
        format!(
            "\0rawr.sequence.construct.{:016x}",
            crate::program::schema::symbol_id(&self.key)
        )
    }

    pub(super) fn invoke(
        &self,
        context: Option<&NativeCallContext<'_>>,
        arguments: &mut FnCallArgs,
    ) -> RawrResult {
        let capacity = arguments[0]
            .as_int()
            .ok()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| invariant("a sequence constructor capacity is not usize"))?;
        let value = arguments[1].take();
        let mode = arguments[2].as_int().unwrap_or(-1);
        if let (Some(key), Some(target), Some(runtime)) = (
            self.key.as_deref(),
            self.target.as_ref(),
            context.and_then(|context| context.host_runtime()),
        ) && let Some(native) =
            crate::place::sequence::construct(key, &runtime.witness_for(target), capacity)?
        {
            let place = crate::place::get(&native)
                .ok_or_else(|| invariant("a native sequence factory lost its place"))?;
            match mode {
                0 => {}
                1 => {
                    let items = value
                        .into_array()
                        .map_err(|_| invariant("a sequence literal lost its evaluated operands"))?;
                    for item in items {
                        let _ = place.invoke(crate::place::Operation::Push, &mut [item])?;
                    }
                }
                2 => {
                    let _ = place.invoke(
                        crate::place::Operation::Resize,
                        &mut [Dynamic::from_int(capacity as crate::INT), value],
                    )?;
                }
                _ => return Err(invariant("an unknown sequence constructor mode")),
            }
            return Ok(native);
        }
        let mut items = crate::Array::with_capacity(capacity);
        match mode {
            0 => {}
            1 => items.extend(
                value
                    .into_array()
                    .map_err(|_| invariant("a sequence literal lost its evaluated operands"))?,
            ),
            2 => items.resize(capacity, value),
            _ => return Err(invariant("an unknown sequence constructor mode")),
        }
        Ok(Dynamic::from_array(items))
    }
}

impl Parser<'_> {
    pub(super) fn sequence_source(
        &mut self,
        ty: &Ty,
        span: Span,
        capacity: origin::Fragment,
        items: origin::Fragment,
        mode: usize,
    ) -> origin::Fragment {
        let helper = self.push_op(Op::Sequence {
            target: ty.clone(),
            span,
        });
        origin::Fragment::call(&helper, [capacity, items, mode.to_string().into()])
    }
}

#[cfg(test)]
mod tests;
