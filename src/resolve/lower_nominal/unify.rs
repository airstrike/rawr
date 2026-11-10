//! Unification of the lowering's types, and the projections of associated
//! types it selects.

use super::*;

impl<'a> Parser<'a> {
    /// Whether `expected` and `actual` unify, without binding either.
    pub(super) fn unifies(&mut self, expected: &Ty, actual: &Ty) -> bool {
        let checkpoint = self.inference.checkpoint();
        let fits = self.unify_type(expected, actual);
        self.inference.rollback(checkpoint);
        fits
    }

    pub(super) fn unify_type(&mut self, expected: &Ty, actual: &Ty) -> bool {
        let expected = self.inference.resolve(expected);
        let actual = self.inference.resolve(actual);
        if actual == Ty::Never || expected == actual {
            return true;
        }
        match (&expected, &actual) {
            (Ty::Infer(id), _) => {
                self.inference.bind(*id, actual);
                true
            }
            (_, Ty::Infer(id)) => {
                self.inference.bind(*id, expected);
                true
            }
            // A literal's `{integer}` or `{float}` takes the other kind.
            (Ty::Int(Int::Var(id)), Ty::Int(_)) | (Ty::Float(Float::Var(id)), Ty::Float(_)) => {
                self.inference.bind(*id, actual);
                true
            }
            (Ty::Int(_), Ty::Int(Int::Var(id))) | (Ty::Float(_), Ty::Float(Float::Var(id))) => {
                self.inference.bind(*id, expected);
                true
            }
            (
                Ty::Reference {
                    mutable: expected_mutable,
                    target: expected_target,
                },
                Ty::Reference {
                    mutable: actual_mutable,
                    target: actual_target,
                },
            ) => {
                expected_mutable == actual_mutable
                    && self.unify_type(expected_target, actual_target)
            }
            // A `&str` is a `&T` whose `T` is `str`, as jiff's
            // `strftime<'f, F: ?Sized>(&self, format: &'f F)` takes a literal.
            (
                Ty::Reference {
                    mutable: false,
                    target,
                },
                Ty::Str,
            ) => self.unify_type(target, &Ty::UnsizedStr),
            (
                Ty::Array {
                    element: expected_element,
                    kind: expected_kind,
                },
                Ty::Array {
                    element: actual_element,
                    kind: actual_kind,
                },
            ) if expected_kind == actual_kind => self.unify_type(expected_element, actual_element),
            (
                Ty::Script(expected_id, expected_arguments),
                Ty::Script(actual_id, actual_arguments),
            ) if expected_id == actual_id && expected_arguments.len() == actual_arguments.len() => {
                expected_arguments
                    .iter()
                    .zip(actual_arguments.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
            }
            (Ty::Boxed(expected_element), Ty::Boxed(actual_element)) => {
                self.unify_type(expected_element, actual_element)
            }
            (
                Ty::Counted(expected_counter, expected_element),
                Ty::Counted(actual_counter, actual_element),
            )
            | (
                Ty::Weak(expected_counter, expected_element),
                Ty::Weak(actual_counter, actual_element),
            ) => {
                expected_counter == actual_counter
                    && self.unify_type(expected_element, actual_element)
            }
            (Ty::Option(expected_element), Ty::Option(actual_element))
            | (Ty::Iter(expected_element, _), Ty::Iter(actual_element, _))
            | (Ty::Reverse(expected_element), Ty::Reverse(actual_element))
            | (Ty::Cow(expected_element), Ty::Cow(actual_element))
            | (Ty::Range(expected_element), Ty::Range(actual_element))
            | (Ty::InclusiveOf(expected_element), Ty::InclusiveOf(actual_element)) => {
                self.unify_type(expected_element, actual_element)
            }
            (Ty::InclusiveRange(expected_kind), Ty::InclusiveRange(actual_kind)) => {
                self.unify_type(&Ty::Int(*expected_kind), &Ty::Int(*actual_kind))
            }
            (Ty::Cell(expected_kind, expected_element), Ty::Cell(actual_kind, actual_element)) => {
                expected_kind == actual_kind && self.unify_type(expected_element, actual_element)
            }
            (
                Ty::Thread(expected_kind, expected_element),
                Ty::Thread(actual_kind, actual_element),
            ) => expected_kind == actual_kind && self.unify_type(expected_element, actual_element),
            (Ty::Result(expected_ok, expected_err), Ty::Result(actual_ok, actual_err)) => {
                self.unify_type(expected_ok, actual_ok) && self.unify_type(expected_err, actual_err)
            }
            (
                Ty::Keyed(expected_kind, expected_members),
                Ty::Keyed(actual_kind, actual_members),
            ) if expected_kind == actual_kind && expected_members.len() == actual_members.len() => {
                expected_members
                    .iter()
                    .zip(actual_members.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
            }
            (Ty::Tuple(expected_members), Ty::Tuple(actual_members))
                if expected_members.len() == actual_members.len() =>
            {
                expected_members
                    .iter()
                    .zip(actual_members.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
            }
            // A named function is a host callable of its signature.
            (
                Ty::Callable {
                    inputs: expected_inputs,
                    output: expected_output,
                    ..
                },
                Ty::FnItem {
                    inputs: actual_inputs,
                    output: actual_output,
                    ..
                },
            ) if actual_inputs.len() == expected_inputs.len() => {
                expected_inputs
                    .iter()
                    .zip(actual_inputs.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
                    && self.unify_type(expected_output, actual_output)
            }
            // A closure a crate value holds, as `Map<W, F, A>` from iced's
            // `Widget::map` does, is named `fn(A) -> B` in a signature, and
            // the runner writes both as the bridge's boxed closure (T417).
            (
                Ty::Fn {
                    inputs: expected_inputs,
                    output: expected_output,
                },
                Ty::Callable {
                    inputs: actual_inputs,
                    output: actual_output,
                    ..
                },
            )
            | (
                Ty::Callable {
                    inputs: expected_inputs,
                    output: expected_output,
                    ..
                },
                Ty::Fn {
                    inputs: actual_inputs,
                    output: actual_output,
                },
            ) if expected_inputs.len() == actual_inputs.len() => {
                expected_inputs
                    .iter()
                    .zip(actual_inputs.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
                    && self.unify_type(expected_output, actual_output)
            }
            // A function item coerces to a `fn` pointer of its signature.
            (
                Ty::Fn {
                    inputs: expected_inputs,
                    output: expected_output,
                },
                Ty::Fn {
                    inputs: actual_inputs,
                    output: actual_output,
                }
                | Ty::FnItem {
                    inputs: actual_inputs,
                    output: actual_output,
                    ..
                },
            )
            | (
                Ty::FnItem {
                    inputs: expected_inputs,
                    output: expected_output,
                    ..
                },
                Ty::FnItem {
                    inputs: actual_inputs,
                    output: actual_output,
                    ..
                },
            ) if expected_inputs.len() == actual_inputs.len()
                && (matches!(expected, Ty::Fn { .. })
                    || expected == actual
                    || matches!((&expected, &actual), (Ty::FnItem { key: left, .. }, Ty::FnItem { key: right, .. }) if left == right)) =>
            {
                expected_inputs
                    .iter()
                    .zip(actual_inputs.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
                    && self.unify_type(expected_output, actual_output)
            }
            (
                Ty::Callable {
                    inputs: expected_inputs,
                    output: expected_output,
                    ..
                },
                Ty::Callable {
                    inputs: actual_inputs,
                    output: actual_output,
                    ..
                },
            ) if expected_inputs.len() == actual_inputs.len() => {
                expected_inputs
                    .iter()
                    .zip(actual_inputs.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
                    && self.unify_type(expected_output, actual_output)
            }
            // A stream's tuple spread over a closure of as many parameters,
            // as `event::listen_with(|event, status, window| ..)`.
            (
                Ty::Callable {
                    inputs: expected_inputs,
                    output: expected_output,
                    ..
                },
                Ty::Callable {
                    inputs: actual_inputs,
                    output: actual_output,
                    ..
                },
            ) if actual_inputs.len() > 1
                && matches!(
                    expected_inputs.as_ref(),
                    [input] if matches!(
                        self.inference.resolve(input),
                        Ty::Tuple(members) if members.len() == actual_inputs.len()
                    )
                ) =>
            {
                let Ty::Tuple(members) = self.inference.resolve(&expected_inputs[0]) else {
                    unreachable!("matched a tuple input");
                };
                members
                    .iter()
                    .zip(actual_inputs.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
                    && self.unify_type(expected_output, actual_output)
            }
            // A crate's function where a host callable is expected, as
            // `.style(container::rounded_box)`: the adapter passes the Rust
            // function itself.
            #[cfg(feature = "rust-import")]
            (
                Ty::Callable {
                    inputs: expected_inputs,
                    output: expected_output,
                    ..
                },
                Ty::ImportedFunctionItem {
                    input: actual_input,
                    output: actual_output,
                    item: Some(proof),
                },
            ) if proof.item.signature().parameters().len() == expected_inputs.len() => {
                let actual_inputs: Box<[Ty]> = match (expected_inputs.len(), actual_input.as_ref())
                {
                    (1, input) => Box::new([input.clone()]),
                    (_, Ty::Tuple(members)) => members.clone(),
                    (_, _) => Box::default(),
                };
                expected_inputs
                    .iter()
                    .zip(actual_inputs.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
                    && self.unify_type(expected_output, actual_output)
            }
            #[cfg(feature = "rust-import")]
            (
                Ty::ImportedFunctionItem {
                    input: expected_input,
                    output: expected_output,
                    ..
                },
                Ty::ImportedFunctionItem {
                    input: actual_input,
                    output: actual_output,
                    ..
                },
            ) => {
                self.unify_type(expected_input, actual_input)
                    && self.unify_type(expected_output, actual_output)
            }
            (
                Ty::Host {
                    proof: expected_proof,
                    arguments: expected_arguments,
                },
                Ty::Host {
                    proof: actual_proof,
                    arguments: actual_arguments,
                },
            ) if same_host_definition(
                &expected_proof.definition,
                &expected_proof.owner,
                &actual_proof.definition,
                &actual_proof.owner,
            ) && expected_arguments.len() == actual_arguments.len() =>
            {
                expected_arguments
                    .iter()
                    .zip(actual_arguments.iter())
                    .all(|(expected, actual)| self.unify_type(expected, actual))
            }
            #[cfg(feature = "rust-import")]
            (
                Ty::ProfileMessage {
                    payload: expected_payload,
                    script: expected_script,
                    seal: expected_seal,
                },
                Ty::ProfileMessage {
                    payload: actual_payload,
                    script: actual_script,
                    seal: actual_seal,
                },
            ) if expected_seal == actual_seal => {
                self.unify_type(expected_payload, actual_payload)
                    && self.unify_type(expected_script, actual_script)
            }
            (Ty::Host { .. }, Ty::Host { .. }) => {
                let checkpoint = self.inference.checkpoint();
                let accepted = self
                    .project_type(&actual, &expected)
                    .is_some_and(|projected| self.unify_type(&expected, &projected));
                if accepted {
                    self.inference.commit(checkpoint);
                } else {
                    self.inference.rollback(checkpoint);
                }
                accepted
            }
            _ => false,
        }
    }

    pub(super) fn project_type(&mut self, source: &Ty, target: &Ty) -> Option<Ty> {
        self.selected_projection(source, target)
            .map(|(_, projected)| projected)
    }

    pub(super) fn selected_projection(
        &mut self,
        source: &Ty,
        target: &Ty,
    ) -> Option<(crate::host::projection::Key, Ty)> {
        let Ty::Host {
            proof: source_proof,
            arguments: source_arguments,
        } = source
        else {
            return None;
        };
        let source_definition = &source_proof.definition;
        let source_owner = source_proof.owner.as_ref()?;
        let Ty::Host {
            proof: target_proof,
            ..
        } = target
        else {
            return None;
        };
        let target_definition = &target_proof.definition;
        let projection = source_definition
            .projections()
            .iter()
            .find(|projection| projection.target_contract() == &target_definition.contract())?;
        let projected_owner = projection
            .target_dependency()
            .unwrap_or_else(|| source_owner.clone());
        if target_proof.owner.as_ref() != Some(&projected_owner) {
            return None;
        }
        let substitutions: BTreeMap<_, _> = source_definition
            .parameters()
            .iter()
            .zip(source_arguments.iter())
            .map(|(name, argument)| (name.as_str(), argument.clone()))
            .collect();
        let arguments = projection
            .arguments()
            .iter()
            .map(|argument| match argument {
                crate::host::type_def::Argument::Source(name) => {
                    substitutions.get(name.as_str()).cloned()
                }
                crate::host::type_def::Argument::Fresh => Some(self.inference.fresh()),
            })
            .collect::<Option<Box<[_]>>>()?;
        Some(Ty::Host {
            proof: crate::Shared::clone(target_proof),
            arguments,
        })
        .map(|projected| {
            (
                crate::host::projection::Key::new(
                    source_owner.clone(),
                    source_definition.contract(),
                    projection.clone(),
                ),
                projected,
            )
        })
    }

    pub(super) fn require_capability(
        &self,
        model: &Nominal,
        capability: capability::Kind,
        span: Span,
    ) -> Result<(), Error> {
        if model.capabilities().contains(capability) {
            return Ok(());
        }
        Err(Error::MissingCapability {
            owner: model.name().into(),
            capability: capability.name().into(),
            operation: span,
            derive: true,
            required_by: None,
        })
    }
}
