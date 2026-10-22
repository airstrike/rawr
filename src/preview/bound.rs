//! Preview-specific execution through exact launch-bound authority.

use crate::{application, host, preview, program, Shared};
#[cfg(not(feature = "no_function"))]
use crate::{item, Dynamic};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A preview catalog whose complete native runtime boundary is proven.
pub struct BoundCatalog {
    authority: program::Authority,
    #[cfg_attr(feature = "no_function", allow(dead_code))]
    runtime: Shared<host::runtime::Registry>,
    application: Option<application::BoundLaunch>,
}

impl BoundCatalog {
    /// Resolve one non-authoritative generated event-mapper seal against this
    /// exact catalog generation.
    #[doc(hidden)]
    pub fn profile_event_mapper(&self, seal: &[u8; 16]) -> Option<item::Value> {
        self.authority.items().find_map(|item| {
            (crate::preview::digest::event_mapper(item).ok().as_ref() == Some(seal))
                .then(|| item.clone())
        })
    }

    pub(crate) fn new(
        authority: program::Authority,
        runtime: Shared<host::runtime::Registry>,
        application: Option<application::BoundLaunch>,
    ) -> Self {
        Self {
            authority,
            runtime,
            application,
        }
    }

    /// Return the optional real application selection.
    #[must_use]
    pub const fn application(&self) -> Option<&application::BoundLaunch> {
        self.application.as_ref()
    }

    /// Call the root package's `fn main` once, for a program that is not an
    /// application. `None` when the package has no `main`.
    #[cfg(not(feature = "no_function"))]
    #[doc(hidden)]
    pub fn invoke_main(&self) -> Option<Result<Dynamic, program::Error>> {
        let package = self.authority.package();
        let main = self.authority.items().find(|item| {
            let key = item.key();
            item.kind() == crate::item::Kind::Function
                && key.package() == package
                && key.modules().is_empty()
                && key.name() == "main"
        })?;
        Some(
            self.authority
                .invoke_bound(main, &mut [], self.runtime.clone()),
        )
    }

    /// Return compiler-validated previews in stable key order.
    #[must_use]
    pub fn previews(&self) -> &[preview::Definition] {
        self.authority.previews()
    }

    /// Return the immutable generation shared by every selection.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.authority.generation()
    }

    /// Return the exact compiler-selected root package name.
    #[must_use]
    pub fn package(&self) -> &str {
        self.authority.package()
    }

    /// Materialize the exact compiler-proven fixture for one preview.
    #[cfg(not(feature = "no_function"))]
    pub fn seed(&self, key: &preview::Key) -> Result<Option<Dynamic>, preview::RuntimeError> {
        let definition = self.definition(key)?;
        match definition.seed() {
            preview::Seed::None => Ok(None),
            preview::Seed::Default => {
                let ty = input_type(definition).ok_or(preview::RuntimeError::UnknownPreview)?;
                let value = self
                    .authority
                    .materialize_default(ty)
                    .map_err(preview::RuntimeError::SeedConstruction)?;
                self.validate(definition.subject().item(), ty, &value)?;
                Ok(Some(value))
            }
            preview::Seed::Callback(callback) => {
                let value = self.invoke(callback.item(), &mut [])?;
                let ty = input_type(definition).ok_or(preview::RuntimeError::UnknownPreview)?;
                self.validate(callback.item(), ty, &value)?;
                Ok(Some(value))
            }
        }
    }

    /// Invoke and normalize one exact preview subject.
    #[cfg(not(feature = "no_function"))]
    pub fn subject(
        &self,
        key: &preview::Key,
        fixture: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        let definition = self.definition(key)?;
        let mut arguments = match definition.input() {
            preview::Input::Stateless => Vec::new(),
            preview::Input::Shared(ty) | preview::Input::Owned(ty) => {
                let fixture = fixture.ok_or(preview::RuntimeError::UnknownPreview)?;
                self.validate(definition.subject().item(), ty, fixture)?;
                match (definition.subject().parameters().len(), ty.tuple_members()) {
                    // `bind` arguments: one tuple spread over the parameters.
                    (count, Some(members)) if count > 1 && count == members.len() => fixture
                        .read_lock::<crate::Array>()
                        .ok_or(preview::RuntimeError::UnknownPreview)?
                        .to_vec(),
                    _ => vec![fixture.clone()],
                }
            }
        };
        let callback = definition.subject().item();
        let value = self.invoke(callback, &mut arguments)?;
        match definition.projection_site() {
            Some(projection) => self
                .authority
                .project_bound(callback, projection, value, self.runtime.clone())
                .map_err(preview::RuntimeError::Invocation),
            None => {
                self.authority
                    .validate_bound_type(
                        callback,
                        definition.runtime_output(),
                        &value,
                        &self.runtime,
                    )
                    .map_err(preview::RuntimeError::Invocation)?;
                Ok(value)
            }
        }
    }

    /// Invoke one exact preview update and retain its optional durable Task value.
    #[cfg(not(feature = "no_function"))]
    pub fn update(
        &self,
        key: &preview::Key,
        state: &Dynamic,
        message: Dynamic,
    ) -> Result<preview::Update, preview::RuntimeError> {
        let definition = self.definition(key)?;
        let callback = definition
            .update()
            .ok_or(preview::RuntimeError::UnknownPreview)?;
        let state_type = input_type(definition).ok_or(preview::RuntimeError::UnknownPreview)?;
        self.validate(callback.item(), state_type, state)?;
        self.validate(callback.item(), definition.message(), &message)?;
        let mut arguments = [state.clone(), message];
        let output = self.invoke(callback.item(), &mut arguments)?;
        self.validate(callback.item(), state_type, &arguments[0])?;
        self.validate(callback.item(), callback.output(), &output)?;
        Ok(preview::Update::new(
            arguments[0].take(),
            (!callback.output().is_unit()).then_some(output),
        ))
    }

    /// Evaluate one exact explicit preview theme choice.
    #[cfg(not(feature = "no_function"))]
    pub fn theme(
        &self,
        key: &preview::Key,
        index: usize,
        state: Option<&Dynamic>,
    ) -> Result<Dynamic, preview::RuntimeError> {
        let definition = self.definition(key)?;
        let theme = definition
            .themes()
            .get(index)
            .ok_or(preview::RuntimeError::UnknownPreview)?;
        let callback = theme.callback();
        let mut arguments = match callback.receiver() {
            None => Vec::new(),
            Some(crate::receiver::Mode::Shared) => {
                let state = state.ok_or(preview::RuntimeError::UnknownPreview)?;
                let ty = input_type(definition).ok_or(preview::RuntimeError::UnknownPreview)?;
                self.validate(callback.item(), ty, state)?;
                vec![state.clone()]
            }
            Some(crate::receiver::Mode::Owned | crate::receiver::Mode::Mutable) => {
                return Err(preview::RuntimeError::UnknownPreview);
            }
        };
        let value = self.invoke(callback.item(), &mut arguments)?;
        self.validate(callback.item(), callback.output(), &value)?;
        Ok(value)
    }

    /// Map a child payload through one compiler-proven symbolic mapper.
    #[cfg(not(feature = "no_function"))]
    pub fn map_message(
        &self,
        key: &preview::Key,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, preview::RuntimeError> {
        let definition = self.definition(key)?;
        let value = self.invoke(mapper, &mut [payload])?;
        self.validate(mapper, definition.message(), &value)?;
        Ok(value)
    }

    /// Issue one compiler-proven host operation under this exact preview authority.
    #[cfg(not(feature = "no_function"))]
    pub fn issue_operation(
        &self,
        key: &preview::Key,
        call: host::operation::Call,
        mapper: &item::Value,
    ) -> Result<host::operation::Pending, host::operation::Error> {
        let _definition =
            self.definition(key)
                .map_err(|_| host::operation::Error::MessageContract {
                    message: "unknown preview Message".into(),
                })?;
        let request = host::operation::Request::new(call, mapper);
        self.authority
            .issue_operation(request, self.runtime.clone())
    }

    /// Invoke one task mapper without treating its intermediate output as a preview Message.
    #[cfg(not(feature = "no_function"))]
    pub fn map_task_value(
        &self,
        key: &preview::Key,
        mapper: &item::Value,
        payload: Dynamic,
    ) -> Result<Dynamic, preview::RuntimeError> {
        let _definition = self.definition(key)?;
        self.invoke(mapper, &mut [payload])
    }

    /// Invoke one compiler-proven function, such as a style function, with
    /// its arguments in order.
    #[cfg(not(feature = "no_function"))]
    pub fn invoke_function(
        &self,
        key: &preview::Key,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> Result<Dynamic, preview::RuntimeError> {
        let _definition = self.definition(key)?;
        self.invoke(function, arguments)
    }

    /// Validate one value against an exact argument carried by a runtime witness.
    #[cfg(not(feature = "no_function"))]
    pub fn validate_witness_argument(
        &self,
        witness: &host::runtime::Witness,
        index: usize,
        value: &Dynamic,
    ) -> Result<(), host::runtime::ValueError> {
        self.authority
            .validate_witness_argument(witness, index, value, &self.runtime)
    }

    /// Validate one completed task payload against this preview's exact Message.
    #[cfg(not(feature = "no_function"))]
    pub fn validate_message(
        &self,
        key: &preview::Key,
        value: &Dynamic,
    ) -> Result<(), preview::RuntimeError> {
        let definition = self.definition(key)?;
        self.validate(definition.subject().item(), definition.message(), value)
    }

    /// Resolve one preview location against this exact compiled source table.
    pub fn source(
        &self,
        location: preview::Location,
    ) -> Result<crate::diagnostic::Source, preview::RuntimeError> {
        self.authority
            .preview_source(location)
            .cloned()
            .map_err(|error| match error {
                program::source::Error::ForeignTable => preview::RuntimeError::ForeignSource,
                program::source::Error::MissingSource => preview::RuntimeError::MissingSource,
            })
    }

    /// Return every compiler-selected nominal-widget promotion site.
    pub fn widget_promotions(&self) -> &[crate::widget::Promotion] {
        self.authority.widget_promotions()
    }

    #[cfg(not(feature = "no_function"))]
    /// Promote owned nominal data through one current preview widget site proof.
    pub fn promote_widget(
        &self,
        key: &preview::Key,
        promotion: &crate::widget::Promotion,
        data: Dynamic,
    ) -> Result<crate::widget::Descriptor, crate::widget::Error> {
        let definition = self
            .definition(key)
            .map_err(|_| crate::widget::Error::Preview)?;
        self.authority
            .promote_widget(promotion, data, definition.message(), &self.runtime)
    }

    #[cfg(not(feature = "no_function"))]
    /// Bind one durable scripted widget to this exact preview selection.
    pub fn bind_widget(
        &self,
        key: &preview::Key,
        descriptor: &crate::widget::Descriptor,
    ) -> Result<crate::widget::Binding, crate::widget::Error> {
        let _definition = self
            .definition(key)
            .map_err(|_| crate::widget::Error::Preview)?;
        self.authority.bind_widget(descriptor, self.runtime.clone())
    }

    #[cfg(not(feature = "no_function"))]
    fn definition(
        &self,
        key: &preview::Key,
    ) -> Result<&preview::Definition, preview::RuntimeError> {
        self.previews()
            .iter()
            .find(|definition| definition.key() == key)
            .ok_or(preview::RuntimeError::UnknownPreview)
    }

    #[cfg(not(feature = "no_function"))]
    fn invoke(
        &self,
        item: &item::Value,
        arguments: &mut [Dynamic],
    ) -> Result<Dynamic, preview::RuntimeError> {
        self.authority
            .invoke_bound(item, arguments, self.runtime.clone())
            .map_err(preview::RuntimeError::Invocation)
    }

    #[cfg(not(feature = "no_function"))]
    fn validate(
        &self,
        item: &item::Value,
        ty: &application::Type,
        value: &Dynamic,
    ) -> Result<(), preview::RuntimeError> {
        let runtime = ty
            .exact_runtime_type()
            .ok_or(preview::RuntimeError::UnknownPreview)?;
        self.authority
            .validate_bound_type(item, &runtime, value, &self.runtime)
            .map_err(preview::RuntimeError::Invocation)
    }
}

#[cfg(not(feature = "no_function"))]
fn input_type(definition: &preview::Definition) -> Option<&application::Type> {
    match definition.input() {
        preview::Input::Stateless => None,
        preview::Input::Shared(input) | preview::Input::Owned(input) => Some(input),
    }
}

impl fmt::Debug for BoundCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundCatalog")
            .field("has_application", &self.application.is_some())
            .field("preview_count", &self.previews().len())
            .field("generation", &self.generation())
            .finish_non_exhaustive()
    }
}
