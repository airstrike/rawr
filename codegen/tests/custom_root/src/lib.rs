#[test]
fn custom_root_test() {
    #[derive(Clone, custom_rawr::CustomType)]
    #[rawr_type(name = "MyBar", extra = Self::build_extra, root=custom_rawr)]
    pub struct Bar(
        #[rawr_type(skip)] f32,
        u32,
        #[rawr_type(name = "boo", readonly)] String,
        Vec<u32>,
    );

    impl Bar {
        fn build_extra(builder: &mut custom_rawr::TypeBuilder<Self>) {
            builder.with_fn("new_int", || 42);
        }
    }

    let skipped = Bar(0.0, 0, String::new(), Vec::new());
    assert_eq!(skipped.0, 0.0);

    let mut engine = custom_rawr::Engine::new();
    engine.build_type::<Bar>();

    assert_eq!(
        engine
            .eval::<i32>(
                "
                        new_int()
                    "
            )
            .unwrap(),
        42
    );
}

#[test]
fn custom_root_module_test() -> Result<(), Box<custom_rawr::EvalAltResult>> {
    #[custom_rawr::plugin::export_module(root = "custom_rawr")]
    pub mod advanced_math {
        use custom_rawr::FLOAT;
        pub fn get_mystic_number() -> FLOAT {
            42.0 as FLOAT
        }
    }

    let mut engine = custom_rawr::Engine::new();
    let m = custom_rawr::exported_module!(advanced_math);
    engine.register_static_module("Math::Advanced", m.into());

    assert_eq!(
        engine.eval::<custom_rawr::FLOAT>(r#"let m = Math::Advanced::get_mystic_number();m"#)?,
        42.0
    );
    Ok(())
}
