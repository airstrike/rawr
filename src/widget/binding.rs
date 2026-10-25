//! Authority-bound scripted-widget data for the iced lifecycle adapter.

use crate::{application, host, program, timing, widget, Dynamic, Shared};
use std::fmt;

mod stamp {
    use std::sync::Arc;

    #[derive(Clone)]
    pub(super) struct Stamp(Arc<()>);

    impl Stamp {
        pub(super) fn issue() -> Self {
            Self(Arc::new(()))
        }

        pub(super) fn matches(&self, other: &Self) -> bool {
            Arc::ptr_eq(&self.0, &other.0)
        }
    }
}

/// One scripted widget bound to the exact launch that may execute its hooks.
#[allow(dead_code)]
pub struct Binding {
    authority: program::Authority,
    runtime: Shared<host::runtime::Registry>,
    promotion: widget::Promotion,
    data: Dynamic,
    stamp: stamp::Stamp,
    timing: Option<timing::Session>,
}

/// One exact layout candidate awaiting host structural validation.
pub struct Pending {
    stamp: stamp::Stamp,
    generation: program::GenerationId,
    item: crate::item::Value,
    kind: widget::hook::Kind,
    state_type: Option<host::runtime::type_ref::Type>,
    output_type: host::runtime::type_ref::Type,
    receiver: Dynamic,
    state: Option<Dynamic>,
    output: Dynamic,
}

/// A pending layout output did not contain the requested owned representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    actual: &'static str,
}

impl Binding {
    pub(crate) fn new(
        authority: program::Authority,
        runtime: Shared<host::runtime::Registry>,
        promotion: widget::Promotion,
        data: Dynamic,
    ) -> Self {
        Self {
            authority,
            runtime,
            promotion,
            data,
            stamp: stamp::Stamp::issue(),
            timing: None,
        }
    }

    /// Attach one explicit host-owned timing observation to this binding.
    ///
    /// The complete authenticated authority invocation is observed. Omitted
    /// hooks and hook-specific binding validation that returns before dispatch
    /// remain unmeasured; adapter validation after authority return is outside
    /// the observation.
    #[doc(hidden)]
    #[must_use]
    pub fn observed(mut self, session: &timing::Session) -> Self {
        self.timing = Some(session.clone());
        self
    }

    /// Return the immutable executable generation that issued this binding.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.authority.generation()
    }

    /// Return the current canonical protocol proof used by this binding.
    #[must_use]
    pub const fn promotion(&self) -> &widget::Promotion {
        &self.promotion
    }

    /// Return the exact application Message instantiated for this binding.
    #[must_use]
    pub const fn message(&self) -> &application::Type {
        self.promotion.message()
    }

    /// Execute the exact optional Tree-local state initializer.
    pub fn state(&self) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self.promotion.definition().hook(widget::hook::Kind::State) else {
            return Ok(None);
        };
        let mut arguments = [self.data.clone()];
        self.dispatch(hook.item(), &mut arguments).map(Some)
    }

    /// Execute the exact optional direct-children transformation.
    pub fn children(&self) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::Children)
        else {
            return Ok(None);
        };
        let mut arguments = [self.data.clone()];
        self.dispatch(hook.item(), &mut arguments).map(Some)
    }

    /// Execute the exact optional state reconciliation hook.
    pub fn diff(&mut self, state: Option<&mut Dynamic>) -> Result<Option<()>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::Diff)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(expected_state) = self.change_state() else {
            let mut arguments = [self.data.clone()];
            drop(self.dispatch(hook.item(), &mut arguments)?);
            self.advance();
            self.data = arguments[0].clone();
            return Ok(Some(()));
        };
        let state = state
            .ok_or_else(|| change_error(hook.item(), widget::hook::change::Error::MissingState))?;
        let mut arguments = vec![self.data.clone(), state.clone()];
        let transport = self.dispatch(hook.item(), &mut arguments)?;
        let receiver = arguments[0].clone();
        drop(self.commit_change(
            &hook,
            widget::hook::Kind::Diff,
            &expected_state,
            receiver,
            state,
            transport,
        )?);
        Ok(Some(()))
    }

    /// Execute the exact optional requested-size hook.
    pub fn size(&self) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self.promotion.definition().hook(widget::hook::Kind::Size) else {
            return Ok(None);
        };
        let mut arguments = [self.data.clone()];
        self.dispatch(hook.item(), &mut arguments).map(Some)
    }

    /// Evaluate the exact optional layout transformation without committing it.
    pub fn layout(
        &self,
        state: Option<&Dynamic>,
        input: Dynamic,
    ) -> Result<Option<Pending>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::Layout)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(expected_state) = self.change_state() else {
            let mut arguments = vec![self.data.clone(), input];
            let output = self.dispatch(hook.item(), &mut arguments)?;
            return Ok(Some(Pending {
                stamp: self.stamp.clone(),
                generation: self.generation(),
                item: hook.item().clone(),
                kind: widget::hook::Kind::Layout,
                state_type: None,
                output_type: hook.output().clone(),
                receiver: arguments[0].clone(),
                state: None,
                output,
            }));
        };
        let state = state
            .ok_or_else(|| change_error(hook.item(), widget::hook::change::Error::MissingState))?;
        let mut arguments = vec![self.data.clone(), state.clone(), input];
        let transport = self.dispatch(hook.item(), &mut arguments)?;
        let receiver = arguments[0].clone();
        let values = self.open_change(
            &hook,
            widget::hook::Kind::Layout,
            &expected_state,
            transport,
        )?;
        Ok(Some(Pending {
            stamp: self.stamp.clone(),
            generation: self.generation(),
            item: hook.item().clone(),
            kind: widget::hook::Kind::Layout,
            state_type: Some(expected_state),
            output_type: hook.output().clone(),
            receiver,
            state: Some(values.state),
            output: values.output,
        }))
    }

    /// Execute the exact optional event transformation.
    pub fn update(
        &mut self,
        state: Option<&mut Dynamic>,
        input: Dynamic,
    ) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::Update)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(expected_state) = self.change_state() else {
            let mut arguments = vec![self.data.clone(), input];
            let result = self.dispatch(hook.item(), &mut arguments)?;
            self.advance();
            self.data = arguments[0].clone();
            return Ok(Some(result));
        };
        let state = state
            .ok_or_else(|| change_error(hook.item(), widget::hook::change::Error::MissingState))?;
        let mut arguments = vec![self.data.clone(), state.clone(), input];
        let transport = self.dispatch(hook.item(), &mut arguments)?;
        let receiver = arguments[0].clone();
        self.commit_change(
            &hook,
            widget::hook::Kind::Update,
            &expected_state,
            receiver,
            state,
            transport,
        )
        .map(Some)
    }

    /// Execute only this binding's retained Draw lifecycle hook.
    ///
    /// This is the renderer adapter handoff, not general evaluator control.
    /// The receiver, optional tree-local state, renderer lease, and draw input
    /// are all revalidated against the retained hook before exact unit success
    /// is returned. An omitted hook returns `None` for its inert protocol
    /// default.
    pub fn draw<T: crate::func::SendSync + 'static>(
        &self,
        state: Option<Dynamic>,
        renderer: Dynamic,
        input: Dynamic,
        invocation: &host::runtime::invocation::Invocation<T>,
    ) -> Result<Option<()>, program::Error> {
        let Some(hook) = self.promotion.definition().hook(widget::hook::Kind::Draw) else {
            return Ok(None);
        };
        let mut arguments = Vec::with_capacity(4);
        arguments.push(self.data.clone());
        arguments.extend(state);
        arguments.push(renderer);
        arguments.push(input);
        drop(self.dispatch_with(hook.item(), &mut arguments, invocation)?);
        Ok(Some(()))
    }

    /// Execute the exact optional pointer-interaction hook.
    pub fn mouse_interaction(
        &self,
        state: Option<Dynamic>,
        input: Dynamic,
    ) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::MouseInteraction)
        else {
            return Ok(None);
        };
        let mut arguments = Vec::with_capacity(3);
        arguments.push(self.data.clone());
        arguments.extend(state);
        arguments.push(input);
        self.dispatch(hook.item(), &mut arguments).map(Some)
    }

    /// Execute the exact optional bounded operation hook.
    pub fn operate(
        &mut self,
        state: Option<&mut Dynamic>,
        input: Dynamic,
    ) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::Operate)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(expected_state) = self.change_state() else {
            let mut arguments = vec![self.data.clone(), input];
            let result = self.dispatch(hook.item(), &mut arguments)?;
            self.advance();
            self.data = arguments[0].clone();
            return Ok(Some(result));
        };
        let state = state
            .ok_or_else(|| change_error(hook.item(), widget::hook::change::Error::MissingState))?;
        let mut arguments = vec![self.data.clone(), state.clone(), input];
        let transport = self.dispatch(hook.item(), &mut arguments)?;
        let receiver = arguments[0].clone();
        self.commit_change(
            &hook,
            widget::hook::Kind::Operate,
            &expected_state,
            receiver,
            state,
            transport,
        )
        .map(Some)
    }

    /// Execute the exact optional visual-overlay hook.
    pub fn overlay(
        &mut self,
        state: Option<&mut Dynamic>,
        input: Dynamic,
    ) -> Result<Option<Dynamic>, program::Error> {
        let Some(hook) = self
            .promotion
            .definition()
            .hook(widget::hook::Kind::Overlay)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(expected_state) = self.change_state() else {
            let mut arguments = vec![self.data.clone(), input];
            let result = self.dispatch(hook.item(), &mut arguments)?;
            self.advance();
            self.data = arguments[0].clone();
            return Ok(Some(result));
        };
        let state = state
            .ok_or_else(|| change_error(hook.item(), widget::hook::change::Error::MissingState))?;
        let mut arguments = vec![self.data.clone(), state.clone(), input];
        let transport = self.dispatch(hook.item(), &mut arguments)?;
        let receiver = arguments[0].clone();
        self.commit_change(
            &hook,
            widget::hook::Kind::Overlay,
            &expected_state,
            receiver,
            state,
            transport,
        )
        .map(Some)
    }

    fn dispatch(
        &self,
        item: &crate::item::Value,
        arguments: &mut [Dynamic],
    ) -> Result<Dynamic, program::Error> {
        let mut operation = || {
            self.authority
                .invoke_bound(item, arguments, self.runtime.clone())
        };
        match self.timing.as_ref() {
            Some(session) => session.observe_result(timing::Phase::ScriptedHookDispatch, operation),
            None => operation(),
        }
    }

    fn dispatch_with<T: crate::func::SendSync + 'static>(
        &self,
        item: &crate::item::Value,
        arguments: &mut [Dynamic],
        invocation: &host::runtime::invocation::Invocation<T>,
    ) -> Result<Dynamic, program::Error> {
        let mut operation = || {
            self.authority
                .invoke_bound_with(item, arguments, self.runtime.clone(), invocation)
        };
        match self.timing.as_ref() {
            Some(session) => session.observe_result(timing::Phase::ScriptedHookDispatch, operation),
            None => operation(),
        }
    }

    fn change_state(&self) -> Option<host::runtime::type_ref::Type> {
        self.promotion
            .definition()
            .state()
            .nominal_type()?
            .exact_runtime_type()
    }

    fn commit_change(
        &mut self,
        hook: &widget::hook::Definition,
        kind: widget::hook::Kind,
        state_type: &host::runtime::type_ref::Type,
        receiver: Dynamic,
        state: &mut Dynamic,
        transport: Dynamic,
    ) -> Result<Dynamic, program::Error> {
        let values = self.open_change(hook, kind, state_type, transport)?;
        self.advance();
        self.data = receiver;
        *state = values.state;
        Ok(values.output)
    }

    fn open_change(
        &self,
        hook: &widget::hook::Definition,
        kind: widget::hook::Kind,
        state_type: &host::runtime::type_ref::Type,
        transport: Dynamic,
    ) -> Result<widget::hook::change::Values, program::Error> {
        let change = transport
            .try_cast_result::<widget::hook::change::Change>()
            .map_err(|value| {
                change_error(hook.item(), widget::hook::change::Error::carrier(&value))
            })?;
        let values = change
            .open(
                self.generation(),
                hook.item(),
                kind,
                state_type,
                hook.output(),
            )
            .map_err(|error| change_error(hook.item(), error))?;
        self.authority.validate_bound_type(
            hook.item(),
            state_type,
            &values.state,
            &self.runtime,
        )?;
        self.authority.validate_bound_type(
            hook.item(),
            hook.output(),
            &values.output,
            &self.runtime,
        )?;
        Ok(values)
    }

    fn advance(&mut self) {
        self.stamp = stamp::Stamp::issue();
    }

    /// Translate one exact hook failure through retained source evidence.
    #[must_use]
    pub fn report(
        &self,
        kind: widget::hook::Kind,
        error: &program::Error,
    ) -> crate::diagnostic::Report {
        let evidence = self
            .promotion
            .definition()
            .hook(kind)
            .and_then(|hook| self.authority.item_evidence(hook.item()));
        let report = match evidence {
            Some(evidence) => match evidence.translate(error) {
                Some(original) => crate::diagnostic::Report::from_program_mapped(
                    evidence.source().clone(),
                    original,
                    error,
                ),
                None => crate::diagnostic::Report::from_program_at(
                    evidence.source().clone(),
                    evidence.declaration(),
                    error,
                ),
            },
            None => crate::diagnostic::Report::from_program(None, error),
        };
        report.unwrap_or_else(|_| {
            crate::diagnostic::Report::error(
                crate::diagnostic::code::RUNTIME_EVALUATION,
                error.to_string(),
            )
            .finish()
        })
    }

    /// Translate one adapter validation failure through the exact hook evidence.
    #[must_use]
    pub fn report_message(
        &self,
        kind: widget::hook::Kind,
        message: impl Into<String>,
    ) -> crate::diagnostic::Report {
        let Some(hook) = self.promotion.definition().hook(kind) else {
            return crate::diagnostic::Report::error(
                crate::diagnostic::code::RUNTIME_EVALUATION,
                message.into(),
            )
            .finish();
        };
        let error = program::Error::Evaluation {
            key: hook.item().key().clone(),
            error: crate::EvalAltResult::ErrorRuntime(message.into().into(), crate::Position::NONE)
                .into(),
        };
        self.report(kind, &error)
    }
}

impl Clone for Binding {
    fn clone(&self) -> Self {
        Self {
            authority: self.authority.clone(),
            runtime: self.runtime.clone(),
            promotion: self.promotion.clone(),
            data: self.data.clone(),
            stamp: stamp::Stamp::issue(),
            timing: self.timing.clone(),
        }
    }
}

impl Pending {
    /// Copy the already exact-type-validated declared layout output for inspection.
    pub fn output<T: Clone + 'static>(&self) -> Result<T, Error> {
        self.output
            .clone()
            .try_cast_result::<T>()
            .map_err(|value| Error {
                actual: value.type_name(),
            })
    }

    /// Commit one structurally accepted layout candidate to its exact binding.
    pub fn commit(
        self,
        binding: &mut Binding,
        state: Option<&mut Dynamic>,
    ) -> Result<(), program::Error> {
        let Some(hook) = binding
            .promotion
            .definition()
            .hook(widget::hook::Kind::Layout)
        else {
            return Err(pending_error(
                &self.item,
                "layout candidate has no current retained layout hook",
            ));
        };
        let exact = binding.stamp.matches(&self.stamp)
            && binding.generation() == self.generation
            && self.kind == widget::hook::Kind::Layout
            && hook.item() == &self.item
            && hook.output() == &self.output_type
            && binding.change_state() == self.state_type;
        if !exact {
            return Err(pending_error(
                &self.item,
                "layout candidate does not belong to the current exact widget binding",
            ));
        }
        binding.authority.validate_bound_type(
            hook.item(),
            hook.output(),
            &self.output,
            &binding.runtime,
        )?;

        let state = match (self.state_type.as_ref(), self.state, state) {
            (None, None, None) => None,
            (Some(expected), Some(candidate), Some(current)) => {
                binding.authority.validate_bound_type(
                    hook.item(),
                    expected,
                    current,
                    &binding.runtime,
                )?;
                binding.authority.validate_bound_type(
                    hook.item(),
                    expected,
                    &candidate,
                    &binding.runtime,
                )?;
                Some((current, candidate))
            }
            _ => {
                return Err(pending_error(
                    hook.item(),
                    "layout candidate Tree-local State does not match the current protocol",
                ));
            }
        };

        binding.advance();
        binding.data = self.receiver;
        if let Some((current, candidate)) = state {
            *current = candidate;
        }
        Ok(())
    }
}

impl Error {
    /// Return the rejected runtime representation name.
    #[must_use]
    pub const fn actual(&self) -> &'static str {
        self.actual
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "layout output has runtime type `{}`",
            self.actual
        )
    }
}

impl std::error::Error for Error {}

impl fmt::Debug for Pending {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Pending(<private>)")
    }
}

fn change_error(item: &crate::item::Value, error: widget::hook::change::Error) -> program::Error {
    program::Error::Evaluation {
        key: item.key().clone(),
        error: crate::EvalAltResult::ErrorRuntime(error.to_string().into(), crate::Position::NONE)
            .into(),
    }
}

fn pending_error(item: &crate::item::Value, message: &str) -> program::Error {
    program::Error::Evaluation {
        key: item.key().clone(),
        error: crate::EvalAltResult::ErrorRuntime(message.into(), crate::Position::NONE).into(),
    }
}

impl fmt::Debug for Binding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Binding")
            .field("generation", &self.generation())
            .field("site", self.promotion.site())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::Binding;
    use crate::types::nominal;
    use crate::{host, program, timing, widget, Dynamic, Engine, Shared};

    #[derive(Clone)]
    struct Open;

    #[test]
    fn observed_binding_counts_only_authenticated_dispatch() {
        let session = timing::Session::start();
        let (mut successful, mut successful_state) = fixture::open(false);
        assert!(successful.timing.is_none());
        successful = successful.observed(&session);

        assert!(successful.children().unwrap().is_none());
        assert!(successful.diff(None).is_err());
        assert_eq!(session.snapshot().attempts(), []);

        assert_eq!(
            successful.diff(Some(&mut successful_state)).unwrap(),
            Some(())
        );
        let (failing, mut failing_state) = fixture::open(true);
        let mut failing = failing.observed(&session);
        assert!(failing.diff(Some(&mut failing_state)).is_err());

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), 2);
        assert_eq!(
            snapshot.attempts()[0].phase(),
            timing::Phase::ScriptedHookDispatch
        );
        assert_eq!(snapshot.attempts()[0].outcome(), timing::Outcome::Success);
        assert_eq!(
            snapshot.attempts()[1].phase(),
            timing::Phase::ScriptedHookDispatch
        );
        assert_eq!(snapshot.attempts()[1].outcome(), timing::Outcome::Failure);
        assert_eq!(
            snapshot
                .aggregate(timing::Phase::ScriptedHookDispatch)
                .count(),
            2
        );
    }

    #[test]
    fn observed_authority_rejection_is_one_failed_dispatch_without_evaluator_entry() {
        let session = timing::Session::start();
        let (mut binding, mut state) = fixture::open(false);
        binding.authority = fixture::foreign_authority();
        binding = binding.observed(&session);

        let error = binding.diff(Some(&mut state)).unwrap_err();
        assert!(!error.to_string().contains("after mutation"));

        let snapshot = session.snapshot();
        assert_eq!(snapshot.attempts().len(), 1);
        assert_eq!(
            snapshot.attempts()[0].phase(),
            timing::Phase::ScriptedHookDispatch
        );
        assert_eq!(snapshot.attempts()[0].outcome(), timing::Outcome::Failure);
    }

    #[test]
    fn exact_stateful_change_commits_receiver_and_tree_state_together() {
        let (mut binding, mut state) = fixture::open(false);

        assert_eq!(field(binding.data(), 0), 0);
        assert_eq!(field(&state, 0), 0);
        assert_eq!(binding.diff(Some(&mut state)).unwrap(), Some(()));
        assert_eq!(field(binding.data(), 0), 7);
        assert_eq!(field(&state, 0), 9);
    }

    #[test]
    fn wrong_state_or_output_value_rolls_back_receiver_and_tree_state() {
        let (mut binding, mut state) = fixture::open(false);
        let hook = binding
            .promotion()
            .definition()
            .hook(widget::hook::Kind::Diff)
            .unwrap()
            .clone();
        let state_type = binding.change_state().unwrap();

        let wrong_state = change(
            &hook,
            &state_type,
            widget::hook::Kind::Diff,
            hook.item().clone(),
            binding.generation(),
            Dynamic::from(99_i64),
            Dynamic::UNIT,
        );
        let error = binding
            .commit_change(
                &hook,
                widget::hook::Kind::Diff,
                &state_type,
                changed(binding.data(), 31),
                &mut state,
                wrong_state,
            )
            .unwrap_err();
        assert!(
            error.to_string().contains("expected `main::State`"),
            "{error}"
        );
        assert_eq!(field(binding.data(), 0), 0);
        assert_eq!(field(&state, 0), 0);

        let wrong_output = change(
            &hook,
            &state_type,
            widget::hook::Kind::Diff,
            hook.item().clone(),
            binding.generation(),
            changed(&state, 37),
            Dynamic::from(41_i64),
        );
        let error = binding
            .commit_change(
                &hook,
                widget::hook::Kind::Diff,
                &state_type,
                changed(binding.data(), 43),
                &mut state,
                wrong_output,
            )
            .unwrap_err();
        assert!(error.to_string().contains("expected `()`"), "{error}");
        assert_eq!(field(binding.data(), 0), 0);
        assert_eq!(field(&state, 0), 0);
    }

    #[test]
    fn wrong_hook_or_generation_rolls_back_receiver_and_tree_state() {
        let (mut binding, mut state) = fixture::open(false);
        let hook = binding
            .promotion()
            .definition()
            .hook(widget::hook::Kind::Diff)
            .unwrap()
            .clone();
        let state_hook = binding
            .promotion()
            .definition()
            .hook(widget::hook::Kind::State)
            .unwrap()
            .item()
            .clone();
        let state_type = binding.change_state().unwrap();

        for (transport, expected) in [
            (
                change(
                    &hook,
                    &state_type,
                    widget::hook::Kind::Diff,
                    state_hook,
                    binding.generation(),
                    changed(&state, 47),
                    Dynamic::UNIT,
                ),
                "item mismatch",
            ),
            (
                change(
                    &hook,
                    &state_type,
                    widget::hook::Kind::Layout,
                    hook.item().clone(),
                    binding.generation(),
                    changed(&state, 53),
                    Dynamic::UNIT,
                ),
                "kind mismatch",
            ),
            (
                change(
                    &hook,
                    &state_type,
                    widget::hook::Kind::Diff,
                    hook.item().clone(),
                    program::GenerationId::issue(),
                    changed(&state, 59),
                    Dynamic::UNIT,
                ),
                "generation mismatch",
            ),
        ] {
            let error = binding
                .commit_change(
                    &hook,
                    widget::hook::Kind::Diff,
                    &state_type,
                    changed(binding.data(), 61),
                    &mut state,
                    transport,
                )
                .unwrap_err();
            assert!(error.to_string().contains(expected));
            assert_eq!(field(binding.data(), 0), 0);
            assert_eq!(field(&state, 0), 0);
        }
    }

    #[test]
    fn missing_state_or_malformed_transport_rolls_back_receiver_and_tree_state() {
        let (mut binding, mut state) = fixture::open(false);
        let error = binding.diff(None).unwrap_err();
        assert!(error.to_string().contains("missing Tree-local State"));
        assert_eq!(field(binding.data(), 0), 0);
        assert_eq!(field(&state, 0), 0);

        let hook = binding
            .promotion()
            .definition()
            .hook(widget::hook::Kind::Diff)
            .unwrap()
            .clone();
        let state_type = binding.change_state().unwrap();
        let error = binding
            .commit_change(
                &hook,
                widget::hook::Kind::Diff,
                &state_type,
                changed(binding.data(), 67),
                &mut state,
                Dynamic::UNIT,
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("expected private widget hook change"));
        assert_eq!(field(binding.data(), 0), 0);
        assert_eq!(field(&state, 0), 0);
    }

    #[test]
    fn script_error_after_mutation_rolls_back_receiver_and_tree_state() {
        let (mut binding, mut state) = fixture::open(true);

        assert!(binding.diff(Some(&mut state)).is_err());
        assert_eq!(field(binding.data(), 0), 0);
        assert_eq!(field(&state, 0), 0);
    }

    #[test]
    fn source_cannot_call_the_compiler_private_change_transport() {
        let facade = fixture::facade();
        let mut engine = Engine::new();
        engine.register_host_facade(facade).unwrap();
        engine.register_host_facade(fixture::lifecycle()).unwrap();
        let error = engine.compile(fixture::direct_call()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("lifecycle hooks are invoked only by the bound widget adapter"),
            "{error}"
        );
    }

    fn change(
        hook: &widget::hook::Definition,
        state_type: &host::runtime::type_ref::Type,
        kind: widget::hook::Kind,
        item: crate::item::Value,
        generation: program::GenerationId,
        state: Dynamic,
        output: Dynamic,
    ) -> Dynamic {
        Dynamic::from(widget::hook::change::Change::seal(
            generation,
            item,
            kind,
            state_type.clone(),
            hook.output().clone(),
            state,
            output,
        ))
    }

    fn changed(value: &Dynamic, field: i64) -> Dynamic {
        let mut value = value.clone();
        value.write_lock::<nominal::Struct>().unwrap().fields_mut()[0] = Dynamic::from(field);
        value
    }

    fn field(value: &Dynamic, index: usize) -> i64 {
        value.read_lock::<nominal::Struct>().unwrap().fields()[index]
            .as_int()
            .unwrap()
    }

    impl Binding {
        const fn data(&self) -> &Dynamic {
            &self.data
        }
    }

    mod fixture {
        use super::{Binding, Engine, Open, Shared};
        use crate::{host, Dynamic};

        pub(super) fn open(fails: bool) -> (Binding, Dynamic) {
            let facade = facade();
            let lifecycle = lifecycle();
            let mut engine = Engine::new();
            engine.register_host_facade(facade.clone()).unwrap();
            engine.register_host_facade(lifecycle.clone()).unwrap();
            let engine = Shared::new(engine);
            let authority = engine
                .compile_authority(if fails { failing() } else { successful() })
                .unwrap();
            let mut registry = host::runtime::Registry::new();
            registry
                .install(host::runtime::Builder::new(lifecycle).finish())
                .unwrap();
            registry
                .install(host::runtime::Builder::new(facade).finish())
                .unwrap();
            let registry = Shared::new(registry);
            let make = authority
                .items()
                .find(|item| item.key().name() == "make")
                .unwrap()
                .clone();
            let data = authority
                .invoke_bound(&make, &mut [], registry.clone())
                .unwrap();
            let promotion = authority.widget_promotions()[0].clone();
            let binding = Binding::new(authority, registry, promotion, data);
            let state = binding.state().unwrap().unwrap();
            (binding, state)
        }

        pub(super) fn facade() -> host::facade::Definition {
            let mut facade =
                host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap();
            facade.module("widget").unwrap();
            facade.type_::<Open>("Element", &["Message"], &[]).unwrap();
            facade.finish().unwrap()
        }

        pub(super) fn lifecycle() -> host::facade::Definition {
            host::facade::Builder::new("rawr_iced", host::facade::Abi::new(1).unwrap())
                .unwrap()
                .finish()
                .unwrap()
        }

        pub(super) fn foreign_authority() -> crate::program::Authority {
            let facade = facade();
            let mut engine = Engine::new();
            engine.register_host_facade(facade).unwrap();
            engine.register_host_facade(lifecycle()).unwrap();
            let engine = Shared::new(engine);
            engine.compile_authority(foreign()).unwrap()
        }

        fn successful() -> &'static str {
            r#"
                struct State { value: i64 }
                struct Dial { value: i64 }
                enum Message { Tick }

                #[widget]
                impl Dial {
                    fn state(&self) -> State { State { value: 0 } }

                    fn diff(&mut self, state: &mut State) {
                        self.value = 7;
                        state.value = 9;
                    }
                }

                pub fn make() -> Dial { Dial { value: 0 } }
                pub fn view() -> iced::Element<Message> { Dial { value: 0 } }
            "#
        }

        fn failing() -> &'static str {
            r#"
                struct State { value: i64 }
                struct Dial { value: i64 }
                enum Message { Tick }

                #[widget]
                impl Dial {
                    fn state(&self) -> State { State { value: 0 } }

                    fn diff(&mut self, state: &mut State) {
                        self.value = 7;
                        state.value = 9;
                        panic!("after mutation");
                    }
                }

                pub fn make() -> Dial { Dial { value: 0 } }
                pub fn view() -> iced::Element<Message> { Dial { value: 0 } }
            "#
        }

        fn foreign() -> &'static str {
            r#"
                struct State { value: i64 }
                struct Knob { value: i64 }
                enum Message { Tick }

                #[widget]
                impl Knob {
                    fn state(&self) -> State { State { value: 0 } }

                    fn diff(&mut self, state: &mut State) {
                        self.value = 7;
                        state.value = 9;
                        panic!("after mutation");
                    }
                }

                pub fn view() -> iced::Element<Message> { Knob { value: 0 } }
            "#
        }

        pub(super) fn direct_call() -> &'static str {
            r#"
                struct State { value: i64 }
                struct Dial { value: i64 }
                enum Message { Tick }

                #[widget]
                impl Dial {
                    fn state(&self) -> State { State { value: 0 } }
                    fn diff(&mut self, state: &mut State) {}
                }

                impl Dial {
                    fn illegal(&mut self) {
                        self.diff(State { value: 0 });
                    }
                }

                pub fn view() -> iced::Element<Message> { Dial { value: 0 } }
            "#
        }
    }
}
