//! std's `sync::atomic` for Rawr code: the atomic integers and
//! `AtomicBool`, each std's own atomic, and the `Ordering` their operations
//! take. A Rawr value of an atomic is a handle to its one place, which every
//! `&self` operation changes, so a static, a local and a borrow of either
//! share the value as std's do.

use std::sync::atomic::{
    AtomicBool, AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicIsize, AtomicU16, AtomicU32,
    AtomicU64, AtomicU8, AtomicUsize, Ordering,
};
use std::sync::Arc;

use super::held::{self, Held};
use crate::host::{self, facade};
use crate::{capability, Dynamic, EvalAltResult, Position, INT};

/// The types the module holds.
pub(super) const HELD: [Held; 12] = [
    held::<AtomicBool>(),
    held::<AtomicI8>(),
    held::<AtomicI16>(),
    held::<AtomicI32>(),
    held::<AtomicI64>(),
    held::<AtomicIsize>(),
    held::<AtomicU8>(),
    held::<AtomicU16>(),
    held::<AtomicU32>(),
    held::<AtomicU64>(),
    held::<AtomicUsize>(),
    Held::Value(held::Value {
        std: "std::sync::atomic::Ordering",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .value_type::<Ordering>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: true,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Hash,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
];

/// The held atomic `A`, whose Rust value is an `Arc<A>`.
const fn held<A: Atomic>() -> Held {
    Held::Value(held::Value {
        std: A::STD,
        parameters: &[],
        lent: &[],
        also: &[],
        register: register::<A>,
        crosses: false,
        runs: &[
            capability::Kind::Default,
            capability::Kind::Debug,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    })
}

fn register<A: Atomic>(
    builder: &mut facade::Builder,
    path: &str,
    parameters: &[&str],
    capabilities: &[capability::Kind],
) -> Result<(), host::Error> {
    builder
        .default_type::<Arc<A>>(path, parameters, capabilities)
        .map(drop)
}

/// A primitive an atomic holds, as rawr's value of it.
trait Primitive: Copy {
    /// The primitive `value` holds, of a value of its type.
    fn read(value: &Dynamic) -> Option<Self>;

    /// Rawr's value of it.
    fn made(self) -> Dynamic;
}

impl Primitive for bool {
    fn read(value: &Dynamic) -> Option<Self> {
        value.as_bool().ok()
    }

    fn made(self) -> Dynamic {
        Dynamic::from_bool(self)
    }
}

/// Each integer is rawr's `INT` of its bits, as a `u64`'s past `i64::MAX`
/// is.
macro_rules! integer_primitives {
    ($($native:ty)*) => {
        $(impl Primitive for $native {
            fn read(value: &Dynamic) -> Option<Self> {
                value.as_int().ok().map(|value| value as $native)
            }

            fn made(self) -> Dynamic {
                Dynamic::from_int(self as INT)
            }
        })*
    };
}

integer_primitives!(i8 i16 i32 i64 isize u8 u16 u32 u64 usize);

/// An atomic's bitwise `fetch_` operation, which each atomic has.
#[derive(Clone, Copy)]
enum Bitwise {
    And,
    Nand,
    Or,
    Xor,
}

/// An atomic integer's arithmetic `fetch_` operation.
#[derive(Clone, Copy)]
enum Arithmetic {
    Add,
    Sub,
    Max,
    Min,
}

/// One of std's atomics: its public path, and the operations rawr runs of
/// it, each std's own.
trait Atomic: Default + std::fmt::Debug + Send + Sync + 'static {
    type Primitive: Primitive;

    /// Its public path: `std::sync::atomic::AtomicUsize`.
    const STD: &'static str;

    fn new(value: Self::Primitive) -> Self;

    fn load(&self, order: Ordering) -> Self::Primitive;

    fn store(&self, value: Self::Primitive, order: Ordering);

    fn swap(&self, value: Self::Primitive, order: Ordering) -> Self::Primitive;

    fn compare_exchange(
        &self,
        current: Self::Primitive,
        new: Self::Primitive,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Self::Primitive, Self::Primitive>;

    fn compare_exchange_weak(
        &self,
        current: Self::Primitive,
        new: Self::Primitive,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Self::Primitive, Self::Primitive>;

    fn bitwise(
        &self,
        operation: Bitwise,
        value: Self::Primitive,
        order: Ordering,
    ) -> Self::Primitive;
}

/// One of std's atomic integers, which has the arithmetic operations too.
trait Integer: Atomic {
    fn arithmetic(
        &self,
        operation: Arithmetic,
        value: Self::Primitive,
        order: Ordering,
    ) -> Self::Primitive;
}

/// Each atomic's operations are std's of the same name.
macro_rules! atomics {
    ($($atomic:ident: $primitive:ty,)*) => {
        $(impl Atomic for $atomic {
            type Primitive = $primitive;

            const STD: &'static str = concat!("std::sync::atomic::", stringify!($atomic));

            fn new(value: $primitive) -> Self {
                $atomic::new(value)
            }

            fn load(&self, order: Ordering) -> $primitive {
                $atomic::load(self, order)
            }

            fn store(&self, value: $primitive, order: Ordering) {
                $atomic::store(self, value, order);
            }

            fn swap(&self, value: $primitive, order: Ordering) -> $primitive {
                $atomic::swap(self, value, order)
            }

            fn compare_exchange(
                &self,
                current: $primitive,
                new: $primitive,
                success: Ordering,
                failure: Ordering,
            ) -> Result<$primitive, $primitive> {
                $atomic::compare_exchange(self, current, new, success, failure)
            }

            fn compare_exchange_weak(
                &self,
                current: $primitive,
                new: $primitive,
                success: Ordering,
                failure: Ordering,
            ) -> Result<$primitive, $primitive> {
                $atomic::compare_exchange_weak(self, current, new, success, failure)
            }

            fn bitwise(
                &self,
                operation: Bitwise,
                value: $primitive,
                order: Ordering,
            ) -> $primitive {
                match operation {
                    Bitwise::And => self.fetch_and(value, order),
                    Bitwise::Nand => self.fetch_nand(value, order),
                    Bitwise::Or => self.fetch_or(value, order),
                    Bitwise::Xor => self.fetch_xor(value, order),
                }
            }
        })*
    };
}

atomics! {
    AtomicBool: bool,
    AtomicI8: i8,
    AtomicI16: i16,
    AtomicI32: i32,
    AtomicI64: i64,
    AtomicIsize: isize,
    AtomicU8: u8,
    AtomicU16: u16,
    AtomicU32: u32,
    AtomicU64: u64,
    AtomicUsize: usize,
}

macro_rules! integers {
    ($($atomic:ident)*) => {
        $(impl Integer for $atomic {
            fn arithmetic(
                &self,
                operation: Arithmetic,
                value: Self::Primitive,
                order: Ordering,
            ) -> Self::Primitive {
                match operation {
                    Arithmetic::Add => self.fetch_add(value, order),
                    Arithmetic::Sub => self.fetch_sub(value, order),
                    Arithmetic::Max => self.fetch_max(value, order),
                    Arithmetic::Min => self.fetch_min(value, order),
                }
            }
        })*
    };
}

integers!(AtomicI8 AtomicI16 AtomicI32 AtomicI64 AtomicIsize AtomicU8 AtomicU16 AtomicU32 AtomicU64 AtomicUsize);

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Arc<AtomicBool>>();
    crate::host::debug::register::<Arc<AtomicI8>>();
    crate::host::debug::register::<Arc<AtomicI16>>();
    crate::host::debug::register::<Arc<AtomicI32>>();
    crate::host::debug::register::<Arc<AtomicI64>>();
    crate::host::debug::register::<Arc<AtomicIsize>>();
    crate::host::debug::register::<Arc<AtomicU8>>();
    crate::host::debug::register::<Arc<AtomicU16>>();
    crate::host::debug::register::<Arc<AtomicU32>>();
    crate::host::debug::register::<Arc<AtomicU64>>();
    crate::host::debug::register::<Arc<AtomicUsize>>();
    crate::host::debug::register::<Ordering>();
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

/// The atomic `value` is a handle to.
fn atomic<A: Atomic>(value: &Dynamic) -> Result<Arc<A>, Box<EvalAltResult>> {
    value
        .read_lock::<Arc<A>>()
        .map(|atomic| Arc::clone(&atomic))
        .ok_or_else(|| {
            error(format!(
                "expected `{}`, found `{}`",
                A::STD,
                value.type_name()
            ))
        })
}

fn primitive<A: Atomic>(value: &Dynamic) -> Result<A::Primitive, Box<EvalAltResult>> {
    A::Primitive::read(value).ok_or_else(|| {
        error(format!(
            "expected the value of a `{}`, found `{}`",
            A::STD,
            value.type_name()
        ))
    })
}

fn ordering(value: &Dynamic) -> Result<Ordering, Box<EvalAltResult>> {
    value
        .read_lock::<Ordering>()
        .map(|order| *order)
        .ok_or_else(|| {
            error(format!(
                "expected `std::sync::atomic::Ordering`, found `{}`",
                value.type_name()
            ))
        })
}

/// The `Result` a `compare_exchange` gives.
fn exchanged<P: Primitive>(result: Result<P, P>) -> Dynamic {
    match result {
        Ok(previous) => crate::result::ok(previous.made()),
        Err(current) => crate::result::err(current.made()),
    }
}

type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;

/// Bind the operations of the atomic `A`, at its facade path.
fn bind_atomic<A: Atomic>(binds: &mut impl super::Bind) {
    let path = held::Value::facade(A::STD);
    let callbacks: [(&str, Callback); 9] = [
        ("new", |args| {
            Ok(Dynamic::from(Arc::new(A::new(primitive::<A>(args[0])?))))
        }),
        ("default", |_| Ok(Dynamic::from(Arc::new(A::default())))),
        // `From<usize>`, and the `From<T> for T` every type has.
        ("from", |args| {
            if args[0].is::<Arc<A>>() {
                return Ok(args[0].clone());
            }
            Ok(Dynamic::from(Arc::new(A::new(primitive::<A>(args[0])?))))
        }),
        ("load", |args| {
            Ok(atomic::<A>(args[0])?.load(ordering(args[1])?).made())
        }),
        ("into_inner", |args| {
            Ok(atomic::<A>(args[0])?.load(Ordering::SeqCst).made())
        }),
        ("store", |args| {
            atomic::<A>(args[0])?.store(primitive::<A>(args[1])?, ordering(args[2])?);
            Ok(Dynamic::UNIT)
        }),
        ("swap", |args| {
            let previous = atomic::<A>(args[0])?.swap(primitive::<A>(args[1])?, ordering(args[2])?);
            Ok(previous.made())
        }),
        ("compare_exchange", |args| {
            Ok(exchanged(atomic::<A>(args[0])?.compare_exchange(
                primitive::<A>(args[1])?,
                primitive::<A>(args[2])?,
                ordering(args[3])?,
                ordering(args[4])?,
            )))
        }),
        ("compare_exchange_weak", |args| {
            Ok(exchanged(atomic::<A>(args[0])?.compare_exchange_weak(
                primitive::<A>(args[1])?,
                primitive::<A>(args[2])?,
                ordering(args[3])?,
                ordering(args[4])?,
            )))
        }),
    ];
    for (name, callback) in callbacks {
        binds.plain(&format!("{path}::{name}"), callback);
    }
    let bitwise: [(&str, Callback); 4] = [
        ("fetch_and", |args| fetch_bitwise::<A>(args, Bitwise::And)),
        ("fetch_nand", |args| fetch_bitwise::<A>(args, Bitwise::Nand)),
        ("fetch_or", |args| fetch_bitwise::<A>(args, Bitwise::Or)),
        ("fetch_xor", |args| fetch_bitwise::<A>(args, Bitwise::Xor)),
    ];
    for (name, callback) in bitwise {
        binds.plain(&format!("{path}::{name}"), callback);
    }
}

fn fetch_bitwise<A: Atomic>(
    args: &mut [&mut Dynamic],
    operation: Bitwise,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let previous =
        atomic::<A>(args[0])?.bitwise(operation, primitive::<A>(args[1])?, ordering(args[2])?);
    Ok(previous.made())
}

fn fetch_arithmetic<A: Integer>(
    args: &mut [&mut Dynamic],
    operation: Arithmetic,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let previous =
        atomic::<A>(args[0])?.arithmetic(operation, primitive::<A>(args[1])?, ordering(args[2])?);
    Ok(previous.made())
}

/// Bind the operations of the atomic integer `A`.
fn bind_integer<A: Integer>(binds: &mut impl super::Bind) {
    bind_atomic::<A>(binds);
    let path = held::Value::facade(A::STD);
    let arithmetic: [(&str, Callback); 4] = [
        ("fetch_add", |args| {
            fetch_arithmetic::<A>(args, Arithmetic::Add)
        }),
        ("fetch_sub", |args| {
            fetch_arithmetic::<A>(args, Arithmetic::Sub)
        }),
        ("fetch_max", |args| {
            fetch_arithmetic::<A>(args, Arithmetic::Max)
        }),
        ("fetch_min", |args| {
            fetch_arithmetic::<A>(args, Arithmetic::Min)
        }),
    ];
    for (name, callback) in arithmetic {
        binds.plain(&format!("{path}::{name}"), callback);
    }
}

/// Bind the atomics' operations and `Ordering`'s variants.
pub(super) fn bind(binds: &mut impl super::Bind) {
    bind_atomic::<AtomicBool>(binds);
    binds.plain("sync::atomic::AtomicBool::fetch_not", |args| {
        let previous = atomic::<AtomicBool>(args[0])?.fetch_not(ordering(args[1])?);
        Ok(Dynamic::from_bool(previous))
    });
    bind_integer::<AtomicI8>(binds);
    bind_integer::<AtomicI16>(binds);
    bind_integer::<AtomicI32>(binds);
    bind_integer::<AtomicI64>(binds);
    bind_integer::<AtomicIsize>(binds);
    bind_integer::<AtomicU8>(binds);
    bind_integer::<AtomicU16>(binds);
    bind_integer::<AtomicU32>(binds);
    bind_integer::<AtomicU64>(binds);
    bind_integer::<AtomicUsize>(binds);
    macro_rules! orderings {
        ($($name:ident)*) => {
            $(binds.plain(
                concat!("sync::atomic::Ordering::", stringify!($name)),
                |_| Ok(Dynamic::from(Ordering::$name)),
            );)*
        };
    }
    orderings!(Relaxed Release Acquire AcqRel SeqCst);
}
