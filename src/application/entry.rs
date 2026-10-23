//! Stable metadata selected by restricted application-root resolution.

use crate::{host, item, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// Supported restricted application-root form.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Form {
    /// `iced::run(update, view)` with a later Default-backed boot proof.
    Run,
    /// `iced::application(boot, update, view).run()` with an explicit boot item.
    Application,
}

/// One compiler-selected application root before callback-contract validation.
///
/// This proof owns only stable symbolic callback and static facade metadata.
/// [`crate::application::Candidate`] keeps it paired with the exact executable;
/// Phase 3 contract validation promotes that intact candidate to the final
/// descriptor.
#[derive(Clone, Debug)]
pub struct Entry {
    form: Form,
    facade: host::facade::Dependency,
    root_intrinsic: host::item::Fingerprint,
    title_intrinsic: Option<host::item::Fingerprint>,
    theme_intrinsic: Option<host::item::Fingerprint>,
    subscription_intrinsic: Option<host::item::Fingerprint>,
    centered_intrinsic: Option<host::item::Fingerprint>,
    run_intrinsic: Option<host::item::Fingerprint>,
    boot: Option<item::Value>,
    prelude: Option<(item::Value, Box<str>)>,
    update: item::Value,
    view: item::Value,
    title: Option<item::Value>,
    theme: Option<item::Value>,
    subscription: Option<item::Value>,
    centered: bool,
    task: Option<host::type_def::Contract>,
    theme_type: Option<host::type_def::Contract>,
    subscription_type: Option<host::type_def::Contract>,
    widget: Option<host::type_def::Contract>,
    span: Span,
}

impl Entry {
    pub(crate) fn new(
        form: Form,
        facade: host::facade::Dependency,
        root_intrinsic: host::item::Fingerprint,
        title_intrinsic: Option<host::item::Fingerprint>,
        theme_intrinsic: Option<host::item::Fingerprint>,
        subscription_intrinsic: Option<host::item::Fingerprint>,
        centered_intrinsic: Option<host::item::Fingerprint>,
        run_intrinsic: Option<host::item::Fingerprint>,
        boot: Option<item::Value>,
        prelude: Option<(item::Value, Box<str>)>,
        update: item::Value,
        view: item::Value,
        title: Option<item::Value>,
        theme: Option<item::Value>,
        subscription: Option<item::Value>,
        centered: bool,
        task: Option<host::type_def::Contract>,
        theme_type: Option<host::type_def::Contract>,
        subscription_type: Option<host::type_def::Contract>,
        widget: Option<host::type_def::Contract>,
        span: Span,
    ) -> Self {
        Self {
            form,
            facade,
            root_intrinsic,
            title_intrinsic,
            theme_intrinsic,
            subscription_intrinsic,
            centered_intrinsic,
            run_intrinsic,
            boot,
            prelude,
            update,
            view,
            title,
            theme,
            subscription,
            centered,
            task,
            theme_type,
            subscription_type,
            widget,
            span,
        }
    }

    /// Return the restricted source form selected by the compiler.
    #[must_use]
    pub const fn form(&self) -> Form {
        self.form
    }

    /// Return the exact installed facade ABI proof used for selection.
    #[must_use]
    pub const fn facade(&self) -> &host::facade::Fingerprint {
        self.facade.fingerprint()
    }

    pub(crate) const fn dependency(&self) -> &host::facade::Dependency {
        &self.facade
    }

    /// Return the exact `iced::run` or `iced::application` intrinsic proof.
    #[must_use]
    pub const fn root_intrinsic(&self) -> &host::item::Fingerprint {
        &self.root_intrinsic
    }

    /// Return the terminal `Application::run` proof for explicit boot form.
    #[must_use]
    pub const fn run_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.run_intrinsic.as_ref()
    }

    /// Return the optional application-title intrinsic proof.
    #[must_use]
    pub const fn title_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.title_intrinsic.as_ref()
    }

    /// Return the optional application-theme intrinsic proof.
    #[must_use]
    pub const fn theme_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.theme_intrinsic.as_ref()
    }

    /// Return the optional application-subscription intrinsic proof.
    #[must_use]
    pub const fn subscription_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.subscription_intrinsic.as_ref()
    }

    /// Return the optional application-centered intrinsic proof.
    #[must_use]
    pub const fn centered_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.centered_intrinsic.as_ref()
    }

    /// Return the explicit boot candidate, when present.
    #[must_use]
    pub const fn boot(&self) -> Option<&item::Value> {
        self.boot.as_ref()
    }

    /// Return `main`'s statements before the entry and their text, when
    /// present.
    #[must_use]
    pub fn prelude(&self) -> Option<(&item::Value, &str)> {
        self.prelude.as_ref().map(|(value, text)| (value, &**text))
    }

    /// Return the stable symbolic update candidate.
    #[must_use]
    pub const fn update(&self) -> &item::Value {
        &self.update
    }

    /// Return the stable symbolic view candidate.
    #[must_use]
    pub const fn view(&self) -> &item::Value {
        &self.view
    }

    /// Return the optional stable symbolic title callback.
    #[must_use]
    pub const fn title(&self) -> Option<&item::Value> {
        self.title.as_ref()
    }

    /// Return the optional stable symbolic theme callback.
    #[must_use]
    pub const fn theme(&self) -> Option<&item::Value> {
        self.theme.as_ref()
    }

    /// Return the optional stable symbolic subscription callback.
    #[must_use]
    pub const fn subscription(&self) -> Option<&item::Value> {
        self.subscription.as_ref()
    }

    /// Return whether the application requests centered initial placement.
    #[must_use]
    pub const fn centered(&self) -> bool {
        self.centered
    }

    pub(crate) const fn task(&self) -> Option<&host::type_def::Contract> {
        self.task.as_ref()
    }

    pub(crate) const fn theme_type(&self) -> Option<&host::type_def::Contract> {
        self.theme_type.as_ref()
    }

    pub(crate) const fn subscription_type(&self) -> Option<&host::type_def::Contract> {
        self.subscription_type.as_ref()
    }

    pub(crate) const fn widget(&self) -> Option<&host::type_def::Contract> {
        self.widget.as_ref()
    }

    /// Return the complete accepted root-expression span.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}
