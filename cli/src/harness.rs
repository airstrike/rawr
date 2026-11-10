//! `rawr test`: a crate's `#[test]` functions, built as `cargo test`
//! builds them (`cfg(test)` holding) and run by libtest's harness
//! ([`rawr::package::test::harness`]) on the base language.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use rawr::package::test::harness::{self, Tests};
use rawr::package::Test;
use rawr::{diagnostic, CallFnOptions, Dynamic, Engine, EvalAltResult, Scope, AST};

use crate::base;

pub use harness::Options;

/// Build the loaded program for its tests and run those `options`
/// selects, reporting as libtest does. The exit status is libtest's: 101
/// when a test failed. For a Cargo crate, `cargo check` builds its tests
/// beside them, and its errors follow the report: rustc stays the
/// authority.
#[must_use]
pub fn run(loaded: base::Loaded, options: &Options) -> ExitCode {
    let cargo = crate::cargo::begin_tests(&loaded.graph);
    let code = run_tests(loaded, options);
    crate::cargo::after_run(cargo.map(crate::cargo::Pending::finish));
    code
}

/// The tests of a program compiled on the base language.
struct Base {
    engine: Engine,
    scope: Scope<'static>,
    ast: AST,
    target: rawr::project::Target,
    positions: Arc<rawr::program::Positions>,
}

impl Tests for Base {
    fn call(&mut self, test: &Test) -> Result<Dynamic, Box<EvalAltResult>> {
        self.engine.call_fn_with_options::<Dynamic>(
            CallFnOptions::new().eval_ast(false).rewind_scope(true),
            &mut self.scope,
            &self.ast,
            test.function(),
            (),
        )
    }

    fn located(&self, error: &EvalAltResult) -> diagnostic::Report {
        base::located(error, &self.target, &self.positions)
    }

    fn named<'path>(&self, path: &'path Path) -> &'path Path {
        base::cargo_path(path, &self.target).unwrap_or(path)
    }
}

/// [`run`]'s tests.
fn run_tests(loaded: base::Loaded, options: &Options) -> ExitCode {
    let base::Loaded { target, graph, .. } = loaded;
    let mut engine = base::engine(base::Build::Test);
    if options.captures() {
        harness::capture(&mut engine);
    }
    let (ast, tests, positions) = match engine.compile_package_tests(graph) {
        Ok(compiled) => compiled,
        Err(error) => return crate::failure(&base::compile_report(&error, &target)),
    };
    let positions = Arc::new(positions);
    base::locate_panics(&target, &positions);
    let mut scope = Scope::new();
    if let Err(error) = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast) {
        eprint!(
            "{}",
            diagnostic::panic(&base::located(&error, &target, &positions))
        );
        return ExitCode::from(harness::FAILED);
    }
    let mut base = Base {
        engine,
        scope,
        ast,
        target,
        positions,
    };
    ExitCode::from(harness::run(&tests, options, &mut base))
}
