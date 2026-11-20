//! A program compiled once on its own engine, run again for each count and
//! sample, with what it prints kept.

use std::sync::{Arc, Mutex};

/// A compiled program and what its last run printed.
pub struct Script {
    engine: rawr::Engine,
    ast: rawr::AST,
    printed: Arc<Mutex<String>>,
}

impl Script {
    /// `source` compiled on a fresh engine.
    ///
    /// # Panics
    ///
    /// When rawr refuses `source`: a workload's fixture is a program rawr
    /// runs, so a refusal is the fixture's bug or rawr's.
    pub fn compile(source: &str) -> Self {
        let printed = Arc::new(Mutex::new(String::new()));
        let mut engine = rawr::Engine::new();
        let sink = Arc::clone(&printed);
        engine.on_output(move |_, text| {
            sink.lock()
                .expect("only this callback takes the printed text")
                .push_str(text)
        });
        let ast = engine
            .compile(source)
            .unwrap_or_else(|error| panic!("a workload's fixture compiles: {error}"));
        Self {
            engine,
            ast,
            printed,
        }
    }

    /// Run the program once and give back what it printed.
    ///
    /// # Panics
    ///
    /// When the program fails, as a workload's fixture never does.
    pub fn run(&self) -> String {
        self.printed
            .lock()
            .expect("the program prints only while it runs")
            .clear();
        self.engine
            .run_ast(&self.ast)
            .unwrap_or_else(|error| panic!("a workload's fixture runs: {error}"));
        std::mem::take(
            &mut *self
                .printed
                .lock()
                .expect("the program prints only while it runs"),
        )
    }
}

/// The values after `name` on the line of `printed` that starts with it.
///
/// # Panics
///
/// When no line starts with `name`, or a value isn't a number: the
/// fixture prints every line its workload reads.
pub fn values(printed: &str, name: &str) -> Vec<f64> {
    let line = printed
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("the fixture prints `{name}`; it printed:\n{printed}"));
    line.split_whitespace()
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("`{name}` prints numbers, not `{value}`"))
        })
        .collect()
}
