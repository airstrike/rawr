use rawr::Engine;

#[test]
fn test_not() {
    let engine = Engine::new();

    assert!(!engine.eval::<bool>("let not_true = !true; not_true").unwrap());

    assert!(engine.eval::<bool>("fn not(x: bool) -> bool { !x } not(false)").unwrap());

    assert!(engine.eval::<bool>("!!!!true").unwrap());
}
