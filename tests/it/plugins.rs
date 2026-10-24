#![cfg(not(feature = "no_index"))]
#![cfg(not(feature = "no_module"))]
use rawr::plugin::*;
use rawr::{Engine, INT, Module};

mod test {
    use super::*;

    #[export_module]
    pub mod special_array_package {
        use rawr::{Array, INT};

        pub const MYSTIC_NUMBER: INT = 42;

        #[cfg(not(feature = "no_object"))]
        pub mod feature {
            use rawr::{Array, Dynamic, EvalAltResult};

            #[rawr_fn(get = "foo", return_raw)]
            #[inline(always)]
            pub fn foo(array: &mut Array) -> Result<Dynamic, Box<EvalAltResult>> {
                Ok(array[0].clone())
            }
        }

        pub fn hash(_text: String) -> INT {
            42
        }
        pub fn hash2(_text: &str) -> INT {
            42
        }

        #[rawr_fn(name = "test", name = "hi")]
        pub fn len(array: &mut Array, mul: INT) -> INT {
            (array.len() as INT) * mul
        }
        #[rawr_fn(name = "+")]
        pub fn funky_add(x: INT, y: INT) -> INT {
            x / 2 + y * 2
        }
        #[rawr_fn(name = "no_effect", set = "no_effect", pure)]
        pub fn no_effect(array: &mut Array, value: INT) {
            // array is not modified
            println!("Array = {array:?}, Value = {value}");
        }
    }
}

macro_rules! expand_enum {
    ($module:ident : $typ:ty => $($variant:ident),+) => {
        #[export_module]
        pub mod $module {
            $(
                #[allow(non_upper_case_globals)]
                pub const $variant: $typ = <$typ>::$variant;
            )*
        }
    };
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum MyEnum {
    Foo,
    Bar,
    Baz,
    Hello,
    World,
}

expand_enum! { my_enum_module: MyEnum => Foo, Bar, Baz, Hello, World }

#[test]
fn test_plugins_package() {
    let mut engine = Engine::new();

    let mut m = Module::new();
    combine_with_exported_module!(&mut m, "test", test::special_array_package);
    combine_with_exported_module!(&mut m, "enum", my_enum_module);
    engine.register_global_module(m.into());

    assert_eq!(engine.eval::<INT>(r#"hash("hello")"#).unwrap(), 42);
    assert_eq!(engine.eval::<INT>(r#"hash2("hello")"#).unwrap(), 42);
    assert_eq!(engine.eval::<INT>("let a = vec![1, 2, 3]; test(a, 2)").unwrap(), 6);
    assert_eq!(engine.eval::<INT>("let a = vec![1, 2, 3]; hi(a, 2)").unwrap(), 6);

    // The module's `+` does not change how integers add.
    assert_eq!(engine.eval::<INT>("2 + 2").unwrap(), 4);
}

#[test]
fn test_plugins_parameters() {
    #[export_module]
    mod rawr_std {
        pub fn noop(_: &str) {}
    }

    let mut engine = Engine::new();

    let std = exported_module!(rawr_std);

    engine.register_global_module(std.into());

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    let s = "hello";
                    noop(s);
                    s
                "#
            )
            .unwrap(),
        "hello"
    );
}
