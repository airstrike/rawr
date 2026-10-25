//! Symbolic scripted widgets and their explicit launch binding.

use iced::widget::Widget as _;
use rawr::{host, program, widget, Dynamic};

use crate::runtime;

/// One durable scripted widget awaiting the current lowering authority.
#[derive(Clone, Debug)]
pub(crate) struct Descriptor {
    symbolic: widget::Descriptor,
    identity: super::Identity,
}

/// One scripted widget bound to the exact authority that may execute it.
pub(crate) struct Bound<'a> {
    pub(super) binding: widget::Binding,
    pub(super) instance: rawr::application::Instance,
    pub(super) context: super::Context<'a>,
    pub(super) children: Vec<iced::Element<'a, runtime::Message>>,
    pub(super) children_ready: bool,
}

impl std::fmt::Debug for Bound<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Bound")
            .field("binding", &self.binding)
            .field("instance", &self.instance)
            .field("children", &self.children.len())
            .field("children_ready", &self.children_ready)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("the scripted draw hook failed: {0}")]
    Hook(#[from] program::Error),
    #[error("the scripted renderer transaction failed: {0}")]
    Renderer(#[from] crate::renderer::Error),
}

pub(crate) fn bind(builder: &mut host::runtime::Builder) {
    builder
        .bind_widget::<super::Value>(|descriptor, witness| {
            Ok(Dynamic::from(super::Value::from_activation(
                Descriptor::retain(descriptor),
                witness,
            )))
        })
        .expect("the iced facade must retain exactly one open widget carrier");
}

impl Descriptor {
    #[cfg(test)]
    pub(crate) fn promote(
        context: &super::Context<'_>,
        promotion: &widget::Promotion,
        data: Dynamic,
    ) -> Result<Self, widget::Error> {
        context.promote_script(promotion, data).map(Self::retain)
    }

    pub(crate) fn retain(symbolic: widget::Descriptor) -> Self {
        let identity = super::Identity::script(symbolic.site().clone());
        Self { symbolic, identity }
    }

    pub(crate) fn bind<'a>(
        &self,
        context: &super::Context<'a>,
    ) -> Result<Bound<'a>, widget::Error> {
        context.bind_script(&self.symbolic).map(|binding| Bound {
            binding,
            instance: context.instance(),
            context: context.clone(),
            children: Vec::new(),
            children_ready: false,
        })
    }

    #[cfg(test)]
    pub(crate) const fn site(&self) -> &widget::Site {
        self.symbolic.site()
    }
}

impl super::Descriptor for Descriptor {
    fn lower<'a>(&self, context: &super::Context<'a>) -> iced::Element<'a, runtime::Message> {
        match self.bind(context) {
            Ok(bound) => {
                context.diagnostics().clear(&self.binding_key());
                bound.boxed()
            }
            Err(error) => {
                context.diagnostics().record(
                    self.binding_key(),
                    rawr::diagnostic::Report::error(
                        rawr::diagnostic::code::RUNTIME_EVALUATION,
                        format!("scripted widget binding failed: {error}"),
                    )
                    .finish(),
                );
                iced::widget::text("scripted widget unavailable").boxed()
            }
        }
    }

    fn identity(&self) -> super::Identity {
        self.identity.clone()
    }
}

impl Descriptor {
    fn binding_key(&self) -> super::diagnostic::Key {
        super::diagnostic::Key::binding(
            self.symbolic.site().clone(),
            self.symbolic.promotion().location().span(),
        )
    }
}

impl Bound<'_> {
    #[cfg(test)]
    pub(crate) fn generation(&self) -> program::GenerationId {
        self.binding.generation()
    }

    pub(crate) const fn site(&self) -> &widget::Site {
        self.binding.promotion().site()
    }

    pub(crate) fn draw(
        &self,
        state: Option<Dynamic>,
        input: crate::widget::draw::Input,
    ) -> Result<Option<crate::renderer::Replay>, Error> {
        let transaction = crate::renderer::Transaction::bind(
            self.instance,
            self.binding.generation(),
            self.site().clone(),
            self.binding.promotion().definition().fingerprint().clone(),
        )?;
        let declared = self.binding.draw(
            state,
            Dynamic::from(transaction.lease()),
            Dynamic::from(input),
            transaction.invocation(),
        )?;
        match declared {
            Some(()) => transaction.commit().map(Some).map_err(Error::from),
            None => {
                transaction.abort();
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rawr::{application, diagnostic, host, preview, widget, Dynamic, Engine, Shared};

    use super::{Descriptor, Error};

    #[test]
    fn retained_descriptor_rebinds_current_code_while_bound_keeps_issuer() {
        let engine = engine();
        let first = launch(&engine, application("first"));
        let first_state = first.boot_default().unwrap();
        let theme = crate::theme::Value::default();
        let first_instance = application::Instance::new();
        let first_diagnostics = sink(first_instance, first.generation());
        let first_context =
            super::super::Context::application(&first, first_instance, &first_diagnostics, &theme);
        let symbolic =
            Descriptor::promote(&first_context, &first.widget_promotions()[0], first_state)
                .unwrap();
        let retained_site = symbolic.site().clone();
        let old = symbolic.bind(&first_context).unwrap();

        let second = launch(&engine, application("second"));
        let second_instance = application::Instance::new();
        let second_diagnostics = sink(second_instance, second.generation());
        let second_context = super::super::Context::application(
            &second,
            second_instance,
            &second_diagnostics,
            &theme,
        );
        let current = symbolic.bind(&second_context).unwrap();

        assert_ne!(old.generation(), current.generation());
        assert_eq!(old.generation(), first.generation());
        assert_eq!(current.generation(), second.generation());
        assert_eq!(old.site(), &retained_site);
        assert_eq!(current.site(), &retained_site);
    }

    #[test]
    fn promotion_rejects_data_outside_the_exact_nominal_schema() {
        let engine = engine();
        let launch = launch(&engine, application("size"));
        let theme = crate::theme::Value::default();
        let instance = application::Instance::new();
        let diagnostics = sink(instance, launch.generation());
        let context = super::super::Context::application(&launch, instance, &diagnostics, &theme);
        let error = Descriptor::promote(
            &context,
            &launch.widget_promotions()[0],
            Dynamic::from(42_i64),
        )
        .unwrap_err();

        assert!(matches!(error, widget::Error::Data(_)));
    }

    #[test]
    fn selected_boundary_activates_the_exact_scripted_descriptor() {
        let engine = engine();
        let launch = launch(&engine, application("size"));
        let mut state = launch.boot_default().unwrap();
        let value = launch
            .invoke_view(std::slice::from_mut(&mut state))
            .unwrap();

        assert!(super::super::Value::from_dynamic(value).is_ok());
    }

    #[test]
    fn preview_context_binds_the_same_symbolic_contract() {
        let engine = engine();
        let source = application("preview").replacen(
            "                    fn view(&self)",
            "                    #[preview]\n                    fn view(&self)",
            1,
        );
        let catalog = catalog(&engine, source);
        let key = catalog.previews()[0].key().clone();
        let data = catalog.seed(&key).unwrap().unwrap();
        let theme = crate::theme::Value::default();
        let instance = application::Instance::new();
        let diagnostics = sink(instance, catalog.generation());
        let catalog = crate::preview::catalog::Authority::static_(catalog);
        let context =
            super::super::Context::preview(&catalog, &key, instance, &diagnostics, &theme);
        let symbolic =
            Descriptor::promote(&context, &catalog.widget_promotions()[0], data).unwrap();
        let bound = symbolic.bind(&context).unwrap();

        assert_eq!(bound.generation(), catalog.generation());
        assert_eq!(bound.site(), symbolic.site());
    }

    #[test]
    fn retained_descriptor_rejects_current_nominal_schema_drift() {
        let engine = engine();
        let first = launch(&engine, application("first"));
        let data = first.boot_default().unwrap();
        let theme = crate::theme::Value::default();
        let first_instance = application::Instance::new();
        let first_diagnostics = sink(first_instance, first.generation());
        let first_context =
            super::super::Context::application(&first, first_instance, &first_diagnostics, &theme);
        let symbolic =
            Descriptor::promote(&first_context, &first.widget_promotions()[0], data).unwrap();

        let drifted = launch(
            &engine,
            application("drifted").replace("radius: i64", "radius: bool"),
        );
        let drifted_instance = application::Instance::new();
        let drifted_diagnostics = sink(drifted_instance, drifted.generation());
        let drifted_context = super::super::Context::application(
            &drifted,
            drifted_instance,
            &drifted_diagnostics,
            &theme,
        );
        let error = symbolic.bind(&drifted_context).unwrap_err();

        assert!(matches!(error, widget::Error::Site));
    }

    #[test]
    fn draw_failure_after_fill_cannot_return_replay() {
        let engine = engine();
        let launch = launch(&engine, draw_then_throw());
        let data = launch.boot_default().unwrap();
        let descriptor = launch
            .promote_widget(&launch.widget_promotions()[0], data)
            .unwrap();
        let instance = application::Instance::new();
        let diagnostics = sink(instance, launch.generation());
        let theme = crate::theme::Value::default();
        let context = super::super::Context::application(&launch, instance, &diagnostics, &theme);
        let bound = Descriptor::retain(descriptor).bind(&context).unwrap();

        let result = bound.draw(
            None,
            crate::widget::draw::Input::new(
                iced::Rectangle::default(),
                None,
                iced::Rectangle::default(),
                iced::Color::BLACK,
            ),
        );

        assert!(matches!(result, Err(Error::Hook(_))));
    }

    fn engine() -> Shared<Engine> {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        Shared::new(engine)
    }

    fn sink(
        instance: application::Instance,
        generation: rawr::program::GenerationId,
    ) -> Arc<crate::widget::diagnostic::Sink> {
        Arc::new(crate::widget::diagnostic::Sink::new(instance, generation))
    }

    fn launch(engine: &Shared<Engine>, source: String) -> Arc<application::BoundLaunch> {
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        Arc::new(launch.bind(registry).unwrap())
    }

    fn catalog(engine: &Shared<Engine>, source: String) -> Arc<preview::BoundCatalog> {
        let source = diagnostic::Source::new("main.rs", source).unwrap();
        let catalog = engine.compile_preview_catalog_source(source).unwrap();
        let launch = engine.launch_previews(catalog).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        Arc::new(launch.bind(registry).unwrap())
    }

    fn application(hook: &str) -> String {
        format!(
            r#"
                use iced::Element;

                #[derive(Default)]
                struct Dial {{ radius: i64 }}

                #[derive(Debug, Clone, Copy)]
                enum Message {{ Tick }}

                #[widget]
                impl Dial {{
                    fn size(&self) -> rawr_iced::widget::layout::Size {{ throw "{hook}" }}
                }}

                impl Dial {{
                    fn update(&mut self, message: Message) {{
                        match message {{ Message::Tick => () }}
                    }}

                    fn view(&self) -> Element<Message> {{
                        Dial {{ radius: self.radius }}
                    }}
                }}

                pub fn main() -> iced::Result {{
                    iced::run(Dial::update, Dial::view)
                }}
            "#
        )
    }

    fn draw_then_throw() -> String {
        r#"
            use iced::{Color, Result};
            use iced::Element;

            #[derive(Default)]
            struct Dial { radius: i64 }
            #[derive(Debug, Clone, Copy)]
            enum Message { Tick }

            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                    renderer.fill_quad(input.bounds(), Color::BLACK);
                    throw "after fill";
                }
            }

            impl Dial {
                fn update(&mut self, message: Message) {
                    match message { Message::Tick => () }
                }

                fn view(&self) -> Element<Message> { Dial { radius: self.radius } }
            }

            pub fn main() -> Result { iced::run(Dial::update, Dial::view) }
        "#
        .into()
    }
}
