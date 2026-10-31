use super::*;

#[test]
fn a_native_future_runs_where_it_is_polled_and_its_output_is_made_on_the_owner() {
    struct Plain;
    impl Host for Plain {
        fn invoke(&self, _: &item::Value, _: &mut [Dynamic]) -> RawrResultOf<Dynamic> {
            unreachable!()
        }
    }
    serve(Rc::new(Plain));
    let future = Future::join(vec![
        Future::native(|| Box::pin(async { Box::new(|| Ok(Dynamic::from(1_i64))) as Finish })),
        Future::ready(Dynamic::from(2_i64)),
    ]);
    let mut script = future.clone().into_script();
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let std::task::Poll::Ready(Ok(output)) = script.poll_output(&mut cx) else {
        panic!("the join is ready");
    };
    assert_eq!(format!("{output:?}"), "[1, 2]");
    // A copy shares the future, which runs once.
    let mut again = future.into_script();
    assert!(matches!(
        again.poll_output(&mut cx),
        std::task::Poll::Ready(Err(_))
    ));
}

#[cfg(feature = "rust-import")]
mod checked_places {
    use super::super::{Callable, Host, Program, Remote, HOST};
    use crate::host::runtime::{type_ref::Type, Registry, Witness};
    use crate::{Dynamic, Engine, ImmutableString, Shared};
    use std::any::TypeId;
    use std::rc::Rc;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Running(Program);

    impl Host for Running {
        fn invoke(
            &self,
            function: &crate::item::Value,
            arguments: &mut [Dynamic],
        ) -> crate::RawrResult {
            self.0.invoke(function, arguments)
        }

        fn share(&self) -> Option<Program> {
            Some(self.0.clone())
        }
    }

    struct Restore(Option<Rc<dyn Host>>);

    impl Drop for Restore {
        fn drop(&mut self) {
            HOST.with_borrow_mut(|host| *host = self.0.take());
        }
    }

    struct Selected {
        invokes: Arc<AtomicUsize>,
        next: Option<Program>,
    }

    impl Remote for Selected {
        fn validate(&self, _: &Witness, value: &Dynamic) -> Result<(), String> {
            if value.as_int() != Ok(17) {
                return Err("expected the checked integer".into());
            }
            if let Some(next) = &self.next {
                HOST.with_borrow_mut(|host| *host = Some(Rc::new(Running(next.clone()))));
            }
            Ok(())
        }

        fn invoke(&self, _: &crate::item::Value, arguments: &mut [Dynamic]) -> crate::RawrResult {
            self.invokes.fetch_add(1, Ordering::SeqCst);
            let place = crate::place::get(&arguments[1]).unwrap();
            let _ = place.invoke(crate::place::Operation::PushStr, &mut [" changed".into()])?;
            Ok(Dynamic::from_array(vec![
                17_i64.into(),
                "copied envelope".into(),
            ]))
        }

        fn rebinds(&self, _: &crate::item::Value, _: &dyn Remote) -> bool {
            true
        }
    }

    #[test]
    fn checked_places_validate_and_invoke_one_generation_without_restoring_native_storage() {
        let engine = Shared::new(Engine::new());
        let authority = engine
            .compile_authority("pub fn callback(a: &mut String, b: &mut String, value: i64) {}")
            .unwrap();
        let function = authority
            .items()
            .find(|item| item.key().name() == "callback")
            .unwrap()
            .clone();
        let invoked = Arc::new(AtomicUsize::new(0));
        let other_invoked = Arc::new(AtomicUsize::new(0));
        let other: Program = Arc::new(Selected {
            invokes: other_invoked.clone(),
            next: None,
        });
        let program: Program = Arc::new(Selected {
            invokes: invoked.clone(),
            next: Some(other),
        });
        let _restore =
            Restore(HOST.with_borrow_mut(|host| host.replace(Rc::new(Running(program.clone())))));
        let callable = Callable {
            function,
            program,
            _held: None,
        };
        let registry = Registry::new();
        let text = registry.witness_for(&Type::builtin("String", TypeId::of::<ImmutableString>()));
        let integer = registry.witness_for(&Type::builtin("i64", TypeId::of::<i64>()));
        let mut native = String::with_capacity(32);
        native.push_str("original");
        let allocation = native.as_ptr();
        let mut arguments = [
            "first".into(),
            crate::place::owned::<crate::place::text::Text>(native, &text).unwrap(),
            17_i64.into(),
        ];
        let result = callable
            .call_places_checked(
                &mut arguments,
                &[0, 1],
                &[None, None, Some(integer.clone())],
            )
            .unwrap();
        assert_eq!(result.as_int().unwrap(), 17);
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
        assert_eq!(other_invoked.load(Ordering::SeqCst), 0);
        let place = crate::place::get(&arguments[1]).unwrap();
        let native = crate::place::take::<crate::place::text::Text>(&place, &text).unwrap();
        assert_eq!(native, "original changed");
        assert_eq!(native.as_ptr(), allocation);

        arguments[2] = "wrong".into();
        assert!(callable
            .call_places_checked(&mut arguments, &[0, 1], &[None, None, Some(integer)])
            .is_err());
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
        assert_eq!(other_invoked.load(Ordering::SeqCst), 0);
    }
}
