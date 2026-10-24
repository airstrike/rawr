use rawr::packages::Package;
use rawr::{Engine, INT, Module, Scope, def_package};

#[test]
fn test_packages() {
    def_package! {
        /// My custom package.
        MyPackage(m) {
            m.set_native_fn("hello", |x: INT| Ok(x + 1));
        }
    }

    let pkg = MyPackage::new();

    let make_call = |x: INT| {
        // Create a raw Engine - extremely cheap.
        let mut engine = Engine::new_raw();

        // Register packages - cheap.
        pkg.register_into_engine(&mut engine);

        // Create custom scope - cheap.
        let mut scope = Scope::new();

        // Push variable into scope - relatively cheap.
        scope.push("x", x);

        // Evaluate script.

        engine.eval_with_scope::<INT>(&mut scope, "hello(x) * hello(x)")
    };

    assert_eq!(make_call(42).unwrap(), 1849);
}

#[cfg(not(feature = "no_function"))]
#[cfg(not(feature = "no_module"))]
#[test]
fn test_packages_with_script() {
    let mut engine = Engine::new();
    let ast = engine.compile("pub fn foo(x: i64) -> i64 { x + 1 } pub fn bar(x: i64) -> i64 { foo(x) + 1 }").unwrap();

    let module = Module::eval_ast_as_new(Scope::new(), &ast, &engine).unwrap();
    engine.register_global_module(module.into());
    assert_eq!(engine.eval::<INT>("foo(41)").unwrap(), 42);
    assert_eq!(engine.eval::<INT>("bar(40)").unwrap(), 42);
}
