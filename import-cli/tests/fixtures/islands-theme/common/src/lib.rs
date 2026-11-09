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
