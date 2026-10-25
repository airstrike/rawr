//! Open durable widget descriptors lowered into transient iced elements.

mod adapter;
mod angle;
mod animation;
pub(crate) mod builtin;
#[cfg(feature = "canvas")]
mod canvas;
pub mod children;
mod column;
mod component;
pub(crate) mod custom;
pub(crate) mod diagnostic;
pub mod draw;
pub mod event;
mod gradient;
mod helpers;
mod icon;
pub(crate) mod key;
pub mod layout;
mod mapped;
mod media;
pub mod mouse;
pub mod operation;
pub mod overlay;
mod protocol;
pub(crate) mod reported;
mod rich;
pub(crate) mod script;
mod scroll;
pub(crate) mod style;
mod table;
mod value;

pub use value::{Error, Value};

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use iced::widget::Widget as _;
use rawr::{application, item, preview, program, timing, Dynamic};

use crate::runtime;
use crate::theme;

pub(crate) fn bind(
    iced: &mut rawr::host::runtime::Builder,
    rawr: &mut rawr::host::runtime::Builder,
) {
    builtin::bind(iced, rawr);
    #[cfg(feature = "canvas")]
    if crate::features::on(crate::features::Feature::Canvas) {
        canvas::bind(iced);
    }
    component::bind(iced);
    custom::bind(iced);
    protocol::bind(iced, rawr);
    script::bind(iced);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn bind_profile(
    iced: &mut rawr::host::runtime::Builder,
    rawr: &mut rawr::host::runtime::Builder,
    _element: crate::profile::runner::ElementReplacement,
) {
    builtin::bind_profile(iced, rawr);
    #[cfg(feature = "canvas")]
    if crate::features::on(crate::features::Feature::Canvas) {
        canvas::bind(iced);
    }
    component::bind(iced);
    custom::bind(iced);
    protocol::bind_profile(iced, rawr);
    script::bind(iced);
}

/// One opaque logical identity owned by a widget descriptor.
///
/// Identity is never derived from an allocation address or from serialized
/// descriptor structure. A descriptor must return the same identity for its
/// lifetime. [`Value`] snapshots it during construction so clones remain
/// stable even if an implementation violates that contract later.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity(Arc<Kind>);

#[derive(Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Kind {
    Unique(usize),
    Script(rawr::widget::Site),
}

/// The authority-limited services available while lowering one descriptor.
pub struct Context<'a> {
    instance: application::Instance,
    generation: program::GenerationId,
    authority: MappingAuthority,
    diagnostics: Arc<diagnostic::Sink>,
    theme: &'a theme::Value,
    timing: Option<timing::Session>,
}

/// Owned provenance stamp for messages produced by one freshly lowered native widget.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct NativeMapper {
    instance: application::Instance,
    generation: program::GenerationId,
}

/// One transient profile-authenticated script event mapper rebound through the
/// current lowering authority. It is created only during a view region and is
/// never retained in an imported native arena.
#[doc(hidden)]
#[derive(Clone)]
pub struct ProfileEventMapper(mapping::Mapper);

#[derive(Clone)]
pub(crate) enum MappingAuthority {
    Unavailable,
    Application {
        authority: Box<rawr::reload::installation::installed::Application>,
        boundary: mapping::Boundary,
    },
    Preview {
        catalog: crate::preview::catalog::Authority,
        key: preview::Key,
        boundary: mapping::Boundary,
    },
}

mod mapping {
    use rawr::{application, item, program, Dynamic};

    use crate::runtime;

    #[derive(Clone, Copy)]
    pub(crate) enum Boundary {
        Final,
        Intermediate,
    }

    /// One exact symbolic message mapping bundled with its native owner stamp.
    ///
    /// Built-in controls construct this proof while lowering and move it into
    /// the native iced event closure. The symbolic mapper remains inert until
    /// iced produces an input value.
    #[derive(Clone)]
    pub(crate) struct Mapper {
        instance: application::Instance,
        generation: program::GenerationId,
        authority: super::MappingAuthority,
        mapper: item::Value,
    }

    impl Mapper {
        pub(crate) fn new(
            instance: application::Instance,
            generation: program::GenerationId,
            authority: super::MappingAuthority,
            mapper: item::Value,
        ) -> Self {
            Self {
                instance,
                generation,
                authority,
                mapper,
            }
        }

        pub(crate) fn map(&self, input: Dynamic) -> runtime::Message {
            self.authority.map_message(
                runtime::Message::new(self.instance, self.generation, input),
                &self.mapper,
            )
        }
    }
}

pub(crate) trait ApplicationAuthority {
    fn attenuate(&self) -> rawr::reload::installation::installed::Application;
    fn generation(&self) -> program::GenerationId;
}

impl ApplicationAuthority for rawr::reload::installation::installed::Application {
    fn attenuate(&self) -> rawr::reload::installation::installed::Application {
        self.clone()
    }

    fn generation(&self) -> program::GenerationId {
        self.generation()
    }
}

#[cfg(test)]
impl ApplicationAuthority for Arc<application::BoundLaunch> {
    fn attenuate(&self) -> rawr::reload::installation::installed::Application {
        rawr::reload::installation::installed::Application::from_bound(self.as_ref().clone())
    }

    fn generation(&self) -> program::GenerationId {
        self.as_ref().generation()
    }
}

/// A durable open widget description that can build a fresh iced element.
///
/// Implementations own all data borrowed by the returned element and must not
/// cache an [`iced::Element`]. Native and built-in descriptors remain inert;
/// the private scripted descriptor binds its compiler-proven lifecycle through
/// the current lowering context and stores persistent values only in iced
/// [`Tree`](iced::advanced::widget::Tree) state.
///
/// Native extensions implement this trait in their own crate, seal an instance
/// with [`Value::new`], and stamp every Rawr payload through
/// [`Context::message`]. No built-in widget registration or dispatch change is
/// required.
pub trait Descriptor: Send + Sync + 'static {
    /// Lower this durable description into a fresh transient iced element.
    fn lower(&self, context: &Context<'_>) -> iced::Element<'static, runtime::Message>;

    /// Return this descriptor's stable logical identity.
    fn identity(&self) -> Identity;
}

impl Identity {
    /// Issue an opaque identity for a deliberately new logical widget.
    #[must_use]
    pub fn unique() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        let issued = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(Arc::new(Kind::Unique(issued)))
    }

    pub(crate) fn script(site: rawr::widget::Site) -> Self {
        Self(Arc::new(Kind::Script(site)))
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Identity(<private>)")
    }
}

impl<'a> Context<'a> {
    pub(crate) fn unavailable(
        instance: application::Instance,
        generation: program::GenerationId,
        diagnostics: &Arc<diagnostic::Sink>,
        theme: &'a theme::Value,
    ) -> Self {
        Self {
            instance,
            generation,
            authority: MappingAuthority::Unavailable,
            diagnostics: diagnostics.clone(),
            theme,
            timing: None,
        }
    }

    pub(crate) fn application(
        authority: &impl ApplicationAuthority,
        instance: application::Instance,
        diagnostics: &Arc<diagnostic::Sink>,
        theme: &'a theme::Value,
    ) -> Self {
        debug_assert!(diagnostics.owns(instance, authority.generation()));
        Self {
            instance,
            generation: authority.generation(),
            authority: MappingAuthority::Application {
                authority: Box::new(authority.attenuate()),
                boundary: mapping::Boundary::Final,
            },
            diagnostics: diagnostics.clone(),
            theme,
            timing: None,
        }
    }

    pub(crate) fn preview(
        catalog: &'a crate::preview::catalog::Authority,
        key: &'a preview::Key,
        instance: application::Instance,
        diagnostics: &Arc<diagnostic::Sink>,
        theme: &'a theme::Value,
    ) -> Self {
        debug_assert!(diagnostics.owns(instance, catalog.generation()));
        Self {
            instance,
            generation: catalog.generation(),
            authority: MappingAuthority::Preview {
                catalog: catalog.clone(),
                key: key.clone(),
                boundary: mapping::Boundary::Final,
            },
            diagnostics: diagnostics.clone(),
            theme,
            timing: None,
        }
    }

    pub(crate) fn observed(mut self, session: &timing::Session) -> Self {
        self.timing = Some(session.clone());
        self
    }

    /// Return the application instance receiving lowered messages.
    #[must_use]
    pub const fn instance(&self) -> application::Instance {
        self.instance
    }

    /// Return the executable generation receiving lowered messages.
    #[must_use]
    pub const fn generation(&self) -> program::GenerationId {
        self.generation
    }

    /// Stamp one opaque Rawr payload for delivery to the current owner pair.
    #[must_use]
    pub fn message(&self, payload: Dynamic) -> runtime::Message {
        runtime::Message::new(self.instance, self.generation, payload)
    }

    /// Attenuate this lowering context to an owned native-event message stamp.
    #[doc(hidden)]
    #[must_use]
    pub const fn native_mapper(&self) -> NativeMapper {
        NativeMapper {
            instance: self.instance,
            generation: self.generation,
        }
    }

    /// Rebind one generated non-authoritative mapper seal to this exact
    /// application/preview generation.
    #[doc(hidden)]
    #[must_use]
    pub fn profile_event_mapper(&self, seal: &[u8; 16]) -> Option<ProfileEventMapper> {
        let mapper = match &self.authority {
            MappingAuthority::Unavailable => return None,
            MappingAuthority::Application { authority, .. } => {
                authority.profile_event_mapper(seal)?
            }
            MappingAuthority::Preview { catalog, .. } => catalog.profile_event_mapper(seal)?,
        };
        Some(ProfileEventMapper(self.message_mapper(mapper)))
    }

    /// Return the fully materialized inert application theme.
    #[must_use]
    pub const fn theme(&self) -> &theme::Value {
        self.theme
    }

    /// Invoke one compiler-proven function, such as a style function, while
    /// lowering. `None` without an invocation authority.
    pub(crate) fn invoke(
        &self,
        function: &item::Value,
        arguments: &mut [Dynamic],
    ) -> Option<Result<Dynamic, String>> {
        Some(match &self.authority {
            MappingAuthority::Unavailable => return None,
            MappingAuthority::Application { authority, .. } => authority
                .invoke(function, arguments)
                .map_err(|error| error.to_string()),
            MappingAuthority::Preview { catalog, key, .. } => catalog
                .invoke_function(key, function, arguments)
                .map_err(|error| error.to_string()),
        })
    }

    pub(crate) fn mapping_authority(&self) -> MappingAuthority {
        self.authority.clone()
    }

    pub(crate) fn message_mapper(&self, mapper: item::Value) -> mapping::Mapper {
        mapping::Mapper::new(
            self.instance,
            self.generation,
            self.authority.clone(),
            mapper,
        )
    }

    pub(crate) fn child_mapping(&self) -> Self {
        Self {
            instance: self.instance,
            generation: self.generation,
            authority: self.authority.intermediate(),
            diagnostics: self.diagnostics.clone(),
            theme: self.theme,
            timing: self.timing.clone(),
        }
    }

    /// This context with its theme owned, for a widget that lowers its
    /// content later, as `responsive` does at layout.
    pub(crate) fn owned(&self) -> OwnedContext {
        OwnedContext {
            instance: self.instance,
            generation: self.generation,
            authority: self.authority.clone(),
            diagnostics: self.diagnostics.clone(),
            theme: self.theme.clone(),
            timing: self.timing.clone(),
        }
    }

    pub(crate) fn diagnostics(&self) -> Arc<diagnostic::Sink> {
        self.diagnostics.clone()
    }

    pub(crate) fn timing(&self) -> Option<&timing::Session> {
        self.timing.as_ref()
    }

    #[allow(dead_code)]
    pub(crate) fn promote_script(
        &self,
        promotion: &rawr::widget::Promotion,
        data: Dynamic,
    ) -> Result<rawr::widget::Descriptor, rawr::widget::Error> {
        self.authority.promote_widget(promotion, data)
    }

    #[allow(dead_code)]
    pub(crate) fn bind_script(
        &self,
        descriptor: &rawr::widget::Descriptor,
    ) -> Result<rawr::widget::Binding, rawr::widget::Error> {
        self.authority
            .bind_widget(descriptor)
            .map(|binding| match self.timing.as_ref() {
                Some(session) => binding.observed(session),
                None => binding,
            })
    }
}

/// A [`Context`] that owns its theme; [`OwnedContext::context`] lends one.
#[derive(Clone)]
pub(crate) struct OwnedContext {
    instance: application::Instance,
    generation: program::GenerationId,
    authority: MappingAuthority,
    diagnostics: Arc<diagnostic::Sink>,
    theme: theme::Value,
    timing: Option<timing::Session>,
}

impl OwnedContext {
    pub(crate) fn context(&self) -> Context<'_> {
        Context {
            instance: self.instance,
            generation: self.generation,
            authority: self.authority.clone(),
            diagnostics: self.diagnostics.clone(),
            theme: &self.theme,
            timing: self.timing.clone(),
        }
    }
}

impl NativeMapper {
    /// Stamp one generated native event for the exact current application owner.
    #[must_use]
    pub fn map(&self, payload: Dynamic) -> runtime::Message {
        runtime::Message::new(self.instance, self.generation, payload)
    }
}

impl ProfileEventMapper {
    /// Map one native payload by invoking the rebound symbolic item under
    /// the current Authority/Context.
    #[must_use]
    pub fn map(&self, payload: Dynamic) -> runtime::Message {
        self.0.map(payload)
    }
}

impl MappingAuthority {
    pub(crate) fn map_message(
        &self,
        message: runtime::Message,
        mapper: &rawr::item::Value,
    ) -> runtime::Message {
        match self {
            MappingAuthority::Unavailable => message,
            MappingAuthority::Application {
                authority,
                boundary: mapping::Boundary::Final,
            } => message.map_application(authority, mapper),
            MappingAuthority::Application {
                authority,
                boundary: mapping::Boundary::Intermediate,
            } => message.map_application_intermediate(authority, mapper),
            MappingAuthority::Preview {
                catalog,
                key,
                boundary: mapping::Boundary::Final,
            } => message.map_preview(catalog, key, mapper),
            MappingAuthority::Preview {
                catalog,
                key,
                boundary: mapping::Boundary::Intermediate,
            } => message.map_preview_intermediate(catalog, key, mapper),
        }
    }

    fn intermediate(&self) -> Self {
        match self {
            Self::Unavailable => Self::Unavailable,
            Self::Application { authority, .. } => Self::Application {
                authority: authority.clone(),
                boundary: mapping::Boundary::Intermediate,
            },
            Self::Preview { catalog, key, .. } => Self::Preview {
                catalog: catalog.clone(),
                key: key.clone(),
                boundary: mapping::Boundary::Intermediate,
            },
        }
    }

    #[allow(dead_code)]
    fn promote_widget(
        &self,
        promotion: &rawr::widget::Promotion,
        data: Dynamic,
    ) -> Result<rawr::widget::Descriptor, rawr::widget::Error> {
        match self {
            Self::Unavailable => Err(rawr::widget::Error::Site),
            Self::Application { authority, .. } => authority.promote_widget(promotion, data),
            Self::Preview { catalog, key, .. } => catalog.promote_widget(key, promotion, data),
        }
    }

    #[allow(dead_code)]
    fn bind_widget(
        &self,
        descriptor: &rawr::widget::Descriptor,
    ) -> Result<rawr::widget::Binding, rawr::widget::Error> {
        match self {
            Self::Unavailable => Err(rawr::widget::Error::Site),
            Self::Application { authority, .. } => authority.bind_widget(descriptor),
            Self::Preview { catalog, key, .. } => catalog.bind_widget(key, descriptor),
        }
    }
}

/// The view shown when the app's view failed: what failed, and why, so a
/// headless render (?play, a snapshot) says it without the window's
/// diagnostic panel.
pub(crate) fn unavailable(reason: impl Into<String>) -> Value {
    Value::new(Unavailable {
        identity: Identity::unique(),
        reason: reason.into(),
    })
}

struct Unavailable {
    identity: Identity,
    reason: String,
}

impl Descriptor for Unavailable {
    fn lower(&self, _context: &Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::column![
            iced::widget::text("Rawr view unavailable"),
            iced::widget::text(self.reason.clone()).size(14),
        ]
        .spacing(8)
        .boxed()
    }

    fn identity(&self) -> Identity {
        self.identity.clone()
    }
}

pub(crate) fn diagnostic(message: impl Into<String>) -> Value {
    Value::new(Diagnostic {
        identity: Identity::unique(),
        message: message.into(),
    })
}

#[cfg(test)]
pub(crate) fn diagnostic_from_projection(
    message: impl Into<String>,
    call: &rawr::host::runtime::Call<'_>,
    source: &rawr::host::runtime::Witness,
) -> Result<Value, rawr::host::runtime::WitnessError> {
    Value::from_projection(
        Diagnostic {
            identity: Identity::unique(),
            message: message.into(),
        },
        call,
        source,
    )
}

struct Diagnostic {
    identity: Identity,
    message: String,
}

impl Descriptor for Diagnostic {
    fn lower(&self, _context: &Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::text(self.message.clone()).boxed()
    }

    fn identity(&self) -> Identity {
        self.identity.clone()
    }
}
