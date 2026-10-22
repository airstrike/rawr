use rawr::{CustomType, Engine, TypeBuilder, INT};

// Sanity check to make sure everything compiles

#[derive(Clone, CustomType)]
pub struct Bar(
    #[rawr_type(skip)] rawr::FLOAT,
    INT,
    #[rawr_type(name = "boo", readonly)] String,
    Vec<INT>,
);

#[derive(Clone, Default, CustomType)]
#[rawr_type(name = "MyFoo", extra = Self::build_extra)]
pub struct Foo {
    #[rawr_type(skip)]
    _dummy: rawr::FLOAT,
    #[rawr_type(get = get_bar)]
    pub bar: INT,
    #[rawr_type(name = "boo", readonly)]
    pub(crate) baz: String,
    #[rawr_type(set = Self::set_qux)]
    pub qux: Vec<INT>,
    pub maybe: Option<INT>,
}

impl Foo {
    pub fn set_qux(&mut self, value: Vec<INT>) {
        self.qux = value;
    }

    fn build_extra(builder: &mut TypeBuilder<Self>) {
        builder.with_fn("new_foo", Self::default);
    }
}

fn get_bar(_this: &Foo) -> INT {
    42
}

#[test]
fn test() {
    let skipped = Bar(0.0, 0, String::new(), Vec::new());
    assert_eq!(skipped.0, 0.0);

    let mut engine = Engine::new();
    engine.build_type::<Foo>().build_type::<Bar>();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let foo = new_foo();
                    foo.bar = 42;
                    foo.bar
                "
            )
            .unwrap(),
        42
    );
}
