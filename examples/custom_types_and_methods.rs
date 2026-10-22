//! An example showing how to register a Rust type and methods/getters/setters for it.

#[cfg(feature = "no_object")]
fn main() {
    panic!("This example does not run under 'no_object'.");
}

use rawr::{CustomType, Engine, EvalAltResult, TypeBuilder};

#[cfg(not(feature = "no_object"))]
fn main() -> Result<(), Box<EvalAltResult>> {
    /// This is a test structure. If the metadata feature
    /// is enabled, this comment will be exported.
    #[derive(Debug, Clone, CustomType)]
    #[rawr_type(extra = Self::build_extra)]
    struct TestStruct {
        /// A number.
        ///
        /// ```js
        /// let t = new_ts();
        /// print(t.x); // Get the value of x.
        /// t.x = 42;   // Set the value of x.
        /// ```
        x: i64,
    }

    impl TestStruct {
        pub fn new() -> Self {
            Self { x: 1 }
        }
        pub fn update(&mut self) {
            self.x += 1000;
        }
        pub fn calculate(&mut self, data: i64) -> i64 {
            self.x * data
        }

        fn build_extra(builder: &mut TypeBuilder<Self>) {
            builder
                .with_fn("new_ts", TestStruct::new)
                .with_fn("update", TestStruct::update)
                .with_fn("calc", TestStruct::calculate);
        }
    }

    let mut engine = Engine::new();

    engine.build_type::<TestStruct>();

    #[cfg(feature = "metadata")]
    {
        println!("Functions registered:");

        engine
            .gen_fn_signatures(false)
            .into_iter()
            .for_each(|func| println!("{func}"));

        println!();

        let docs: serde_json::Value =
            serde_json::from_str(&engine.gen_fn_metadata_to_json(false).unwrap()).unwrap();

        // compare comments from the type.
        assert_eq!(
            docs["customTypes"][0]["docComments"],
            serde_json::json!([
                "/// This is a test structure. If the metadata feature",
                "/// is enabled, this comment will be exported."
            ])
        );

        // compare comments from the getter.
        assert_eq!(
            docs["functions"][1]["docComments"],
            serde_json::json!([
                "/// A number.",
                "///",
                "/// ```js",
                "/// let t = new_ts();",
                "/// print(t.x); // Get the value of x.",
                "/// t.x = 42;   // Set the value of x.",
                "/// ```"
            ])
        );

        // compare comments from the setter.
        assert_eq!(
            docs["functions"][3]["docComments"],
            serde_json::json!([
                "/// A number.",
                "///",
                "/// ```js",
                "/// let t = new_ts();",
                "/// print(t.x); // Get the value of x.",
                "/// t.x = 42;   // Set the value of x.",
                "/// ```"
            ])
        );
    }

    let result = engine.eval::<i64>(
        "
            let x = new_ts();
            x.x = 42;
            x.update();
            x.calc(x.x)
        ",
    )?;

    println!("result: {result}"); // prints 1085764

    Ok(())
}
