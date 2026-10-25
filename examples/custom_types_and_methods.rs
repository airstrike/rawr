//! Export a Rust type, its fields and its methods to a typed script.

use rawr::{Engine, EvalAltResult, Exports, Scope};

/// A counter the app owns. Scripts read and set its pub field.
#[derive(Debug, Clone)]
#[rawr::export]
pub struct TestStruct {
    pub x: i64,
}

#[rawr::export]
impl TestStruct {
    pub fn new() -> Self {
        Self { x: 1 }
    }

    pub fn update(&mut self) {
        self.x += 1000;
    }

    pub fn calculate(&self, data: i64) -> i64 {
        self.x * data
    }
}

impl Default for TestStruct {
    fn default() -> Self {
        Self::new()
    }
}

fn main() -> Result<(), Box<EvalAltResult>> {
    let mut app = Exports::new("app");
    app.add::<TestStruct>();
    let mut engine = Engine::new();
    engine.export(app)?;

    let ast = engine.compile(
        "
            use app::TestStruct;

            fn run() -> i64 {
                let mut x = TestStruct::new();
                x.x = 42;
                x.update();
                x.calculate(x.x)
            }
        ",
    )?;
    let result: i64 = engine.call_fn(&mut Scope::new(), &ast, "run", ())?;

    println!("result: {result}"); // prints 1085764

    Ok(())
}
