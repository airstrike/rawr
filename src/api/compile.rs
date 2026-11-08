//! Module that defines the public compilation API of [`Engine`].

use crate::parser::{ParseResult, ParseState};
use crate::tokenizer::Token;
use crate::{program, Engine, Scope, Shared, AST};

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

    /// A reload generation of any program, a plain one (no application or
    /// previews, a `main` run once) included, retaining the same program for
    /// top-level evaluation before the generation is bound.
    #[cfg(feature = "rust-import")]
    #[doc(hidden)]
    pub fn compile_package_snapshot_generation_with_top_level(
        &self,
        snapshot: &crate::package::snapshot::Snapshot,
    ) -> Result<(crate::reload::Candidate, AST), crate::package::Error> {
        crate::package::compile::snapshot_generation_plain(self, snapshot)
            .map(crate::reload::Candidate::with_top_level)
    }

    /// Compile one discovered static package graph into an executable bundle.
    pub fn compile_package_executable(
        &self,
        target: &crate::project::Target,
    ) -> Result<program::Executable, crate::package::Error> {
        crate::package::compile::executable(self, target, None)
    }

    /// Compile one loaded package graph to run as a program: its [`AST`]
    /// (the top level), and the script name of the entry package's
    /// `fn main`, when it has one, to call after it.
    pub fn compile_package(
        &self,
        graph: crate::package::Graph,
    ) -> Result<(AST, Option<crate::ImmutableString>), crate::package::Error> {
        self.compile_package_with_positions(graph)
            .map(|(ast, main, _)| (ast, main))
    }

    /// [`Engine::compile_package`], with the [`program::Positions`] that map
    /// a runtime error's position back to the entry source.
    #[doc(hidden)]
    pub fn compile_package_with_positions(
        &self,
        graph: crate::package::Graph,
    ) -> Result<(AST, Option<crate::ImmutableString>, program::Positions), crate::package::Error>
    {
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
        let (ast, positions) = executable.into_ast_with_positions();
        Ok((ast, main, positions))
    }

    /// Compile one loaded package graph for its tests, as `rustc --test`
    /// builds a crate: its [`AST`] (the top level), its `#[test]`
    /// functions sorted by name, as libtest runs them, and the
    /// [`program::Positions`] that map a runtime error's position back to
    /// its source. The engine builds with `cfg(test)` holding when it was
    /// [set to](Engine::set_test) and loaded the graph so.
    #[doc(hidden)]
    pub fn compile_package_tests(
        &self,
        graph: crate::package::Graph,
    ) -> Result<(AST, Box<[crate::package::Test]>, program::Positions), crate::package::Error> {
        let declared = crate::package::test::Declared::of(&graph);
        let (executable, ..) =
            crate::package::compile::executable_from_graph(self, graph, None, false)?;
        let mut tests = declared
            .into_iter()
            .filter_map(|test| {
                let item = executable.items().find(|item| {
                    item.key().package() == executable.package() && test.is(item.key())
                })?;
                let function = crate::resolve::lower_nominal::private_function_name(item.key());
                Some(test.compiled(function))
            })
            .collect::<Vec<_>>();
        tests.sort_by(|left, right| left.name().cmp(right.name()));
        let (ast, positions) = executable.into_ast_with_positions();
        Ok((ast, tests.into_boxed_slice(), positions))
    }

    /// Compile one in-memory source this Engine already parsed (with
    /// [`Engine::parse_unit`]), naming its diagnostics by `source`: a host
    /// that judged the program from that parse (its
    /// [`crate::layer::Facts::of_unit`]) compiles the same parse.
    #[doc(hidden)]
    pub fn compile_unit(
        &self,
        unit: crate::parse::Unit,
        source: crate::diagnostic::Source,
    ) -> ParseResult<AST> {
        crate::package::compile::one_file(
            self,
            unit,
            (source.identity(), source.text()),
            (Some(&Scope::new()), self.optimization_level),
            None,
        )
        .map(program::Executable::into_hosted_ast)
    }

    /// [`Engine::compile_unit`], with the [`program::Positions`] that map a
    /// runtime error's position back to `source`.
    #[doc(hidden)]
    pub fn compile_unit_with_positions(
        &self,
        unit: crate::parse::Unit,
        source: crate::diagnostic::Source,
    ) -> ParseResult<(AST, program::Positions)> {
        crate::package::compile::one_file(
            self,
            unit,
            (source.identity(), source.text()),
            (Some(&Scope::new()), self.optimization_level),
            None,
        )
        .map(program::Executable::into_ast_with_positions)
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
            None,
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
        self.compile_scripts_named(scope, scripts, "<script>")
    }

    /// Compile scripts whose error, if any, prints as rustc prints it, with
    /// `identity` naming the source.
    pub(crate) fn compile_scripts_named<S: AsRef<str>>(
        &self,
        scope: &Scope,
        scripts: impl AsRef<[S]>,
        identity: &str,
    ) -> ParseResult<AST> {
        let compiled =
            self.compile_scripts_with_scope_raw(Some(scope), &scripts, self.optimization_level);

        compiled.map_err(|error| {
            let text: String = scripts.as_ref().iter().map(AsRef::as_ref).collect();
            error.rendered(identity, &text)
        })
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
        optimization_level: crate::OptimizationLevel,
    ) -> ParseResult<AST> {
        self.compile_executable_scripts_with_scope_raw(
            scope,
            scripts,
            None,
            None,
            optimization_level,
        )
        .map(|executable| {
            self.freeze_exports(executable.host_dependencies());
            // A runtime error in this program is at the script's own text,
            // in the walker and in anything compiled from the AST.
            let positions = executable.positions();
            let mut ast = executable.into_hosted_ast();
            crate::resolve::lower_nominal::relocate(&mut ast, &mut |position| {
                positions.located(position)
            });
            ast
        })
    }

    /// Compile one script into its hosted [`AST`] and the typed signatures
    /// of its free functions, for [`Script`](crate::Script). An error prints
    /// as rustc prints it, with `identity` naming the source.
    pub(crate) fn compile_script(
        &self,
        text: &str,
        identity: &str,
    ) -> ParseResult<(AST, program::signatures::Signatures)> {
        self.compile_executable_scripts_with_scope_raw(
            None,
            [text],
            None,
            None,
            self.optimization_level,
        )
        .map(|executable| {
            self.freeze_exports(executable.host_dependencies());
            let positions = executable.positions();
            let (mut ast, signatures) = executable.into_hosted_parts();
            crate::resolve::lower_nominal::relocate(&mut ast, &mut |position| {
                positions.located(position)
            });
            (ast, signatures)
        })
        .map_err(|error| error.rendered(identity, text))
    }

    fn compile_executable_scripts_with_scope_raw<S: AsRef<str>>(
        &self,
        scope: Option<&Scope>,
        scripts: impl AsRef<[S]>,
        evidence: Option<crate::diagnostic::Source>,
        session: Option<&crate::timing::Session>,
        optimization_level: crate::OptimizationLevel,
    ) -> ParseResult<program::Executable> {
        let source = scripts
            .as_ref()
            .iter()
            .fold(String::new(), |mut source, script| {
                source.push_str(script.as_ref());
                source
            });
        let unit =
            crate::timing::observe_optional(session, crate::timing::Phase::ParseItems, || {
                self.parse_unit(&source)
            })?;
        let identity = evidence
            .as_ref()
            .map_or("<script>", crate::diagnostic::Source::identity);
        crate::package::compile::one_file(
            self,
            unit,
            (identity, &source),
            (scope, optimization_level),
            session,
        )
    }

    /// Parse the lowered program text the evaluator runs. Only text the
    /// compiler generated reaches this parser; user input is checked as Rust.
    pub(crate) fn compile_legacy_script_with_scope_raw(
        &self,
        scope: Option<&Scope>,
        script: &str,
        optimization_level: crate::OptimizationLevel,
    ) -> ParseResult<AST> {
        let scripts = [script];
        let (stream, tc) = self.lex(&scripts);

        let input = &mut stream.peekable();
        let lib = &mut <_>::default();
        let state = ParseState::new(scope, input, tc.clone(), lib);
        let mut _ast = self.parse(state, optimization_level)?;
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
        self.expression_only(script.as_ref())?;
        self.compile_scripts_with_scope_raw(Some(scope), [script], self.optimization_level)
    }

    /// Refuse a script that is not one expression: a statement ends with
    /// `;` or starts with `let`, as rustc reports where it wants an
    /// expression.
    fn expression_only(&self, script: &str) -> ParseResult<()> {
        let scripts = [script];
        let (stream, _) = self.lex(&scripts);
        let mut depth = 0_usize;
        let mut first = true;
        for (token, position) in stream {
            let statement = match token {
                Token::EOF => break,
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace | Token::MapStart => {
                    depth += 1;
                    None
                }
                Token::RightParen | Token::RightBracket | Token::RightBrace => {
                    depth = depth.saturating_sub(1);
                    None
                }
                Token::SemiColon if depth == 0 => Some("found `;`".to_owned()),
                Token::Let if first => Some("found `let` statement".to_owned()),
                // An item is not an expression: `fn f() {}`, `const X: i64 = 1;`.
                Token::Fn | Token::Const | Token::Reserved(_) | Token::Identifier(_)
                    if first && starts_item(&token) =>
                {
                    Some(format!("found {}", crate::parser::item::found(&token)))
                }
                _ => None,
            };
            first = false;
            if let Some(found) = statement {
                return Err(crate::parser::item::unexpected(
                    format!("expected expression, {found}"),
                    "expected expression",
                    None,
                    &token,
                    position,
                ));
            }
        }
        Ok(())
    }
}

/// Whether `token` starts an item, which is not an expression.
fn starts_item(token: &Token) -> bool {
    match token {
        Token::Fn | Token::Const => true,
        Token::Reserved(word) | Token::Identifier(word) => matches!(
            word.as_str(),
            "struct" | "enum" | "impl" | "trait" | "use" | "mod" | "static" | "type" | "pub"
        ),
        _ => false,
    }
}
