//! Module that defines public event handlers for [`Engine`].

use crate::func::SendSync;
use crate::{Dynamic, Engine, EvalContext, Position, RawrResultOf, VarDefInfo};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// Where `print!`/`println!` (standard output) and `eprint!`/`eprintln!`
/// (standard error) send their text.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Output {
    /// `print!` and `println!`.
    Stdout,
    /// `eprint!` and `eprintln!`.
    Stderr,
}

impl Engine {
    /// Provide a callback that will be invoked before each variable access.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(name: &str, index: usize, context: EvalContext) -> Result<Option<Dynamic>, Box<EvalAltResult>>`
    ///
    /// where:
    /// * `name`: name of the variable.
    /// * `index`: an offset from the bottom of the current [`Scope`][crate::Scope] that the
    ///   variable is supposed to reside. Offsets start from 1, with 1 meaning the last variable in
    ///   the current [`Scope`][crate::Scope].  Essentially the correct variable is at position
    ///   `scope.len() - index`. If `index` is zero, then there is no pre-calculated offset position
    ///   and a search through the current [`Scope`][crate::Scope] must be performed.
    /// * `context`: the current [evaluation context][`EvalContext`].
    ///
    /// ## Return value
    ///
    /// * `Ok(None)`: continue with normal variable access.
    /// * `Ok(Some(Dynamic))`: the variable's value.
    ///
    /// ## Raising errors
    ///
    /// Return `Err(...)` if there is an error.
    ///
    /// Typed scripts can't read a name they don't declare yet, so the callback only sees
    /// variables that the script or its [`Scope`][crate::Scope] declares.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::{Engine, Scope};
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Register a variable resolver.
    /// engine.on_var(|name, _, _| {
    ///     match name {
    ///         "MYSTIC_NUMBER" => Ok(Some(42_i64.into())),
    ///         _ => Ok(None)
    ///     }
    /// });
    ///
    /// let mut scope = Scope::new();
    /// scope.push("MYSTIC_NUMBER", 0_i64);
    ///
    /// assert_eq!(engine.eval_with_scope::<i64>(&mut scope, "MYSTIC_NUMBER")?, 42);
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[inline(always)]
    pub fn on_var(
        &mut self,
        callback: impl Fn(&str, usize, EvalContext) -> RawrResultOf<Option<Dynamic>>
            + SendSync
            + 'static,
    ) -> &mut Self {
        self.resolve_var = Some(Box::new(callback));
        self
    }
    /// Provide a callback that will be invoked before the definition of each variable .
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(is_runtime: bool, info: VarInfo, context: EvalContext) -> Result<bool, Box<EvalAltResult>>`
    ///
    /// where:
    /// * `is_runtime`: `true` if the variable definition event happens during runtime, `false` if during compilation.
    /// * `info`: information on the variable.
    /// * `context`: the current [evaluation context][`EvalContext`].
    ///
    /// ## Return value
    ///
    /// * `Ok(true)`: continue with normal variable definition.
    /// * `Ok(false)`: deny the variable definition with an [runtime error][crate::EvalAltResult::ErrorRuntime].
    ///
    /// ## Raising errors
    ///
    /// Return `Err(...)` if there is an error.
    ///
    /// In a typed script an immutable `let` counts as a constant definition.
    /// A `const` item is not a variable definition, so the callback doesn't see it.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::Engine;
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Register a variable definition filter.
    /// engine.on_def_var(|_, info, _| {
    ///     // Disallow defining `mystic_number` as a constant
    ///     if info.name() == "mystic_number" && info.is_const() {
    ///         Ok(false)
    ///     } else {
    ///         Ok(true)
    ///     }
    /// });
    ///
    /// // The following runs fine:
    /// engine.run("let mut mystic_number = 42;")?;
    ///
    /// // The following causes an error:
    /// assert!(engine.run("let mystic_number = 42;").is_err());
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[inline(always)]
    pub fn on_def_var(
        &mut self,
        callback: impl Fn(bool, VarDefInfo, EvalContext) -> RawrResultOf<bool> + SendSync + 'static,
    ) -> &mut Self {
        self.def_var_filter = Some(Box::new(callback));
        self
    }
    /// _(internals)_ Register a callback that will be invoked during parsing to remap certain tokens.
    /// Exported under the `internals` feature only.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(token: Token, pos: Position, state: &TokenizeState) -> Token`
    ///
    /// where:
    /// * [`token`][crate::tokenizer::Token]: current token parsed
    /// * [`pos`][`Position`]: location of the token
    /// * [`state`][crate::tokenizer::TokenizeState]: current state of the tokenizer
    ///
    /// ## Raising errors
    ///
    /// It is possible to raise a parsing error by returning
    /// [`Token::LexError`][crate::tokenizer::Token::LexError] as the mapped token.
    ///
    /// Typed scripts are checked before their tokens are mapped, so a mapper can't add syntax to them yet.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::{Engine, Token};
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Register a token mapper.
    /// # #[allow(deprecated)]
    /// engine.on_parse_token(|token, _, _| {
    ///     match token {
    ///         // Convert all integer literals to strings
    ///         Token::IntegerConstant(n) => Token::StringConstant(Box::new(n.to_string().into())),
    ///         // Convert 'begin' .. 'end' to '{' .. '}'
    ///         Token::Identifier(s) if &*s == "begin" => Token::LeftBrace,
    ///         Token::Identifier(s) if &*s == "end" => Token::RightBrace,
    ///         // Pass through all other tokens unchanged
    ///         _ => token
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[cfg(feature = "internals")]
    #[inline(always)]
    pub fn on_parse_token(
        &mut self,
        callback: impl Fn(
                crate::tokenizer::Token,
                Position,
                &crate::tokenizer::TokenizeState,
            ) -> crate::tokenizer::Token
            + SendSync
            + 'static,
    ) -> &mut Self {
        self.token_mapper = Some(Box::new(callback));
        self
    }
    /// Register a callback for script evaluation progress.
    ///
    /// Not available under `unchecked`.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(counter: u64) -> Option<Dynamic>`
    ///
    /// ## Return value
    ///
    /// * `None`: continue running the script.
    /// * `Some(Dynamic)`: terminate the script with the specified exception value.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # use std::sync::RwLock;
    /// # use std::sync::Arc;
    /// use rawr::Engine;
    ///
    /// let result = Arc::new(RwLock::new(0_u64));
    /// let logger = result.clone();
    ///
    /// let mut engine = Engine::new();
    ///
    /// engine.on_progress(move |ops| {
    ///     if ops > 1000 {
    ///         Some("Over 1,000 operations!".into())
    ///     } else if ops % 123 == 0 {
    ///         *logger.write().unwrap() = ops;
    ///         None
    ///     } else {
    ///         None
    ///     }
    /// });
    ///
    /// engine.run("let mut sum = 0; for x in 0..5000 { sum += x; }")
    ///       .expect_err("should error");
    ///
    /// let last = *result.read().unwrap();
    /// assert!(last > 0 && last <= 1000 && last % 123 == 0);
    ///
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn on_progress(
        &mut self,
        callback: impl Fn(u64) -> Option<Dynamic> + SendSync + 'static,
    ) -> &mut Self {
        self.progress = Some(Box::new(callback));
        self
    }
    /// Override default action of `print` (print to stdout using [`println!`])
    ///
    /// Typed scripts can't call `print` yet. Their `print!` and `println!` text goes to
    /// [`on_output`][Engine::on_output] instead.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::sync::RwLock;
    /// # use std::sync::Arc;
    /// use rawr::Engine;
    ///
    /// let result = Arc::new(RwLock::new(String::new()));
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Override action of 'print' function
    /// let logger = result.clone();
    /// engine.on_print(move |s| logger.write().unwrap().push_str(s));
    /// ```
    #[inline(always)]
    pub fn on_print(&mut self, callback: impl Fn(&str) + SendSync + 'static) -> &mut Self {
        self.print = Some(Box::new(callback));
        self
    }
    /// Override default action of `print!`, `println!`, `eprint!` and
    /// `eprintln!` (write to stdout or stderr). The text is exactly what the
    /// macro writes, its newline included for `println!` and `eprintln!`.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # use std::sync::{Arc, RwLock};
    /// use rawr::{Engine, Output};
    ///
    /// let result = Arc::new(RwLock::new(String::new()));
    ///
    /// let mut engine = Engine::new();
    ///
    /// let logger = result.clone();
    /// engine.on_output(move |output, text| {
    ///     if output == Output::Stdout {
    ///         logger.write().unwrap().push_str(text);
    ///     }
    /// });
    ///
    /// engine.eval::<()>(r#"fn main() { let n = 42; print!("n = "); println!("{n}"); } main()"#)?;
    ///
    /// assert_eq!(*result.read().unwrap(), "n = 42\n");
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn on_output(&mut self, callback: impl Fn(Output, &str) + SendSync + 'static) -> &mut Self {
        self.output = Some(Box::new(callback));
        self
    }
    /// Override default action of `debug` (print to stdout using [`println!`])
    ///
    /// # Callback Function Signature
    ///
    /// The callback function signature passed takes the following form:
    ///
    /// `Fn(text: &str, source: Option<&str>, pos: Position)`
    ///
    /// where:
    /// * `text`: the text to display
    /// * `source`: current source, if any
    /// * [`pos`][`Position`]: location of the `debug` call
    ///
    /// Typed scripts can't call `debug` yet. Their `eprint!` and `eprintln!` text goes to
    /// [`on_output`][Engine::on_output] instead.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::sync::RwLock;
    /// # use std::sync::Arc;
    /// use rawr::Engine;
    ///
    /// let result = Arc::new(RwLock::new(String::new()));
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Override action of 'debug' function
    /// let logger = result.clone();
    /// engine.on_debug(move |s, src, pos| logger.write().unwrap().push_str(
    ///                     &format!("{} @ {:?} > {}", src.unwrap_or("unknown"), pos, s)
    ///                ));
    /// ```
    #[inline(always)]
    pub fn on_debug(
        &mut self,
        callback: impl Fn(&str, Option<&str>, Position) + SendSync + 'static,
    ) -> &mut Self {
        self.debug = Some(Box::new(callback));
        self
    }
    /// _(internals)_ Register a callback for access to [`Map`][crate::Map] properties that do not exist.
    /// Exported under the `internals` feature only.
    ///
    /// Not available under `no_index`.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(array: &mut Array, index: INT) -> Result<Target, Box<EvalAltResult>>`
    ///
    /// where:
    /// * `array`: mutable reference to the [`Array`][crate::Array] instance.
    /// * `index`: numeric index of the array access.
    ///
    /// ## Return value
    ///
    /// * `Ok(Target)`: [`Target`][crate::Target] of the indexing access.
    ///
    /// ## Raising errors
    ///
    /// Return `Err(...)` if there is an error, usually
    /// [`EvalAltResult::ErrorPropertyNotFound`][crate::EvalAltResult::ErrorPropertyNotFound].
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # use rawr::{Engine, Dynamic, EvalAltResult, Position};
    /// # use std::convert::TryInto;
    /// let mut engine = Engine::new();
    ///
    /// engine.on_invalid_array_index(|arr, index, _| match index
    /// {
    ///     3 => {
    ///         // The array can be modified in place
    ///         arr.push((42_i64).into());
    ///         // Return a mutable reference to an element
    ///         let value_ref = arr.last_mut().unwrap();
    ///         value_ref.try_into()
    ///     }
    ///     100 => {
    ///         let value = Dynamic::from(100_i64);
    ///         // Return a temporary value (not a reference)
    ///         Ok(value.into())
    ///     }
    ///     // Return the standard out-of-bounds error
    ///     _ => Err(EvalAltResult::ErrorArrayBounds(
    ///                 arr.len(), index, Position::NONE
    ///          ).into()),
    /// });
    ///
    /// let r = engine.eval::<i64>("
    ///             let mut a = vec![1, 2, 3];
    ///             a[3] += 1;
    ///             a[3] + a[100]
    ///         ")?;
    ///
    /// assert_eq!(r, 143);
    /// # Ok(()) }
    /// ```
    #[cfg(not(feature = "no_index"))]
    #[cfg(feature = "internals")]
    #[inline(always)]
    pub fn on_invalid_array_index(
        &mut self,
        callback: impl for<'a> Fn(
                &'a mut crate::Array,
                crate::INT,
                EvalContext,
            ) -> RawrResultOf<crate::Target<'a>>
            + SendSync
            + 'static,
    ) -> &mut Self {
        self.invalid_array_index = Some(Box::new(callback));
        self
    }
    /// _(internals)_ Register a callback for access to [`Map`][crate::Map] properties that do not exist.
    /// Exported under the `internals` feature only.
    ///
    /// Not available under `no_object`.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(map: &mut Map, prop: &str) -> Result<Target, Box<EvalAltResult>>`
    ///
    /// where:
    /// * `map`: mutable reference to the [`Map`][crate::Map] instance.
    /// * `prop`: name of the property that does not exist.
    ///
    /// ## Return value
    ///
    /// * `Ok(Target)`: [`Target`][crate::Target] of the property access.
    ///
    /// ## Raising errors
    ///
    /// Return `Err(...)` if there is an error, usually [`EvalAltResult::ErrorPropertyNotFound`][crate::EvalAltResult::ErrorPropertyNotFound].
    ///
    /// Typed scripts can't use object maps yet, so they don't reach this callback.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # use rawr::{Engine, Dynamic, EvalAltResult, Position};
    /// # use std::convert::TryInto;
    /// let mut engine = Engine::new();
    ///
    /// engine.on_map_missing_property(|map, prop, _| match prop
    /// {
    ///     "x" => {
    ///         // The object-map can be modified in place
    ///         map.insert("y".into(), (42_i64).into());
    ///         // Return a mutable reference to an element
    ///         let value_ref = map.get_mut("y").unwrap();
    ///         value_ref.try_into()
    ///     }
    ///     "z" => {
    ///         // Return a temporary value (not a reference)
    ///         let value = Dynamic::from(100_i64);
    ///         Ok(value.into())
    ///     }
    ///     // Return the standard property-not-found error
    ///     _ => Err(EvalAltResult::ErrorPropertyNotFound(
    ///                 prop.to_string(), Position::NONE
    ///          ).into()),
    /// });
    /// # Ok(()) }
    /// ```
    #[cfg(not(feature = "no_object"))]
    #[cfg(feature = "internals")]
    #[inline(always)]
    pub fn on_map_missing_property(
        &mut self,
        callback: impl for<'a> Fn(&'a mut crate::Map, &str, EvalContext) -> RawrResultOf<crate::Target<'a>>
            + SendSync
            + 'static,
    ) -> &mut Self {
        self.missing_map_property = Some(Box::new(callback));
        self
    }
    /// _(internals)_ Register a callback for when a function call is not found.
    /// Exported under the `internals` feature only.
    ///
    /// Not triggered for qualified function calls (e.g. `module::function()`).
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # Callback Function Signature
    ///
    /// `Fn(name: &str, args: &mut [&mut Dynamic], is_method_call: bool, context: EvalContext) -> RawrResultOf<Option<Dynamic>>`
    ///
    /// where:
    /// * `name`: name of the function being called.
    /// * `args`: mutable reference to the argument list. For method calls, the first element is the object.
    /// * `is_method_call`: `true` if this is a method-style call (`obj.method()`), `false` for a function-style call (`func()`).
    /// * `context`: the current evaluation context.
    ///
    /// ## Return value
    ///
    /// * `Ok(Some(value))`: provide a return value as the result of the function call.
    /// * `Ok(None)`: fall through to the standard [`EvalAltResult::ErrorFunctionNotFound`][crate::EvalAltResult::ErrorFunctionNotFound].
    ///
    /// ## Raising errors
    ///
    /// Return `Err(...)` if there is an error.
    ///
    /// Typed scripts can't call a function that is neither declared nor registered yet,
    /// so they don't reach this callback.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # use rawr::{Engine, Dynamic};
    /// let mut engine = Engine::new();
    ///
    /// #[allow(deprecated)]
    /// engine.on_missing_function(|name, args, _is_method_call, _ctx| {
    ///     if name == "greet" {
    ///         Ok(Some(Dynamic::from("hello")))
    ///     } else {
    ///         Ok(None) // fall through to standard error
    ///     }
    /// });
    /// # Ok(()) }
    /// ```
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[cfg(feature = "internals")]
    #[inline(always)]
    pub fn on_missing_function(
        &mut self,
        callback: impl Fn(&str, &mut [&mut Dynamic], bool, EvalContext) -> RawrResultOf<Option<Dynamic>>
            + SendSync
            + 'static,
    ) -> &mut Self {
        self.missing_function = Some(Box::new(callback));
        self
    }
    /// _(debugging)_ Register a callback for debugging.
    /// Exported under the `debugging` feature only.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[cfg(feature = "debugging")]
    #[inline(always)]
    pub fn register_debugger(
        &mut self,
        init: impl Fn(&Self, crate::debugger::Debugger) -> crate::debugger::Debugger
            + SendSync
            + 'static,
        callback: impl Fn(
                EvalContext,
                crate::eval::DebuggerEvent,
                crate::ast::ASTNode,
                Option<&str>,
                Position,
            ) -> RawrResultOf<crate::eval::DebuggerCommand>
            + SendSync
            + 'static,
    ) -> &mut Self {
        self.debugger_interface = Some((Box::new(init), Box::new(callback)));
        self
    }
}
