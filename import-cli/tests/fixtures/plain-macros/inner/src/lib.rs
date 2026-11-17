//! Macros a dependency exports, which a program reaches only through
//! `outer`. Their `$crate` paths name items `outer` does not re-export,
//! one of them `#[doc(hidden)]`.

#[derive(Debug, Clone, PartialEq, Default)]
pub struct List {
    items: Vec<i64>,
}

impl List {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, item: i64) -> Self {
        self.items.push(item);
        self
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn sum(&self) -> i64 {
        self.items.iter().sum()
    }
}

#[doc(hidden)]
pub mod __private {
    pub fn double(value: i64) -> i64 {
        value * 2
    }
}

/// A list of the doubled values.
#[macro_export]
macro_rules! list {
    () => {
        $crate::List::new()
    };
    ($($x:expr),+ $(,)?) => {
        $crate::List::new()$(.with($crate::__private::double($x)))+
    };
}

/// The sum of the values, through a local the caller's names never meet.
#[macro_export]
macro_rules! total {
    ($($x:expr),*) => {{
        let mut sum = 0;
        $(sum += $x;)*
        sum
    }};
}

/// A literal times ten, or one value times another.
#[macro_export]
macro_rules! scaled {
    ($value:literal) => {
        $crate::scaled!($value, 10)
    };
    ($value:expr, $by:expr) => {
        $value * $by
    };
}

/// The literal as written, and its length: `stringify!` of a fragment.
#[macro_export]
macro_rules! written {
    ($value:literal) => {
        (stringify!($value), stringify!($value).len())
    };
}

/// A measure of a value, which a macro calls through its trait's path.
pub trait Measure {
    fn measure(&self) -> i64;
}

impl Measure for List {
    fn measure(&self) -> i64 {
        self.sum() * 10
    }
}

/// A counter type and its methods, declared where the call is, as a
/// bitflags-style crate declares a program's types.
#[macro_export]
macro_rules! counter {
    ($name:ident, $start:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $name(i64);

        impl $name {
            pub const START: Self = Self($start);

            pub fn next(self) -> Self {
                Self(self.0 + 1)
            }

            pub fn get(self) -> i64 {
                self.0
            }
        }

        impl $crate::Measure for $name {
            fn measure(&self) -> i64 {
                self.0 * 10
            }
        }
    };
}

/// The measure of a list, as `$crate::Measure::measure(&list)`.
#[macro_export]
macro_rules! measured {
    ($list:expr) => {
        $crate::Measure::measure(&$list)
    };
}
