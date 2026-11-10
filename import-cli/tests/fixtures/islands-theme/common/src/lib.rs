//! A stand-in for inboard's `common`: the crate's text module re-exported,
//! a macro that names this crate's own paths, and extension traits on
//! functions.
pub use ui as core;
pub use ui::text;

pub mod widget {
    pub use ui::Space;
}

/// Space that fills both ways, as inboard's `common::space!`.
#[macro_export]
macro_rules! space {
    () => {
        $crate::widget::Space::new()
            .width($crate::core::Length::Fill)
            .height($crate::core::Length::Fill)
    };
}

/// Extension traits on functions, as inboard's `common::function`.
pub mod function {
    /// A binary function's first argument given ahead (`Fn(A, B) -> O`).
    pub trait Function<A, B, O> {
        fn with(self, prefix: A) -> impl Fn(B) -> O
        where
            A: Clone;
    }

    impl<F, A, B, O> Function<A, B, O> for F
    where
        F: Fn(A, B) -> O,
        Self: Sized,
        A: Clone,
    {
        fn with(self, prefix: A) -> impl Fn(B) -> O {
            move |result| self(prefix.clone(), result)
        }
    }

    /// A ternary function's last argument given ahead (`Fn(&T, B, C) ->
    /// O`), as a style function's `selected`.
    pub trait Ternary<T, B, C, O>: Sized {
        fn with(self, last: C) -> impl Fn(&T, B) -> O
        where
            C: Clone;
    }

    impl<F, T, B, C, O> Ternary<T, B, C, O> for F
    where
        F: Fn(&T, B, C) -> O,
    {
        fn with(self, last: C) -> impl Fn(&T, B) -> O
        where
            C: Clone,
        {
            move |t, b| self(t, b, last.clone())
        }
    }
}

/// A numeric conversion named as `From`'s, as num-traits' `NumCast` is:
/// with it in scope, `u64::from(..)` is ambiguous, so a runner names
/// `From` for itself.
pub mod cast {
    pub trait NumCast: Sized {
        fn from(value: u32) -> Option<Self>;
        fn to_u32(&self) -> Option<u32>;
    }

    impl NumCast for u64 {
        fn from(value: u32) -> Option<Self> {
            Some(value.into())
        }

        fn to_u32(&self) -> Option<u32> {
            u32::try_from(*self).ok()
        }
    }

    /// A count of the crate's own, which the trait converts too.
    #[derive(Debug, Clone, Copy)]
    pub struct Count(pub u64);

    impl NumCast for Count {
        fn from(value: u32) -> Option<Self> {
            Some(Self(value.into()))
        }

        fn to_u32(&self) -> Option<u32> {
            self.0.to_u32()
        }
    }
}
