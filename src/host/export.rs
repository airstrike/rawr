//! An app's own Rust items, exported to typed scripts.
//!
//! `#[rawr::export]` on a struct, on its impl block or on a free function
//! writes an [`Export`] implementation. [`Exports`] collects the exports
//! into one facade [`Definition`](host::facade::Definition) and the native
//! callbacks that implement it. [`Engine::export`] installs both, so a
//! script compiled by that engine names the items and `call_fn` runs them.

use crate::host::{self, facade, item, signature, type_ref::Type};
use crate::types::dynamic::Variant;
use crate::{capability, receiver, Dynamic, Engine, EvalAltResult, ImmutableString, Position};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A native callback: the receiver first, then the arguments.
pub type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;

/// An item `#[rawr::export]` makes available to scripts: a struct with its
/// pub fields and the methods of its exported impl block, or a free
/// function.
pub trait Export {
    /// Declare this item and its callbacks.
    fn export(exports: &mut Exports);
}

/// A Rust type that crosses into scripts by value.
pub trait Value: Sized + 'static {
    /// The script type, relative to the facade root.
    ///
    /// # Errors
    ///
    /// Returns an error when the type's name is not a valid path.
    fn ty() -> Result<Type, host::Error>;
    /// The script value.
    fn into_dynamic(self) -> Dynamic;
    /// The Rust value a script passed.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` holds another type.
    fn from_dynamic(value: &Dynamic) -> Result<Self, Box<EvalAltResult>>;
}

/// A type a parameter borrows: `&str` reads a `String`, and `&T` reads a
/// `T`.
pub trait Param {
    /// The owned value the parameter borrows from.
    type Owned: Value;
}

impl Param for str {
    type Owned = String;
}

impl<T: Value> Param for T {
    type Owned = T;
}

fn mismatch<T>(value: &Dynamic) -> Box<EvalAltResult> {
    EvalAltResult::ErrorMismatchDataType(
        std::any::type_name::<T>().into(),
        value.type_name().into(),
        Position::NONE,
    )
    .into()
}

macro_rules! scalar {
    ($($rust:ty => $builtin:ident),* $(,)?) => {$(
        impl Value for $rust {
            fn ty() -> Result<Type, host::Error> {
                Ok(Type::builtin(crate::host::type_ref::Builtin::$builtin))
            }
            fn into_dynamic(self) -> Dynamic {
                Dynamic::from(self)
            }
            fn from_dynamic(value: &Dynamic) -> Result<Self, Box<EvalAltResult>> {
                value.read_lock::<$rust>().map(|value| *value).ok_or_else(|| mismatch::<$rust>(value))
            }
        }
    )*};
}

scalar!(i64 => I64, f64 => F64, bool => Bool, char => Char);

impl Value for String {
    fn ty() -> Result<Type, host::Error> {
        Ok(Type::builtin(crate::host::type_ref::Builtin::String))
    }
    fn into_dynamic(self) -> Dynamic {
        Dynamic::from(ImmutableString::from(self))
    }
    fn from_dynamic(value: &Dynamic) -> Result<Self, Box<EvalAltResult>> {
        value
            .read_lock::<ImmutableString>()
            .map(|text| text.to_string())
            .ok_or_else(|| mismatch::<String>(value))
    }
}

impl Value for () {
    fn ty() -> Result<Type, host::Error> {
        Ok(Type::unit())
    }
    fn into_dynamic(self) -> Dynamic {
        Dynamic::UNIT
    }
    fn from_dynamic(value: &Dynamic) -> Result<Self, Box<EvalAltResult>> {
        value
            .read_lock::<()>()
            .map(|_| ())
            .ok_or_else(|| mismatch::<()>(value))
    }
}

impl<T: Value> Value for Option<T> {
    fn ty() -> Result<Type, host::Error> {
        Ok(Type::option(T::ty()?))
    }
    fn into_dynamic(self) -> Dynamic {
        match self {
            Some(value) => crate::option::some(value.into_dynamic()),
            None => crate::option::none(),
        }
    }
    fn from_dynamic(value: &Dynamic) -> Result<Self, Box<EvalAltResult>> {
        match crate::option::read(value).ok_or_else(|| mismatch::<Self>(value))? {
            Some(inner) => T::from_dynamic(&inner).map(Some),
            None => Ok(None),
        }
    }
}

/// One app's exported items, which [`Engine::export`] installs under one
/// root that scripts `use`.
pub struct Exports {
    builder: Result<facade::Builder, host::Error>,
    bindings: Vec<(String, Callback)>,
}

impl Exports {
    /// Begin the exports scripts reach as `root::Item`.
    #[must_use]
    pub fn new(root: &str) -> Self {
        Self {
            builder: facade::Builder::new(root, facade::Abi::new(1).expect("ABI 1 is valid")),
            bindings: Vec::new(),
        }
    }

    /// Add one item that `#[rawr::export]` marked.
    pub fn add<T: Export>(&mut self) -> &mut Self {
        if self.builder.is_ok() {
            T::export(self);
        }
        self
    }

    fn declare(&mut self, declare: impl FnOnce(&mut facade::Builder) -> Result<(), host::Error>) {
        if let Ok(builder) = &mut self.builder
            && let Err(error) = declare(builder)
        {
            self.builder = Err(error);
        }
    }

    fn signature(
        inputs: Vec<Result<Type, host::Error>>,
        output: Result<Type, host::Error>,
    ) -> Result<signature::Definition, host::Error> {
        let mut signature = signature::Builder::new();
        for input in inputs {
            signature.parameter(input?);
        }
        signature.output(output?);
        Ok(signature.finish())
    }

    /// Declare an exported struct, opaque to scripts but for its fields.
    #[doc(hidden)]
    pub fn type_<T: Variant + Clone>(&mut self, name: &str) -> &mut Self {
        self.declare(|builder| {
            builder
                .type_::<T>(
                    name,
                    &[],
                    &[capability::Kind::Clone, capability::Kind::Static],
                )
                .map(|_| ())
        });
        self
    }

    /// Declare a pub field. Its callback returns the field when it has only
    /// the receiver, and otherwise the receiver with the field set to the
    /// second argument, which is how a script assigns it.
    #[doc(hidden)]
    pub fn field(
        &mut self,
        path: &str,
        ty: Result<Type, host::Error>,
        owner: Result<Type, host::Error>,
        callback: Callback,
    ) -> &mut Self {
        self.method(
            path,
            receiver::Mode::Shared,
            Vec::new(),
            ty.clone(),
            callback,
        );
        self.method(path, receiver::Mode::Owned, vec![ty], owner, callback)
    }

    /// Declare a method with its receiver mode.
    #[doc(hidden)]
    pub fn method(
        &mut self,
        path: &str,
        mode: receiver::Mode,
        inputs: Vec<Result<Type, host::Error>>,
        output: Result<Type, host::Error>,
        callback: Callback,
    ) -> &mut Self {
        self.declare(|builder| {
            let signature = Self::signature(inputs, output)?;
            builder.item(item::Definition::method(path, mode, signature)?)?;
            Ok(())
        });
        self.bind(path, callback)
    }

    /// Declare an associated function, which has no receiver.
    #[doc(hidden)]
    pub fn associated_function(
        &mut self,
        path: &str,
        inputs: Vec<Result<Type, host::Error>>,
        output: Result<Type, host::Error>,
        callback: Callback,
    ) -> &mut Self {
        self.declare(|builder| {
            let signature = Self::signature(inputs, output)?;
            builder.item(item::Definition::associated_function(path, signature)?)?;
            Ok(())
        });
        self.bind(path, callback)
    }

    /// Declare a free function.
    #[doc(hidden)]
    pub fn function(
        &mut self,
        path: &str,
        inputs: Vec<Result<Type, host::Error>>,
        output: Result<Type, host::Error>,
        callback: Callback,
    ) -> &mut Self {
        self.declare(|builder| {
            let signature = Self::signature(inputs, output)?;
            builder.item(item::Definition::function(path, signature)?)?;
            Ok(())
        });
        self.bind(path, callback)
    }

    fn bind(&mut self, path: &str, callback: Callback) -> &mut Self {
        if self.bindings.iter().all(|(bound, _)| bound != path) {
            self.bindings.push((path.into(), callback));
        }
        self
    }
}

fn failed(error: impl std::fmt::Display) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(
        format!("cannot export to rawr: {error}").into(),
        Position::NONE,
    )
    .into()
}

impl Engine {
    /// Install an app's exported items, so scripts this engine compiles can
    /// `use` them and `call_fn` runs them.
    ///
    /// # Errors
    ///
    /// Returns an error when an export is invalid or its root is taken.
    /// Export everything before the first compile that calls an exported
    /// item: after it, the exports can't change.
    ///
    /// # Errors
    ///
    /// Also returns an error after such a compile.
    pub fn export(&mut self, exports: Exports) -> Result<&mut Self, Box<EvalAltResult>> {
        if self
            .exports_frozen
            .load(core::sync::atomic::Ordering::Acquire)
        {
            return Err(failed(
                "a script compiled by this engine already calls its exports; export everything before compiling",
            ));
        }
        let definition = exports
            .builder
            .and_then(facade::Builder::finish)
            .map_err(failed)?;
        let root = definition.root().to_string();
        let mut runtime = host::runtime::Builder::new(definition.clone());
        for (path, callback) in exports.bindings {
            // A field's reader and writer share their path, and one binding
            // serves both.
            runtime
                .bind(format!("{root}::{path}"), callback)
                .map_err(failed)?;
        }
        let registry = self
            .host_runtime
            .get_or_insert_with(|| crate::Shared::new(host::runtime::Registry::new()));
        let registry = crate::Shared::get_mut(registry)
            .ok_or_else(|| failed("a script is running on this engine"))?;
        self.host_registry.install(definition).map_err(failed)?;
        registry.install(runtime.finish()).map_err(failed)?;
        Ok(self)
    }
}

#[doc(hidden)]
pub mod __private {
    //! What `#[rawr::export]` expands to.

    use super::{mismatch, Exports};
    use crate::types::dynamic::Variant;
    use crate::{Dynamic, EvalAltResult};

    /// The methods of a type's exported impl block.
    pub trait Methods {
        fn methods(exports: &mut Exports);
    }

    /// Finds a type's [`Methods`] when it has an exported impl block.
    pub struct Probe<T>(core::marker::PhantomData<T>);

    impl<T> Probe<T> {
        #[must_use]
        pub const fn new() -> Self {
            Self(core::marker::PhantomData)
        }
    }

    impl<T> Default for Probe<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    pub trait ViaMethods {
        fn methods(&self, exports: &mut Exports);
    }

    impl<T: Methods> ViaMethods for Probe<T> {
        fn methods(&self, exports: &mut Exports) {
            T::methods(exports);
        }
    }

    pub trait NoMethods {
        fn methods(&self, exports: &mut Exports);
    }

    impl<T> NoMethods for &Probe<T> {
        fn methods(&self, _: &mut Exports) {}
    }

    /// Call `f` with the `&self` receiver.
    pub fn shared<T: Variant + Clone, R>(
        value: &Dynamic,
        f: impl FnOnce(&T) -> R,
    ) -> Result<R, Box<EvalAltResult>> {
        let this = value.read_lock::<T>().ok_or_else(|| mismatch::<T>(value))?;
        Ok(f(&this))
    }

    /// Call `f` with the `&mut self` receiver.
    pub fn mutable<T: Variant + Clone, R>(
        value: &mut Dynamic,
        f: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Box<EvalAltResult>> {
        if !value.is::<T>() {
            return Err(mismatch::<T>(value));
        }
        let mut this = value
            .write_lock::<T>()
            .ok_or_else(|| mismatch::<T>(&Dynamic::UNIT))?;
        Ok(f(&mut this))
    }

    /// The `self` receiver, which the call consumes.
    pub fn owned<T: Variant + Clone>(value: &mut Dynamic) -> Result<T, Box<EvalAltResult>> {
        if !value.is::<T>() {
            return Err(mismatch::<T>(value));
        }
        Ok(value.take().cast::<T>())
    }
}
