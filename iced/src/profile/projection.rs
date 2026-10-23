//! Generated direct Widget-to-Element projection.

use std::fmt::Write as _;

use rawr_import::profile;

#[cfg(test)]
mod emitted_tests;

pub(super) const HOOK: &str = "direct-widget-element";

pub(super) fn source<'a>(
    hook: profile::demand::Hook,
    demand: profile::trusted::Demand<'a>,
    #[cfg(feature = "import-baseline")] fixture: super::Fixture,
) -> String {
    let mut source = String::with_capacity(2_400);
    source.push_str("mod __rawr_profile {\n");
    #[cfg(feature = "import-baseline")]
    {
        source.push_str(
            "    const PROFILE_FIXTURE: __rawr_profile_host::profile::Fixture = \
             __rawr_profile_host::profile::Fixture::",
        );
        source.push_str(match fixture {
            super::Fixture::Baseline => "Baseline",
            super::Fixture::Variant => "Variant",
        });
        let _ = write!(
            source,
            ";\n    const PROFILE_VERSION: u32 = {};\n",
            fixture.version()
        );
    }
    source.push_str("    const PROFILE_RULES: [u8; 32] = ");
    bytes(&mut source, demand.rules_digest());
    source.push_str(";\n");
    source.push_str(
        "    const ALIAS_TARGETS: &[__rawr_profile_host::profile::runner::AliasTarget] = &[\n",
    );
    for alias in demand.alias_targets() {
        source.push_str("        __rawr_profile_host::profile::runner::AliasTarget::new(");
        let _ = write!(source, "{:?}, {:?}, ", alias.root(), alias.path());
        source.push_str(match alias.namespace() {
            rawr_import::surface::facade::Namespace::Type => {
                "__rawr_profile_host::profile::runner::Namespace::Type, "
            }
            rawr_import::surface::facade::Namespace::Value => {
                "__rawr_profile_host::profile::runner::Namespace::Value, "
            }
            rawr_import::surface::facade::Namespace::Macro => {
                "__rawr_profile_host::profile::runner::Namespace::Macro, "
            }
        });
        bytes(&mut source, alias.export_identity());
        source.push_str(", ");
        bytes(&mut source, alias.alias_identity());
        source.push_str(", ");
        bytes(&mut source, alias.target_identity());
        source.push_str(", &[");
        for (index, parameter) in alias.parameters().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            source.push_str("__rawr_profile_host::profile::runner::AliasParameter::new(");
            source.push_str(match parameter.kind() {
                rawr_import::surface::facade::ParameterKind::Lifetime => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Lifetime, "
                }
                rawr_import::surface::facade::ParameterKind::Type => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Type, "
                }
                rawr_import::surface::facade::ParameterKind::Const => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Const, "
                }
            });
            let _ = write!(
                source,
                "{}, {})",
                parameter.alias_index(),
                parameter.target_index()
            );
        }
        source.push_str("], ");
        bytes(&mut source, alias.site_selector());
        source.push_str(", ");
        bytes(&mut source, alias.selector());
        source.push_str("),\n");
    }
    source.push_str("    ];\n");
    source.push_str(
        "    const PROJECTION_SOURCES: &[__rawr_profile_host::profile::runner::ProjectionSourceSite] = &[\n",
    );
    for site in demand.projection_sources() {
        source.push_str("        __rawr_profile_host::profile::runner::ProjectionSourceSite::new(");
        let _ = write!(source, "{:?}, {:?}, ", site.root(), site.path());
        source.push_str("__rawr_profile_host::profile::runner::Namespace::Type, ");
        bytes(&mut source, site.export_identity());
        source.push_str(", ");
        bytes(&mut source, site.alias_identity());
        source.push_str(", ");
        bytes(&mut source, site.source_identity());
        source.push_str(", &[");
        for (index, parameter) in site.parameters().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            source.push_str("__rawr_profile_host::profile::runner::AliasParameter::new(");
            source.push_str(match parameter.kind() {
                rawr_import::surface::facade::ParameterKind::Lifetime => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Lifetime, "
                }
                rawr_import::surface::facade::ParameterKind::Type => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Type, "
                }
                rawr_import::surface::facade::ParameterKind::Const => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Const, "
                }
            });
            let _ = write!(
                source,
                "{}, {})",
                parameter.alias_index(),
                parameter.target_index()
            );
        }
        source.push_str("], &[");
        for (index, argument) in site.arguments().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            match argument {
                rawr_import::profile::contract::SourceArgument::Region {
                    alias,
                    source: target,
                } => {
                    let _ = write!(source, "__rawr_profile_host::profile::runner::SourceArgument::Region {{ alias: {alias}, source: {target} }}");
                }
                rawr_import::profile::contract::SourceArgument::Parameter {
                    kind,
                    alias,
                    source: target,
                } => {
                    source.push_str(
                        "__rawr_profile_host::profile::runner::SourceArgument::Parameter { kind: ",
                    );
                    source.push_str(match kind {
                        rawr_import::surface::facade::ParameterKind::Lifetime => {
                            "__rawr_profile_host::profile::runner::ParameterKind::Lifetime"
                        }
                        rawr_import::surface::facade::ParameterKind::Type => {
                            "__rawr_profile_host::profile::runner::ParameterKind::Type"
                        }
                        rawr_import::surface::facade::ParameterKind::Const => {
                            "__rawr_profile_host::profile::runner::ParameterKind::Const"
                        }
                    });
                    let _ = write!(source, ", alias: {alias}, source: {target} }}");
                }
            }
        }
        source.push_str("], ");
        bytes(&mut source, site.site_selector());
        source.push_str(", ");
        bytes(&mut source, site.widget_trait_evidence());
        source.push_str(", ");
        bytes(&mut source, site.widget_evidence());
        source.push_str(", ");
        bytes(&mut source, site.target_selector());
        source.push_str(", ");
        bytes(&mut source, site.selector());
        source.push_str("),\n");
    }
    source.push_str("    ];\n");
    source.push_str(
        "    const EXTENSION_ABIS: &[__rawr_profile_host::profile::runner::ExtensionAbi] = &[\n",
    );
    for abi in demand.extension_abis() {
        source.push_str("        __rawr_profile_host::profile::runner::ExtensionAbi::new(");
        source.push_str("__rawr_profile_host::profile::runner::ExtensionAbiRole::");
        source.push_str(abi.role());
        let _ = write!(source, ", {:?}, {:?}, ", abi.root(), abi.path());
        source.push_str("__rawr_profile_host::profile::runner::Namespace::Type, ");
        bytes(&mut source, abi.export_identity());
        source.push_str(", ");
        bytes(&mut source, abi.definition_identity());
        source.push_str(", ");
        bytes(&mut source, abi.shape_digest());
        source.push_str(", ");
        bytes(&mut source, abi.site_selector());
        source.push_str(", ");
        bytes(&mut source, abi.linked_fingerprint());
        source.push_str(", &[");
        for (index, argument) in abi.arguments().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            match argument {
                rawr_import::profile::contract::AbiArgument::DefaultF32 { parameter } => {
                    let _ = write!(source, "__rawr_profile_host::profile::runner::ExtensionAbiArgument::DefaultF32 {{ parameter: {parameter} }}");
                }
            }
        }
        source.push_str("], ");
        bytes(&mut source, abi.selector());
        source.push_str("),\n");
    }
    source.push_str("    ];\n");
    source.push_str(
        "    const OPERATION_ENDPOINTS: &[__rawr_profile_host::profile::runner::OperationEndpoint] = &[\n",
    );
    for endpoint in demand.operation_endpoints() {
        let site = endpoint.site();
        source.push_str("        __rawr_profile_host::profile::runner::OperationEndpoint::new(");
        let _ = write!(source, "{:?}, {:?}, ", site.root(), site.path());
        source.push_str(match site.namespace() {
            rawr_import::surface::facade::Namespace::Type => {
                "__rawr_profile_host::profile::runner::Namespace::Type, "
            }
            rawr_import::surface::facade::Namespace::Value => {
                "__rawr_profile_host::profile::runner::Namespace::Value, "
            }
            rawr_import::surface::facade::Namespace::Macro => {
                "__rawr_profile_host::profile::runner::Namespace::Macro, "
            }
        });
        bytes(&mut source, site.export_identity());
        source.push_str(", ");
        bytes(&mut source, site.definition_identity());
        source.push_str(", &[");
        for (index, formal) in site.formals().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            source.push_str("__rawr_profile_host::profile::runner::OperationFormal::new(");
            bytes(&mut source, formal.owner());
            let _ = write!(source, ", {}, ", formal.index());
            source.push_str(match formal.kind() {
                rawr_import::surface::facade::ParameterKind::Lifetime => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Lifetime"
                }
                rawr_import::surface::facade::ParameterKind::Type => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Type"
                }
                rawr_import::surface::facade::ParameterKind::Const => {
                    "__rawr_profile_host::profile::runner::ParameterKind::Const"
                }
            });
            source.push(')');
        }
        source.push_str("], ");
        bytes(&mut source, site.shape_digest());
        source.push_str(", ");
        bytes(&mut source, site.selector());
        source.push_str(", ");
        bytes(&mut source, endpoint.source_selector());
        source.push_str(", ");
        bytes(&mut source, endpoint.projection_selector());
        source.push_str(", &[");
        for (index, argument) in endpoint.arguments().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            match argument {
                rawr_import::profile::contract::EndpointArgument::Region { parameter } => {
                    let _ = write!(source, "__rawr_profile_host::profile::runner::EndpointArgument::Region {{ parameter: {parameter} }}");
                }
                rawr_import::profile::contract::EndpointArgument::Definition {
                    parameter,
                    definition,
                } => {
                    source.push_str("__rawr_profile_host::profile::runner::EndpointArgument::Definition { parameter: ");
                    let _ = write!(source, "{parameter}, definition: ");
                    bytes(&mut source, definition.digest());
                    source.push_str(" }");
                }
                rawr_import::profile::contract::EndpointArgument::OwnedString {
                    parameter,
                    lifetime,
                } => {
                    let _ = write!(source, "__rawr_profile_host::profile::runner::EndpointArgument::OwnedString {{ parameter: {parameter}, lifetime: {lifetime} }}");
                }
            }
        }
        source.push_str("], ");
        bytes(&mut source, endpoint.linked());
        source.push_str(", ");
        bytes(&mut source, endpoint.selector());
        source.push_str("),\n");
    }
    source.push_str("    ];\n");
    source.push_str(
        "    const PROJECTIONS: &[__rawr_profile_host::profile::runner::Projection] = &[\n",
    );
    for projection in demand.projections() {
        source.push_str("        __rawr_profile_host::profile::runner::Projection::new(");
        bytes(&mut source, projection.source_selector());
        source.push_str(", ");
        bytes(&mut source, projection.alias_selector());
        source.push_str(", ");
        bytes(&mut source, projection.selector());
        source.push_str(", ");
        source.push_str("&[");
        for (index, argument) in projection.arguments().iter().enumerate() {
            if index > 0 {
                source.push_str(", ");
            }
            match argument.source_index() {
                Some(index) => {
                    let _ = write!(
                        source,
                        "__rawr_profile_host::profile::runner::Argument::source({index})"
                    );
                }
                None => source.push_str("__rawr_profile_host::profile::runner::Argument::fresh()"),
            }
        }
        source.push_str("]),\n");
    }
    source.push_str("    ];\n");
    source.push_str(
        r#"
    pub fn payload<T>(value: T) -> T { value }

    pub struct Replay<T>(std::sync::Arc<dyn for<'a> Fn(&__rawr_profile_host::widget::Context<'a>) -> T + Send + Sync>);

    impl<T> Clone for Replay<T> {
        fn clone(&self) -> Self { Self(std::sync::Arc::clone(&self.0)) }
    }

    impl<T> Replay<T> {
        pub fn new(
            replay: impl for<'a> Fn(&__rawr_profile_host::widget::Context<'a>) -> T + Send + Sync + 'static,
        ) -> Self {
            Self(std::sync::Arc::new(replay))
        }

        fn run(&self, context: &__rawr_profile_host::widget::Context<'_>) -> T {
            (self.0)(context)
        }
    }

    struct Projectable<T> {
        replay: Replay<T>,
        mapper: [u8; 16],
    }

    struct Projected<T, Message> {
        replay: Replay<T>,
        mapper: [u8; 16],
        identity: __rawr_profile_host::widget::Identity,
        marker: core::marker::PhantomData<fn(Message)>,
    }

    impl<T, Message> __rawr_profile_host::widget::Descriptor for Projected<T, Message>
    where
        T: iced::advanced::Widget<Message, iced::Theme, iced::Renderer> + Send + Sync + 'static,
        Message: Into<__rawr_internal::Dynamic> + 'static,
    {
        fn lower(
            &self,
            context: &__rawr_profile_host::widget::Context<'_>,
        ) -> iced::Element<'static, __rawr_profile_host::runtime::Message> {
            let Some(mapper) = context.profile_event_mapper(&self.mapper) else {
                return iced::advanced::Widget::_boxed(iced::widget::text("stale generated event mapper"));
            };
            let native = iced::advanced::Widget::_map(self.replay.run(context), move |message: Message| {
                mapper.map(message.into())
            });
            iced::advanced::Widget::_boxed(native)
        }

        fn identity(&self) -> __rawr_profile_host::widget::Identity {
            self.identity.clone()
        }
    }

    fn projectable<T, Message>(
        value: Box<dyn std::any::Any>,
        call: &__rawr_internal::host::runtime::Call<'_>,
        source: &__rawr_internal::host::runtime::Witness,
    ) -> Result<__rawr_internal::Dynamic, Box<__rawr_internal::EvalAltResult>>
    where
        T: iced::advanced::Widget<Message, iced::Theme, iced::Renderer> + Send + Sync + 'static,
        Message: Into<__rawr_internal::Dynamic> + 'static,
    {
        let projectable = value
            .downcast::<Projectable<T>>()
            .map_err(|_| super::runtime_error(super::Error::TypeMismatch))?;
        if call.event_mapper() != Some(&projectable.mapper) {
            return Err(super::runtime_error(super::Error::TypeMismatch));
        }
        let descriptor = Projected::<T, Message> {
            replay: projectable.replay,
            mapper: projectable.mapper,
            identity: __rawr_profile_host::widget::Identity::unique(),
            marker: core::marker::PhantomData,
        };
        let value = __rawr_profile_host::widget::Value::from_projection(descriptor, call, source)
            .map_err(super::runtime_error)?;
        Ok(__rawr_internal::Dynamic::from(value))
    }

    pub fn map_projectable<T, Message>(
        arguments: &mut super::Arguments<'_, '_, '_>,
        mapper: &[u8; 16],
        transform: impl for<'a> Fn(T, &__rawr_profile_host::widget::Context<'a>) -> T + Send + Sync + 'static,
    ) -> Result<__rawr_internal::Dynamic, super::Error>
    where
        T: iced::advanced::Widget<Message, iced::Theme, iced::Renderer> + Send + Sync + 'static,
        Message: Into<__rawr_internal::Dynamic> + 'static,
    {
        let receiver: Projectable<T> = super::take_native(arguments, 0)?;
        if &receiver.mapper != mapper {
            return Err(super::Error::TypeMismatch);
        }
        let replay = Replay::new(move |context| transform(receiver.replay.run(context), context));
        insert_projectable::<T, Message>(arguments.arena, replay, mapper)
            .map(__rawr_internal::Dynamic::from)
    }

    pub fn insert_projectable<T, Message>(
        arena: &mut super::Arena,
        replay: Replay<T>,
        mapper: &[u8; 16],
    ) -> Result<super::NativeHandle, super::Error>
    where
        T: iced::advanced::Widget<Message, iced::Theme, iced::Renderer> + Send + Sync + 'static,
        Message: Into<__rawr_internal::Dynamic> + 'static,
    {
        arena.insert_projectable(
            Projectable { replay, mapper: *mapper },
            projectable::<T, Message>,
            *mapper,
        )
    }
"#,
    );
    source.push_str(
        "    #[derive(Clone)]\n\
             \x20   pub struct ApplicationContract(__rawr_internal::host::facade::ProfileCycle);\n\
             \x20    pub fn compile_initial(\n\
             \x20        engine: &__rawr_internal::Engine,\n\
             \x20        snapshot: &__rawr_internal::package::snapshot::Snapshot,\n\
             \x20        contract: &ApplicationContract,\n\
             \x20        expected: &[u8; 32],\n\
             \x20    ) -> std::io::Result<(Option<__rawr_internal::reload::Candidate>, __rawr_internal::AST)> {\n\
             \x20        engine.compile_package_snapshot_generation_with_top_level_checked(\n\
             \x20            snapshot, &contract.0, expected,\n\
             \x20        )\n\
             \x20            .map(|(candidate, ast)| (Some(candidate), ast))\n\
             \x20            .map_err(|error| std::io::Error::other(error.to_string()))\n\
             \x20    }\n\
             \x20   pub fn compile_generation(\n\
             \x20       engine: &__rawr_internal::Engine,\n\
             \x20       snapshot: &__rawr_internal::package::snapshot::Snapshot,\n\
             \x20       contract: &ApplicationContract,\n\
             \x20       expected: &[u8; 32],\n\
             \x20   ) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> {\n\
             \x20       engine.compile_package_snapshot_generation_checked(\n\
             \x20           snapshot,\n\
             \x20           &contract.0,\n\
             \x20           expected,\n\
             \x20       )\n\
             \x20           .map(Some)\n\
             \x20           .map_err(|error| std::io::Error::other(error.to_string()))\n\
             \x20   }\n\
             \x20   pub fn compile_resident(\n\
             \x20       engine: &__rawr_internal::Engine,\n\
             \x20       snapshot: &__rawr_internal::package::snapshot::Snapshot,\n\
             \x20       contract: &ApplicationContract,\n\
             \x20   ) -> Result<__rawr_internal::package::import::resident::Outcome, __rawr_internal::package::import::resident::Error> {\n\
             \x20       engine.compile_package_snapshot_resident(snapshot, &contract.0)\n\
             \x20   }\n\
             \x20   pub fn bind_runtime(\n\
             \x20       definitions: &[__rawr_internal::host::facade::Definition],\n\
             \x20       builders: &mut [__rawr_internal::host::runtime::Builder],\n\
             \x20   ) -> Result<(), String> {\n\
             \x20       __rawr_profile_host::profile::runner::bind_runtime(definitions, builders, PROJECTIONS)\n\
             \x20           .map_err(|error| error.to_string())\n\
             \x20   }\n\
             \x20   pub fn attest_support_endpoint(\n\
             \x20       value: &__rawr_internal::Dynamic,\n\
             \x20       expected: &__rawr_internal::host::runtime::Instantiation<'_>,\n\
             \x20   ) -> Option<__rawr_internal::host::runtime::Attested> {\n\
             \x20       __rawr_profile_host::profile::runner::attest_support_element(value, expected)\n\
             \x20   }\n\
             \x20   pub fn install_compiler_facades(\n\
             \x20       engine: &mut __rawr_internal::Engine,\n\
             \x20       native: &mut __rawr_internal::package::import::runner::account::Native,\n\
             \x20   ) -> std::io::Result<(Box<[__rawr_internal::host::facade::Definition]>, ApplicationContract)> {\n",
    );
    #[cfg(feature = "import-baseline")]
    source.push_str(
        "        __rawr_profile_host::profile::runner::install_compiler_fixture_retained(\n\
         \x20           engine,\n\
         \x20           PROFILE_FIXTURE,\n\
         \x20           PROFILE_VERSION,\n",
    );
    #[cfg(not(feature = "import-baseline"))]
    source.push_str(
        "        __rawr_profile_host::profile::runner::install_compiler_retained(\n\
         \x20           engine,\n",
    );
    source.push_str(
        "            super::IMPORTED_FACADES,\n\
             \x20           super::PROFILE_FINGERPRINT,\n\
             \x20           PROFILE_RULES,\n\
             \x20           ALIAS_TARGETS,\n\
             \x20           PROJECTION_SOURCES,\n\
             \x20           EXTENSION_ABIS,\n\
             \x20           OPERATION_ENDPOINTS,\n\
             \x20           PROJECTIONS,\n\
             \x20           native,\n\
             \x20       )\n\
             \x20       .map(|compiler| {\n\
             \x20           let contract = ApplicationContract(compiler.application_cycle().clone());\n\
             \x20           (compiler.into_definitions(), contract)\n\
             \x20       })\n\
             \x20       .map_err(|error| std::io::Error::other(error.to_string()))\n\
             \x20   }\n\
             \x20   enum Stage { New, Prepared, Activated, Admitted, Retired }\n\
             \x20   pub type Mode = __rawr_profile_host::profile::runner::Mode;\n\
             \x20   pub type Transport = __rawr_profile_host::profile::runner::Transport;\n\
             \x20   pub type PendingInstall = __rawr_profile_host::profile::runner::PendingInstall;\n\
             \x20   pub struct Runtime {\n\
             \x20       stage: Stage,\n\
             \x20       mode: __rawr_profile_host::profile::runner::Mode,\n\
             \x20       root: std::path::PathBuf,\n\
             \x20       host: Option<__rawr_profile_host::profile::runner::BoundGeneration>,\n\
             \x20   }\n\
             \x20   impl Runtime {\n\
             \x20       pub fn new(\n\
             \x20           mode: __rawr_profile_host::profile::runner::Mode,\n\
             \x20           root: std::path::PathBuf,\n\
             \x20       ) -> Self { Self { stage: Stage::New, mode, root, host: None } }\n\
             \x20       pub fn prepare(&mut self) -> Result<(), &'static str> {\n\
             \x20           match self.stage {\n\
             \x20               Stage::New => { self.stage = Stage::Prepared; Ok(()) }\n\
             \x20               Stage::Prepared | Stage::Activated | Stage::Admitted | Stage::Retired => Err(\"iced prepare order\"),\n\
             \x20           }\n\
             \x20       }\n\
             \x20       pub fn activate(&mut self) -> Result<(), &'static str> {\n\
             \x20           match self.stage {\n\
             \x20               Stage::Prepared => { self.stage = Stage::Activated; Ok(()) }\n\
             \x20               Stage::New | Stage::Activated | Stage::Admitted | Stage::Retired => Err(\"iced activate order\"),\n\
             \x20           }\n\
             \x20       }\n\
             \x20       pub fn admit(&mut self, program: Option<&mut super::Program>) -> Result<(), String> {\n\
             \x20           match self.stage {\n\
             \x20               Stage::Activated => {\n\
             \x20                   let program = program.ok_or_else(|| \"iced generation program unavailable\".to_owned())?;\n\
             \x20                   let candidate = program.candidate.take().ok_or_else(|| \"iced application generation unavailable\".to_owned())?;\n\
             \x20                   let runtime = super::imported_runtime(&program.engine, &program._facades)\n\
             \x20                       .map_err(|error| error.to_string())?;\n\
             \x20                   self.host = Some(__rawr_profile_host::profile::runner::bind_generation(\n\
             \x20                       candidate,\n\
             \x20                       &program.engine,\n\
             \x20                       runtime,\n\
             \x20                       super::PROFILE_FINGERPRINT,\n\
             \x20                       self.mode,\n\
             \x20                       self.root.clone(),\n\
             \x20                   ).map_err(|error| error.to_string())?);\n\
             \x20                   self.stage = Stage::Admitted;\n\
             \x20                   Ok(())\n\
             \x20               }\n\
             \x20               Stage::New | Stage::Prepared | Stage::Admitted | Stage::Retired => Err(\"iced admit order\".to_owned()),\n\
             \x20           }\n\
             \x20       }\n\
             \x20       pub fn message(&mut self, payload: &[u8]) -> Result<Vec<u8>, &'static str> {\n\
             \x20           match self.stage {\n\
             \x20               Stage::Admitted => Ok(payload.to_vec()),\n\
             \x20               Stage::New | Stage::Prepared | Stage::Activated | Stage::Retired => Err(\"iced route is not admitted\"),\n\
             \x20           }\n\
             \x20       }\n\
             \x20       pub fn seed_state(&mut self, state: &[u8]) -> bool {\n\
             \x20           matches!(self.stage, Stage::Admitted)\n\
             \x20               && self.host.as_mut().is_some_and(|host| host.seed_state(state))\n\
             \x20       }\n\
             \x20       pub fn observe(&mut self, bytes: &[u8]) -> Result<(), String> {\n\
             \x20           if !matches!(self.stage, Stage::Admitted) { return Err(\"iced observation is not admitted\".to_owned()); }\n\
             \x20           self.host.as_mut().ok_or_else(|| \"iced generation host unavailable\".to_owned())?\n\
             \x20               .observe(bytes).map_err(|error| error.to_string())\n\
             \x20       }\n\
             \x20       pub fn start(\n\
             \x20           &mut self,\n\
             \x20           program: Option<&mut super::Program>,\n\
             \x20       ) -> Result<(\n\
             \x20           __rawr_profile_host::profile::runner::Running,\n\
             \x20           __rawr_profile_host::profile::runner::Transport,\n\
             \x20       ), &'static str> {\n\
             \x20           match self.stage {\n\
             \x20               Stage::Admitted => {\n\
             \x20                   let program = program.ok_or(\"iced generation program unavailable\")?;\n\
             \x20                   let host = self.host.take().ok_or(\"iced generation host unavailable\")?;\n\
             \x20                   host.start(program.engine.clone())\n\
             \x20                       .map_err(|_| \"iced generation lifecycle failed\")\n\
             \x20               }\n\
             \x20               Stage::New | Stage::Prepared | Stage::Activated | Stage::Retired => Err(\"iced start is not admitted\"),\n\
             \x20           }\n\
             \x20       }\n\
             \x20       pub fn snapshot(&mut self) -> Result<Box<[u8]>, String> {\n\
             \x20           match self.stage {\n\
             \x20               Stage::Admitted => {\n\
             \x20                   let host = self.host.take().ok_or(\"iced generation host unavailable\")?;\n\
             \x20                   let outcome = host.snapshot().map_err(|error| error.to_string())?;\n\
             \x20                   self.stage = Stage::Retired;\n\
             \x20                   __rawr_profile_host::snapshot::wire::encode(&outcome)\n\
             \x20                       .map_err(|error| error.to_string())\n\
             \x20               }\n\
             \x20               Stage::New | Stage::Prepared | Stage::Activated | Stage::Retired => Err(\"iced snapshot is not admitted\".to_owned()),\n\
             \x20           }\n\
             \x20       }\n\
             \x20       pub fn retire(&mut self) { self.host.take(); self.stage = Stage::Retired; }\n\
             \x20   }\n\
             }\n",
    );
    let identity = hook.identity();
    let _ = writeln!(source, "const _: () = {{");
    let _ = write!(source, "    const EVIDENCE: [u8; 32] = [");
    for (index, byte) in identity.iter().enumerate() {
        if index > 0 {
            source.push_str(", ");
        }
        let _ = write!(source, "{byte}");
    }
    source.push_str("];\n");
    source.push_str(
        "    #[allow(dead_code)]\n\
         \x20   fn project<'view, Message, Theme, Renderer, W>(widget: W)\n\
         \x20   -> iced::Element<'view, Message, Theme, Renderer>\n\
         \x20   where\n\
         \x20       Message: 'view,\n\
         \x20       Theme: 'view,\n\
         \x20       Renderer: iced::advanced::Renderer + 'view,\n\
         \x20       W: iced::advanced::Widget<Message, Theme, Renderer> + 'view,\n\
         \x20   {\n\
         \x20       iced::advanced::Widget::_boxed(widget)\n\
         \x20   }\n\
         \x20   let _ = EVIDENCE;\n\
         };",
    );
    source
}

fn bytes(output: &mut String, value: &[u8; 32]) {
    output.push('[');
    for (index, byte) in value.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        let _ = write!(output, "{byte}");
    }
    output.push(']');
}

#[cfg(test)]
mod tests {
    use crate::profile::{Checked, Definition};

    #[test]
    fn generated_compiler_uses_the_retained_native_account() {
        let limits = rawr_import::cargo::Limits::standard();
        let graph = rawr_import::test_fixture::iced_profile();
        let definition = Definition::new(&limits).expect("profile constructs");
        let contract = rawr_import::profile::classify(&definition, &graph, &limits)
            .expect("profile classifies");
        let checked = Checked::from_fingerprint([0x55; 32]);
        let demand = rawr_import::profile::demand(&definition, &contract, &checked, &limits)
            .expect("profile demand seals");
        let generated = rawr_import::profile::Proof::generate(&definition, &demand, &limits)
            .expect("profile generates");
        let source = std::str::from_utf8(generated.generated_source())
            .expect("generated profile source is UTF-8");
        let install = source
            .split_once("pub fn install_compiler_facades(")
            .expect("compiler facade installer")
            .1
            .split_once("enum Stage")
            .expect("profile runtime")
            .0;
        assert!(install
            .contains("native: &mut __rawr_internal::package::import::runner::account::Native,"));
        #[cfg(feature = "import-baseline")]
        assert!(install.contains(
            "runner::install_compiler_fixture_retained(\n            engine,\n            PROFILE_FIXTURE,\n            PROFILE_VERSION,"
        ));
        #[cfg(not(feature = "import-baseline"))]
        assert!(install.contains("runner::install_compiler_retained(\n            engine,"));
        assert!(install.contains("PROJECTIONS,\n            native,\n        )"));
        assert!(!install.contains("runner::install_compiler("));
        assert!(!install.contains("runner::install_compiler_fixture("));
        assert!(source.contains("compile_package_snapshot_generation_with_top_level_checked("));
        assert!(source.contains("compile_package_snapshot_generation_checked("));
        let resident = source
            .split_once("pub fn compile_resident(")
            .expect("resident compiler")
            .1
            .split_once("pub fn bind_runtime(")
            .expect("runtime binder")
            .0;
        assert!(resident.contains("package::import::resident::Outcome"));
        assert!(resident.contains("package::import::resident::Error"));
        assert_eq!(
            resident
                .matches("compile_package_snapshot_resident(")
                .count(),
            1
        );
        assert!(resident.contains("snapshot, &contract.0"));
        assert!(!resident.contains("expected"));
    }
}
