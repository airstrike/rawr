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
