//! Module that defines the public compilation API of [`Engine`].

use crate::parser::{ParseResult, ParseState};
use crate::{program, Engine, Scope, Shared, AST};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

impl Engine {
    /// Compile one immutable runner snapshot without consulting live files.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn compile_package_snapshot(
        &self,
        snapshot: &crate::package::snapshot::Snapshot,
    ) -> Result<AST, crate::package::Error> {
        crate::package::compile::snapshot(self, snapshot)
    }

    /// Compile one immutable runner snapshot into a complete reload generation.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn compile_package_snapshot_generation(
        &self,
        snapshot: &crate::package::snapshot::Snapshot,
    ) -> Result<crate::reload::Candidate, crate::package::Error> {
        crate::package::compile::snapshot_generation(self, snapshot)
    }

    /// Compile a reload generation and require exact staging/final application
    /// semantics before the candidate can be published.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn compile_package_snapshot_generation_checked(
        &self,
        snapshot: &crate::package::snapshot::Snapshot,
        cycle: &crate::host::facade::ProfileCycle,
        expected: &[u8; 32],
    ) -> Result<crate::reload::Candidate, crate::package::Error> {
        crate::package::compile::snapshot_generation_checked(self, snapshot, cycle, expected)
    }

    /// Compile and validate once, retaining the same program for top-level
    /// evaluation after native admission. Neither result escapes on failure.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn compile_package_snapshot_generation_with_top_level_checked(
        &self,
        snapshot: &crate::package::snapshot::Snapshot,
        cycle: &crate::host::facade::ProfileCycle,
        expected: &[u8; 32],
    ) -> Result<(crate::reload::Candidate, AST), crate::package::Error> {
        self.compile_package_snapshot_generation_checked(snapshot, cycle, expected)
            .map(crate::reload::Candidate::with_top_level)
    }

    /// Compile one discovered static package graph into an executable bundle.
    #[cfg(feature = "std")]
    pub fn compile_package_executable(
        &self,
        target: &crate::project::Target,
    ) -> Result<program::Executable, crate::package::Error> {
        crate::package::compile::executable(self, target, None)
    }

    /// Compile one loaded package graph to run as a program: its [`AST`]
    /// (the top level), and the script name of the entry package's
    /// `fn main`, when it has one, to call after it.
    #[cfg(feature = "std")]
    pub fn compile_package(
        &self,
        graph: crate::package::Graph,
    ) -> Result<(AST, Option<crate::ImmutableString>), crate::package::Error> {
        let (executable, ..) =
            crate::package::compile::executable_from_graph(self, graph, None, false)?;
        let main = executable
            .items()
            .find(|item| {
                let key = item.key();
                item.kind() == crate::item::Kind::Function
                    && key.package() == executable.package()
                    && key.modules().is_empty()
                    && key.name() == "main"
            })
            .map(|item| crate::resolve::lower_nominal::private_function_name(item.key()));
        Ok((executable.into_ast(), main))
    }

    /// Compile one immutable reload candidate from a static package graph.
    ///
    /// The returned bundle retains the exact executable, canonical source
    /// graph, application and preview contracts, schema manifest, callback
    /// manifest, and generation identity produced by one compiler
    /// transformation.
    #[cfg(feature = "std")]
    pub fn compile_package_generation(
        &self,
        target: &crate::project::Target,
    ) -> Result<crate::reload::Candidate, crate::package::Error> {
        crate::package::compile::generation(self, target, None)
    }

    /// Compile one reload candidate while recording the real compiler phases.
    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn compile_package_generation_observed(
        &self,
        target: &crate::project::Target,
        session: &crate::timing::Session,
    ) -> Result<crate::reload::Candidate, crate::package::Error> {
        crate::package::compile::generation(self, target, Some(session))
    }

    /// [`Engine::compile_package_generation`] of a graph already loaded:
    /// a host that judged the program from its load compiles that same parse.
    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn compile_loaded_package_generation(
        &self,
        graph: crate::package::Graph,
        session: Option<&crate::timing::Session>,
    ) -> Result<crate::reload::Candidate, crate::package::Error> {
        crate::package::compile::generation_from_graph(self, graph, session)
    }

    /// [`Engine::compile_package_application_descriptor`] of a graph already
    /// loaded.
    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn compile_loaded_package_application_descriptor(
        &self,
        graph: crate::package::Graph,
        session: Option<&crate::timing::Session>,
    ) -> Result<crate::application::Descriptor, crate::package::Error> {
        crate::package::compile::descriptor_from_graph(self, graph, session)
    }

    /// Compile and fully validate one application from a static package graph.
    #[cfg(feature = "std")]
    pub fn compile_package_application_descriptor(
        &self,
        target: &crate::project::Target,
    ) -> Result<crate::application::Descriptor, crate::package::Error> {
        self.compile_package_application_descriptor_with_session(target, None)
    }

    /// Compile one static source graph into a preview catalog.
    #[cfg(feature = "std")]
    pub fn compile_package_preview_catalog(
        &self,
        target: &crate::project::Target,
    ) -> Result<crate::preview::Catalog, crate::package::Error> {
        crate::package::compile::catalog(self, target, None)
    }

    /// Compile a preview catalog while recording the real compiler phases.
    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn compile_package_preview_catalog_observed(
        &self,
        target: &crate::project::Target,
        session: &crate::timing::Session,
    ) -> Result<crate::preview::Catalog, crate::package::Error> {
        crate::package::compile::catalog(self, target, Some(session))
    }

    /// Compile a package application while recording the real compiler phases.
    #[cfg(feature = "std")]
    #[doc(hidden)]
    pub fn compile_package_application_descriptor_observed(
        &self,
        target: &crate::project::Target,
        session: &crate::timing::Session,
    ) -> Result<crate::application::Descriptor, crate::package::Error> {
        self.compile_package_application_descriptor_with_session(target, Some(session))
    }

    /// Compile and require one restricted application-root candidate.
    ///
    /// Callback/type/capability unification is a separate promotion; this API
    /// proves only root shape, registered intrinsic identity, and stable
    /// symbolic callback selection.
    #[inline(always)]
    pub fn compile_application(
        &self,
        script: impl AsRef<str>,
    ) -> ParseResult<crate::application::Candidate> {
        self.compile_executable(script)?
            .require_application()
            .map_err(|error| crate::resolve::Error::from(error).into())
    }

    /// Compile and fully validate one normalized application descriptor.
    ///
    /// Rawr requires the selected message type to prove `Clone + Send + 'static`.
    /// This is intentionally stricter than iced's universal `Send + 'static`
    /// minimum so reloadable framework values retain ordinary Rawr clone
    /// semantics.
    #[inline(always)]
    pub fn compile_application_descriptor(
        &self,
        script: impl AsRef<str>,
    ) -> ParseResult<crate::application::Descriptor> {
        self.compile_application_descriptor_with_session(
            script.as_ref(),
            None,
            #[cfg(not(feature = "no_std"))]
            None,
        )
    }

    /// Compile an application from one exact retained source snapshot.
    #[doc(hidden)]
    pub fn compile_application_descriptor_source(
        &self,
        source: crate::diagnostic::Source,
    ) -> ParseResult<crate::application::Descriptor> {
        let text = source.text().to_owned();
        self.compile_application_descriptor_with_session(
            &text,
            Some(source),
            #[cfg(not(feature = "no_std"))]
            None,
        )
    }

    /// Compile one exact retained source snapshot into a preview catalog.
    #[doc(hidden)]
    pub fn compile_preview_catalog_source(
        &self,
        source: crate::diagnostic::Source,
    ) -> ParseResult<crate::preview::Catalog> {
        let text = source.text().to_owned();
        let executable = self.compile_executable_scripts_with_scope_raw(
            Some(&Scope::new()),
            &[text],
            Some(source),
            #[cfg(not(feature = "no_std"))]
            None,
            #[cfg(not(feature = "no_optimize"))]
            self.optimization_level,
        )?;
        executable
            .require_previews()
            .map_err(|error| crate::resolve::Error::from(error).into())
    }

    /// Compile one application while recording the exact compiler phases.
    ///
    /// This is an inter-crate host seam. Timing remains observational and is
    /// never retained by the returned application descriptor.
    #[cfg(not(feature = "no_std"))]
    #[doc(hidden)]
    pub fn compile_application_descriptor_observed(
        &self,
        script: impl AsRef<str>,
        session: &crate::timing::Session,
    ) -> ParseResult<crate::application::Descriptor> {
        self.compile_application_descriptor_with_session(script.as_ref(), None, Some(session))
    }

    /// Compile an exact retained source snapshot while recording compiler phases.
    #[cfg(not(feature = "no_std"))]
    #[doc(hidden)]
    pub fn compile_application_descriptor_source_observed(
        &self,
        source: crate::diagnostic::Source,
        session: &crate::timing::Session,
    ) -> ParseResult<crate::application::Descriptor> {
        let text = source.text().to_owned();
        self.compile_application_descriptor_with_session(&text, Some(source), Some(session))
    }

    /// Compile source into an opaque authority-eligible executable bundle.
    ///
    /// Unlike a bare [`AST`], this bundle retains the stable resolved metadata
    /// paired with the exact lowering produced from the same transformation.
    #[inline(always)]
    pub fn compile_executable(&self, script: impl AsRef<str>) -> ParseResult<program::Executable> {
        self.compile_executable_with_scope(&Scope::new(), script)
    }

    /// Compile source into an executable bundle using constants from a scope.
    #[inline(always)]
    pub fn compile_executable_with_scope(
        &self,
        scope: &Scope,
        script: impl AsRef<str>,
    ) -> ParseResult<program::Executable> {
        self.compile_executable_scripts_with_scope_raw(
            Some(scope),
            &[script],
            None,
            #[cfg(not(feature = "no_std"))]
            None,
            #[cfg(not(feature = "no_optimize"))]
            self.optimization_level,
        )
    }

    /// Compile and authorize one program using this shared host engine.
    ///
    /// Each call issues a fresh opaque generation even when stable program
    /// metadata is identical.
    #[inline(always)]
    pub fn compile_authority(
        self: &Shared<Self>,
        script: impl AsRef<str>,
    ) -> Result<program::Authority, program::compile::Error> {
        let executable = self.compile_executable(script)?;
        self.authorize(executable).map_err(Into::into)
    }

    /// Bind one intact executable bundle to the exact host engine that compiled it.
    ///
    /// The authority adopts the executable's existing generation. The bundle
    /// is consumed as a whole; callers cannot replace its stable metadata,
    /// AST, or compiler-support section independently.
    #[must_use = "authorization failures must be handled"]
    pub fn authorize(
        self: &Shared<Self>,
        executable: program::Executable,
    ) -> Result<program::Authority, program::Error> {
        program::Authority::new(self.clone(), executable)
    }

    /// Consume a validated descriptor into an origin-checked launch proof.
    pub fn launch_application(
        self: &Shared<Self>,
        descriptor: crate::application::Descriptor,
    ) -> Result<crate::application::Launch, program::Error> {
        let (executable, contract, fingerprint) = descriptor.into_parts();
        let authority = self.authorize(executable)?;
        Ok(crate::application::Launch::new(
            contract,
            fingerprint,
            authority,
        ))
    }

    /// Consume a preview catalog into an origin-checked launch proof.
    pub fn launch_previews(
        self: &Shared<Self>,
        catalog: crate::preview::Catalog,
    ) -> Result<crate::preview::Launch, program::Error> {
        let (executable, application, dependencies, projections, boundary_types) =
            catalog.into_parts();
        let authority = self.authorize(executable)?;
        Ok(crate::preview::Launch::new(
            application,
            dependencies,
            projections,
            boundary_types,
            authority,
        ))
    }

    /// Compile a string into an [`AST`], which can be used later for evaluation.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// // Compile a script to an AST and store it for later evaluation
    /// let ast = engine.compile("40 + 2")?;
    ///
    /// for _ in 0..42 {
    ///     assert_eq!(engine.eval_ast::<i64>(&ast)?, 42);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn compile(&self, script: impl AsRef<str>) -> ParseResult<AST> {
        self.compile_with_scope(&Scope::new(), script)
    }
    /// Compile a string into an [`AST`] using own scope, which can be used later for evaluation.
    ///
    /// ## Constants Propagation
    ///
    /// If not [`OptimizationLevel::None`][crate::OptimizationLevel::None], constants defined within
    /// the scope are propagated throughout the script _including_ functions. This allows functions
    /// to be optimized based on dynamic global constants.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # #[cfg(not(feature = "no_optimize"))]
    /// # {
    /// use rawr::{Engine, Scope, OptimizationLevel};
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Create initialized scope
    /// let mut scope = Scope::new();
    /// scope.push_constant("x", 42_i64);   // 'x' is a constant
    ///
    /// // Compile a script to an AST and store it for later evaluation.
    /// // Notice that `Full` optimization is on, so constants are folded
    /// // into function calls and operators.
    /// let ast = engine.compile_with_scope(&mut scope,
    ///             "if x > 40 { x } else { 0 }"    // all 'x' are replaced with 42
    /// )?;
    ///
    /// // Normally this would have failed because no scope is passed into the 'eval_ast'
    /// // call and so the variable 'x' does not exist.  Here, it passes because the script
    /// // has been optimized and all references to 'x' are already gone.
    /// assert_eq!(engine.eval_ast::<i64>(&ast)?, 42);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn compile_with_scope(&self, scope: &Scope, script: impl AsRef<str>) -> ParseResult<AST> {
        self.compile_scripts_with_scope(scope, &[script])
    }
    /// Compile a string into an [`AST`] using own scope, which can be used later for evaluation,
    /// embedding all imported modules.
    ///
    /// Not available under `no_module`.
    ///
    /// Modules referred by `import` statements containing literal string paths are eagerly resolved
    /// via the current [module resolver][crate::ModuleResolver] and embedded into the resultant
    /// [`AST`]. When it is evaluated later, `import` statement directly recall pre-resolved
    /// [modules][crate::Module] and the resolution process is not performed again.
    #[cfg(not(feature = "no_module"))]
    pub fn compile_into_self_contained(
        &self,
        scope: &Scope,
        script: impl AsRef<str>,
    ) -> crate::RawrResultOf<AST> {
        use crate::{
            ast::{ASTNode, Expr, Stmt},
            func::native::shared_take_or_clone,
            module::resolvers::StaticModuleResolver,
        };
        use std::collections::BTreeSet;

        fn collect_imports(
            ast: &AST,
            resolver: &StaticModuleResolver,
            imports: &mut BTreeSet<crate::Identifier>,
        ) {
            ast._walk(&mut |path| match path.last().unwrap() {
                // Collect all `import` statements with a string constant path
                ASTNode::Stmt(Stmt::Import(x, ..)) => match x.0 {
                    Expr::StringConstant(ref s, ..)
                        if !resolver.contains_path(s)
                            && (imports.is_empty() || !imports.contains(s.as_str())) =>
                    {
                        imports.insert(s.clone().into());
                        true
                    }
                    _ => true,
                },
                _ => true,
            });
        }

        let mut ast = self.compile_with_scope(scope, script)?;

        let mut resolver = StaticModuleResolver::new();
        let mut imports = BTreeSet::new();

        collect_imports(&ast, &resolver, &mut imports);

        if !imports.is_empty() {
            while let Some(path) = imports.pop_first() {
                let path = path.clone();

                match self
                    .module_resolver()
                    .resolve_ast(self, None, &path, crate::Position::NONE)
                {
                    Some(Ok(module_ast)) => collect_imports(&module_ast, &resolver, &mut imports),
                    Some(err) => return err,
                    None => (),
                }

                let module =
                    self.module_resolver()
                        .resolve(self, None, &path, crate::Position::NONE)?;

                let module = shared_take_or_clone(module);

                resolver.insert(path, module);
            }
            ast.resolver = Some(resolver.into());
        }

        Ok(ast)
    }
    /// When passed a list of strings, first join the strings into one large script, and then
    /// compile them into an [`AST`] using own scope, which can be used later for evaluation.
    ///
    /// The scope is useful for passing constants into the script for optimization when using
    /// [`OptimizationLevel::Full`][crate::OptimizationLevel::Full].
    ///
    /// ## Note
    ///
    /// All strings are simply parsed one after another with nothing inserted in between, not even a
    /// newline or space.
    ///
    /// ## Constants Propagation
    ///
    /// If not [`OptimizationLevel::None`][crate::OptimizationLevel::None], constants defined within
    /// the scope are propagated throughout the script _including_ functions. This allows functions
    /// to be optimized based on dynamic global constants.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # #[cfg(not(feature = "no_optimize"))]
    /// # {
    /// use rawr::{Engine, Scope, OptimizationLevel};
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Create initialized scope
    /// let mut scope = Scope::new();
    /// scope.push_constant("x", 42_i64);   // 'x' is a constant
    ///
    /// // Compile a script made up of script segments to an AST and store it for later evaluation.
    /// // Notice that `Full` optimization is on, so constants are folded
    /// // into function calls and operators.
    /// let ast = engine.compile_scripts_with_scope(&mut scope, &[
    ///             "if x > 40",            // all 'x' are replaced with 42
    ///             "{ x } el",
    ///             "se { 0 }"              // segments do not need to be valid scripts!
    /// ])?;
    ///
    /// // Normally this would have failed because no scope is passed into the 'eval_ast'
    /// // call and so the variable 'x' does not exist.  Here, it passes because the script
    /// // has been optimized and all references to 'x' are already gone.
    /// assert_eq!(engine.eval_ast::<i64>(&ast)?, 42);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn compile_scripts_with_scope<S: AsRef<str>>(
        &self,
        scope: &Scope,
        scripts: impl AsRef<[S]>,
    ) -> ParseResult<AST> {
        self.compile_scripts_with_scope_raw(
            Some(scope),
            scripts,
            #[cfg(not(feature = "no_optimize"))]
            self.optimization_level,
        )
    }
    /// Join a list of strings and compile into an [`AST`] using own scope at a specific optimization level.
    ///
    /// ## Constants Propagation
    ///
    /// If not [`OptimizationLevel::None`][`crate::OptimizationLevel::None`], constants defined within the scope are propagated
    /// throughout the script _including_ functions. This allows functions to be optimized based on
    /// dynamic global constants.
    #[inline]
    pub(crate) fn compile_scripts_with_scope_raw<S: AsRef<str>>(
        &self,
        scope: Option<&Scope>,
        scripts: impl AsRef<[S]>,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> ParseResult<AST> {
        self.compile_executable_scripts_with_scope_raw(
            scope,
            scripts,
            None,
            #[cfg(not(feature = "no_std"))]
            None,
            #[cfg(not(feature = "no_optimize"))]
            optimization_level,
        )
        .map(program::Executable::into_ast)
    }

    fn compile_executable_scripts_with_scope_raw<S: AsRef<str>>(
        &self,
        scope: Option<&Scope>,
        scripts: impl AsRef<[S]>,
        evidence: Option<crate::diagnostic::Source>,
        #[cfg(not(feature = "no_std"))] session: Option<&crate::timing::Session>,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> ParseResult<program::Executable> {
        let source = scripts
            .as_ref()
            .iter()
            .fold(String::new(), |mut source, script| {
                source.push_str(script.as_ref());
                source
            });
        #[cfg(not(feature = "no_std"))]
        let unit =
            crate::timing::observe_optional(session, crate::timing::Phase::ParseItems, || {
                self.parse_unit_with_scope_raw(
                    scope,
                    &source,
                    #[cfg(not(feature = "no_optimize"))]
                    optimization_level,
                )
            })?;
        #[cfg(feature = "no_std")]
        let unit = self.parse_unit_with_scope_raw(
            scope,
            &source,
            #[cfg(not(feature = "no_optimize"))]
            optimization_level,
        )?;

        #[cfg(not(feature = "no_std"))]
        let program =
            crate::timing::observe_optional(session, crate::timing::Phase::Resolve, || {
                match evidence {
                    Some(evidence) => {
                        crate::resolve::resolve_with_evidence(unit, &self.host_registry, evidence)
                    }
                    None => crate::resolve::resolve(unit, &self.host_registry),
                }
            })?;
        #[cfg(feature = "no_std")]
        let program = match evidence {
            Some(evidence) => {
                crate::resolve::resolve_with_evidence(unit, &self.host_registry, evidence)
            }
            None => crate::resolve::resolve(unit, &self.host_registry),
        }?;

        #[cfg(not(feature = "no_std"))]
        return crate::timing::observe_optional(session, crate::timing::Phase::Lower, || {
            crate::resolve::lower_with_engine(
                self,
                program,
                scope,
                #[cfg(not(feature = "no_optimize"))]
                optimization_level,
            )
        });
        #[cfg(feature = "no_std")]
        crate::resolve::lower_with_engine(
            self,
            program,
            scope,
            #[cfg(not(feature = "no_optimize"))]
            optimization_level,
        )
    }

    fn compile_application_descriptor_with_session(
        &self,
        script: &str,
        evidence: Option<crate::diagnostic::Source>,
        #[cfg(not(feature = "no_std"))] session: Option<&crate::timing::Session>,
    ) -> ParseResult<crate::application::Descriptor> {
        let executable = self.compile_executable_scripts_with_scope_raw(
            Some(&Scope::new()),
            [script],
            evidence,
            #[cfg(not(feature = "no_std"))]
            session,
            #[cfg(not(feature = "no_optimize"))]
            self.optimization_level,
        )?;

        #[cfg(not(feature = "no_std"))]
        return crate::timing::observe_optional(
            session,
            crate::timing::Phase::ApplicationContract,
            || {
                executable
                    .require_application()
                    .map_err(crate::resolve::Error::from)?
                    .validate()
                    .map_err(|error| crate::resolve::Error::from(error).into())
            },
        );
        #[cfg(feature = "no_std")]
        executable
            .require_application()
            .map_err(crate::resolve::Error::from)?
            .validate()
            .map_err(|error| crate::resolve::Error::from(error).into())
    }

    #[cfg(feature = "std")]
    fn compile_package_application_descriptor_with_session(
        &self,
        target: &crate::project::Target,
        session: Option<&crate::timing::Session>,
    ) -> Result<crate::application::Descriptor, crate::package::Error> {
        crate::package::compile::descriptor(self, target, session)
    }

    /// Parse the masked legacy portion of a Rawr unit into its temporary AST.
    pub(crate) fn compile_legacy_script_with_scope_raw(
        &self,
        scope: Option<&Scope>,
        script: &str,
        #[cfg(not(feature = "no_optimize"))] optimization_level: crate::OptimizationLevel,
    ) -> ParseResult<AST> {
        let scripts = [script];
        let (stream, tc) = self.lex(&scripts);

        let input = &mut stream.peekable();
        let lib = &mut <_>::default();
        let state = ParseState::new(scope, input, tc.clone(), lib);
        let mut _ast = self.parse(
            state,
            #[cfg(not(feature = "no_optimize"))]
            optimization_level,
        )?;
        #[cfg(feature = "metadata")]
        {
            let global_comments = &tc.borrow().global_comments;
            _ast.doc = global_comments.into();
        }
        Ok(_ast)
    }
    /// Compile a string containing an expression into an [`AST`],
    /// which can be used later for evaluation.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// // Compile a script to an AST and store it for later evaluation
    /// let ast = engine.compile_expression("40 + 2")?;
    ///
    /// for _ in 0..42 {
    ///     assert_eq!(engine.eval_ast::<i64>(&ast)?, 42);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn compile_expression(&self, script: impl AsRef<str>) -> ParseResult<AST> {
        self.compile_expression_with_scope(&Scope::new(), script)
    }
    /// Compile a string containing an expression into an [`AST`] using own scope,
    /// which can be used later for evaluation.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # #[cfg(not(feature = "no_optimize"))]
    /// # {
    /// use rawr::{Engine, Scope, OptimizationLevel};
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Create initialized scope
    /// let mut scope = Scope::new();
    /// scope.push_constant("x", 10_i64);   // 'x' is a constant
    ///
    /// // Compile a script to an AST and store it for later evaluation.
    /// let ast = engine.compile_expression_with_scope(&mut scope,
    ///             "2 + (x + x) * 2"    // all 'x' are replaced with 10
    /// )?;
    ///
    /// // Normally this would have failed because no scope is passed into the 'eval_ast'
    /// // call and so the variable 'x' does not exist.  Here, it passes because the script
    /// // has been optimized and all references to 'x' are already gone.
    /// assert_eq!(engine.eval_ast::<i64>(&ast)?, 42);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn compile_expression_with_scope(
        &self,
        scope: &Scope,
        script: impl AsRef<str>,
    ) -> ParseResult<AST> {
        let scripts = [script];
        let (stream, t) = self.lex(&scripts);

        let input = &mut stream.peekable();
        let lib = &mut <_>::default();
        let state = ParseState::new(Some(scope), input, t, lib);

        self.parse_global_expr(
            state,
            |_| {},
            #[cfg(not(feature = "no_optimize"))]
            self.optimization_level,
        )
    }
}
