//! Abort a native callback-bearing operation at the first interpreter error.
//! Native collection cleanup runs during unwinding; the original interpreter
//! error is returned after it. A real native panic is propagated unchanged.

use crate::RawrResultOf;
use std::cell::RefCell;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

struct Context;

impl crate::scope::Family for Context {
    type Of<'a> = crate::NativeCallContext<'a>;
}

thread_local! {
    static CONTEXTS: RefCell<Vec<crate::scope::Nonce>> = const { RefCell::new(Vec::new()) };
}

struct End;

impl Drop for End {
    fn drop(&mut self) {
        CONTEXTS.with_borrow_mut(Vec::pop);
    }
}

pub(super) fn within<R>(context: &crate::NativeCallContext<'_>, body: impl FnOnce() -> R) -> R {
    crate::scope::lend_ref::<Context, _>(context, |nonce| {
        CONTEXTS.with_borrow_mut(|contexts| contexts.push(nonce));
        let _end = End;
        body()
    })
}

pub(super) enum Callable {
    Function(crate::FnPtr, crate::scope::Nonce),
    Crossed(crate::future::Callable),
}

impl Callable {
    pub(super) fn new(value: crate::Dynamic) -> RawrResultOf<Self> {
        if let Some(function) = value.clone().try_cast::<crate::FnPtr>() {
            let context = CONTEXTS
                .with_borrow(|contexts| contexts.last().copied())
                .ok_or_else(|| {
                    super::failure("a native sequence callback has no script context")
                })?;
            return Ok(Self::Function(function, context));
        }
        crate::future::Callable::new(value).map(Self::Crossed)
    }

    pub(super) fn call(&self, arguments: &mut [crate::Dynamic]) -> crate::RawrResult {
        match self {
            Self::Function(function, context) => {
                crate::scope::with_ref::<Context, _>(*context, |context| {
                    function.call_raw(context, None, arguments)
                })
                .map_err(|error| super::failure(error.to_string()))?
            }
            Self::Crossed(callable) => callable.call(arguments),
        }
    }
}

struct Failed;

pub(super) struct Abort(RefCell<Option<Box<crate::EvalAltResult>>>);

impl Abort {
    pub(super) fn value<T>(&self, result: RawrResultOf<T>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => {
                *self.0.borrow_mut() = Some(error);
                resume_unwind(Box::new(Failed));
            }
        }
    }
}

pub(super) fn run<T>(body: impl FnOnce(&Abort) -> T) -> RawrResultOf<T> {
    let abort = Abort(RefCell::new(None));
    match catch_unwind(AssertUnwindSafe(|| body(&abort))) {
        Ok(value) => Ok(value),
        Err(payload) if payload.is::<Failed>() => match abort.0.into_inner() {
            Some(error) => Err(error),
            None => resume_unwind(payload),
        },
        Err(payload) => resume_unwind(payload),
    }
}
