//! An example showing how to register a Rust type and use it with arrays.

#[cfg(any(feature = "no_index", feature = "no_object"))]
fn main() {
    panic!("This example does not run under 'no_index' or 'no_object'.")
}

use rawr::{Engine, EvalAltResult};

#[cfg(not(feature = "no_index"))]
#[cfg(not(feature = "no_object"))]
fn main() -> Result<(), Box<EvalAltResult>> {
    #[derive(Debug, Clone)]
    struct TestStruct {
        x: i64,
    }

    impl TestStruct {
        pub fn new() -> Self {
            Self { x: 1 }
        }
        pub fn update(&mut self) {
            self.x += 1000;
        }
    }

    let mut engine = Engine::new();

    engine
        .register_type_with_name::<TestStruct>("TestStruct")
        .register_fn("new_ts", TestStruct::new)
        .register_fn("update", TestStruct::update);

    #[cfg(feature = "metadata")]
    {
        println!("Functions registered:");

        engine
            .gen_fn_signatures(false)
            .into_iter()
            .for_each(|func| println!("{func}"));

        println!();
    }

    // A registered function's `&mut` first parameter takes the variable by
    // reference, so `update(x)` changes `x`.
    let result = engine.eval::<TestStruct>(
        "
            let mut x = new_ts();
            update(x);
            x
        ",
    )?;

    println!("{result:?}");

    let result = engine.eval::<TestStruct>(
        "
            let mut x = vec![new_ts()];
            // The element is taken out, updated through its variable and put
            // back, because only a variable is handed over by reference.
            let mut first = x.remove(0);
            update(first);
            x.push(first);
            x[0]
        ",
    )?;

    println!("{result:?}");

    Ok(())
}
