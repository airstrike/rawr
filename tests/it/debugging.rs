#![cfg(feature = "debugging")]
use rawr::{Engine, INT};

use rawr::Array;

use rawr::Map;

#[test]
fn test_debugging() {
    let mut engine = Engine::new();

    engine.register_debugger(|_, dbg| dbg, |_, _, _, _, _| Ok(rawr::debugger::DebuggerCommand::Continue));

    assert_eq!(engine.eval::<Array>("back_trace()").unwrap().len(), 0);
}

#[test]
fn test_debugger_state() {
    let mut engine = Engine::new();

    engine.register_debugger(
        |_, mut debugger| {
            // Say, use an object map for the debugger state
            let mut state = Map::new();
            // Initialize properties
            state.insert("hello".into(), (42 as INT).into());
            state.insert("foo".into(), false.into());
            debugger.set_state(state);
            debugger
        },
        |mut context, _, _, _, _| {
            // Print debugger state - which is an object map
            println!("Current state = {}", context.global_runtime_state().debugger().state());

            // Modify state
            let mut state = context.global_runtime_state_mut().debugger_mut().state_mut().as_map_mut().unwrap();
            let hello = state.get("hello").unwrap().as_int().unwrap();
            state.insert("hello".into(), (hello + 1).into());
            state.insert("foo".into(), true.into());
            state.insert("something_new".into(), "hello, world!".into());

            // Continue with debugging
            Ok(rawr::debugger::DebuggerCommand::StepInto)
        },
    );

    engine.run("let x = 42;").unwrap();
}
