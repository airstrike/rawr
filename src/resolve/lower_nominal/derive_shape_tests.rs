//! The constructs a derive's expansion writes, as serde's does, in
//! programs that use only std. Each expected value is what rustc's build of
//! the same program gives.

use crate::Engine;

fn run_string(source: &str) -> String {
    Engine::new()
        .eval::<String>(source)
        .unwrap_or_else(|error| panic!("{error}"))
}

fn rejected(source: &str) -> String {
    match Engine::new().eval::<String>(source) {
        Ok(value) => panic!("Rust rejects this program, but it evaluated to {value}: {source}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_derives_expansion_shape_runs_as_rust() {
    // An unnamed `const _` block of items that imports a crate under
    // another name, a function's own enum, struct and impl with a lifetime,
    // a where-clause, a method turbofish, byte-string patterns and `as`
    // arithmetic.
    let value = run_string(
        r#"
        struct Task { description: String, done: bool }

        #[doc(hidden)]
        #[allow(non_upper_case_globals, unused_attributes, unused_qualifications,
        clippy :: absolute_paths,)]
        const _: () =
            {
                #[allow(unused_extern_crates, clippy :: useless_attribute)]
                extern crate std as _std;
                ;
                #[automatically_derived]
                impl _std::fmt::Display for Task {
                    fn fmt(&self, __formatter: &mut _std::fmt::Formatter)
                        -> _std::fmt::Result {
                        __formatter.write_str(&self.description)?;
                        __formatter.write_str(if self.done { " (done)" } else { " (open)" })
                    }
                }
            };

        fn field(name: &str, bytes: &[u8]) -> usize {
            #[allow(non_camel_case_types)]
            enum __Field { __field0, __field1, __ignore, }
            struct __Visitor<'de> {
                marker: std::marker::PhantomData<Task>,
                lifetime: std::marker::PhantomData<&'de ()>,
            }
            impl<'de> __Visitor<'de> {
                fn by_name<__E>(&self, __value: &str) -> Result<__Field, __E> where
                    __E: Default {
                    match __value {
                        "description" => Ok(__Field::__field0),
                        "done" => Ok(__Field::__field1),
                        _ => { Ok(__Field::__ignore) }
                    }
                }
            }
            fn by_bytes(__value: &[u8]) -> __Field {
                match __value {
                    b"description" => __Field::__field0,
                    b"done" => __Field::__field1,
                    _ => { __Field::__ignore }
                }
            }
            let visitor = __Visitor {
                marker: std::marker::PhantomData::<Task>,
                lifetime: std::marker::PhantomData,
            };
            let named: Result<__Field, String> = visitor.by_name::<String>(name);
            let index = |field: __Field| match field {
                __Field::__field0 => 0usize,
                __Field::__field1 => 1usize,
                __Field::__ignore => 2usize,
            };
            index(named.unwrap()) * 10 + index(by_bytes(bytes))
        }

        pub fn run() -> String {
            let task = Task { description: "write".to_string(), done: false };
            let mut field0: Option<String> = None;
            if !Option::is_some(&field0) {
                field0 = Some(<String as Default>::default() + "empty");
            }
            format!(
                "{task} {} {} {} {} {:?}",
                false as usize + 1 + 1,
                field("done", b"description"),
                field("x", b"done"),
                field("description", b"y"),
                field0,
            )
        }
        run()
    "#,
    );
    assert_eq!(value, "write (open) 2 10 21 2 Some(\"empty\")");
}

#[test]
fn a_crate_root_renamed_by_use_names_its_paths() {
    // `use std as s;` at the top of a file: `s::fmt::Display` is
    // `std::fmt::Display` in an impl's header and its signatures.
    let value = run_string(
        r#"
        use std as s;
        struct T;
        impl s::fmt::Display for T {
            fn fmt(&self, f: &mut s::fmt::Formatter) -> s::fmt::Result { f.write_str("t") }
        }
        pub fn run() -> String { format!("{}", T) }
        run()
    "#,
    );
    assert_eq!(value, "t");
}

#[test]
fn a_generic_trait_function_called_by_its_path() {
    // `Trait::f::<T>(&x)` and `Trait::f(&x)` of a generic trait function
    // on a program type call the type's impl, as `x.f::<T>()` does.
    let value = run_string(
        r#"
        trait Pick { fn pick<T: From<i64> + Default>(&self) -> T; }
        struct Pair(i64);
        impl Pick for Pair {
            fn pick<T: From<i64> + Default>(&self) -> T {
                if self.0 > 0 { T::from(self.0) } else { T::default() }
            }
        }
        pub fn run() -> String {
            let total = Pick::pick::<i64>(&Pair(6));
            let zero: i64 = Pick::pick(&Pair(0));
            let method = Pair(3).pick::<i64>();
            format!("{total} {zero} {method}")
        }
        run()
    "#,
    );
    assert_eq!(value, "6 0 3");
}

#[test]
fn a_method_turbofish_names_the_methods_own_parameters() {
    // rustc's E0107: `get` has one type parameter.
    let error = rejected(
        "struct S; impl S { fn get<T: Default>(&self) -> T { T::default() } } \
         pub fn run() -> String { let n: i64 = S.get::<i64, i64>(); format!(\"{n}\") } run()",
    );
    assert!(error.contains("expected 1, found 2"), "{error}");
}

#[test]
fn a_statement_in_an_unnamed_const_is_not_supported_yet() {
    let error =
        rejected("const _: () = { let x = 1; }; pub fn run() -> String { String::new() } run()");
    assert!(
        error.contains("a statement in the block of `const _` is not supported yet"),
        "{error}"
    );
}
