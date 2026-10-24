#![cfg(not(feature = "no_module"))]
use rawr::{Dynamic, Engine, EvalAltResult, FuncRegistration, INT, Module};

#[test]
fn test_module() {
    let mut module = Module::new();
    module.set_var("answer", 42 as INT);

    assert!(module.contains_var("answer"));
    assert_eq!(module.get_var_value::<INT>("answer").unwrap(), 42);
}

#[test]
fn test_module_syntax() {
    let engine = Engine::new();
    let _ = engine.compile("abc.def::xyz").unwrap_err();
    let _ = engine.compile("abc.def::xyz()").unwrap_err();
}

#[test]
fn test_module_sub_module() {
    let mut module = Module::new();

    let mut sub_module = Module::new();

    let mut sub_module2 = Module::new();
    sub_module2.set_var("answer", 41 as INT);

    let hash_inc = sub_module2.set_native_fn("inc", |x: &mut INT| Ok(*x + 1));
    sub_module2.build_index();
    assert!(!sub_module2.contains_indexed_global_functions());

    let f = |x: &mut INT| *x + 1;
    FuncRegistration::new("super_inc").in_global_namespace().set_into_module(&mut sub_module2, f);

    sub_module2.build_index();
    assert!(sub_module2.contains_indexed_global_functions());

    #[cfg(not(feature = "no_object"))]
    sub_module2.set_getter_fn("doubled", |x: &mut INT| Ok(*x * 2));

    sub_module.set_sub_module("universe", sub_module2);
    module.set_sub_module("life", sub_module);
    module.set_var("MYSTIC_NUMBER", Dynamic::from(42 as INT));
    module.build_index();

    assert!(module.contains_indexed_global_functions());

    assert!(module.contains_sub_module("life"));
    let m = module.get_sub_module("life").unwrap();

    assert!(m.contains_sub_module("universe"));
    let m2 = m.get_sub_module("universe").unwrap();

    assert!(m2.contains_var("answer"));
    assert!(m2.contains_fn(hash_inc));

    assert_eq!(m2.get_var_value::<INT>("answer").unwrap(), 41);

    // A static module's variables, functions and sub-modules are reachable
    // only through module paths, which typed scripts do not resolve. The
    // module is built and read from Rust.
}

#[test]
fn test_module_dynamic() {
    fn test_fn(input: Dynamic, x: INT) -> Result<INT, Box<EvalAltResult>> {
        let s = input.into_string().unwrap();
        Ok(s.len() as INT + x)
    }

    let mut engine = rawr::Engine::new();
    engine.register_fn("test2", test_fn);

    assert_eq!(engine.eval::<INT>(r#"test2("test", 38)"#).unwrap(), 42);
}
