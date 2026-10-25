//! A script an app loads, and typed handles to its functions (D514).
//!
//! [`Engine::load`] and [`Engine::script`] compile a script into a
//! [`Script`], which keeps the engine, the compiled program and the typed
//! signature of each of its free functions. [`Script::func`] looks a
//! function up once and checks its signature against the handle's types.
//! [`Script::reload`] rereads the file and swaps every handle to the new
//! program together, or keeps the old one when the new one fails.
#![cfg(not(feature = "no_function"))]
#![cfg(not(feature = "no_std"))]

use crate::ast::ScriptFuncDef;
use crate::diagnostic::{self, code, Report};
use crate::eval::Caches;
use crate::func::{locked_read, locked_write, Locked};
use crate::host::export::Value;
use crate::program::signatures::{self, Signatures, Ty};
use crate::types::{ParseErrorType, Rendered};
use crate::{
    host, resolve, CallFnOptions, Dynamic, Engine, EvalAltResult, ImmutableString, Position,
    RawrResult, Scope, Shared, AST, ERR,
};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

#[cfg(not(feature = "sync"))]
type Weak<T> = std::rc::Weak<T>;
#[cfg(feature = "sync")]
type Weak<T> = std::sync::Weak<T>;

/// A compiled script, whose functions an app calls through typed handles.
///
/// ```
/// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
/// use rawr::Engine;
///
/// let script = Engine::new().script("fn price(subtotal: f64, items: i64) -> f64 {
///     subtotal + 4.99 * items as f64
/// }")?;
/// let price = script.func::<(f64, i64), f64>("price")?;
/// assert_eq!(price.call(100.0, 2)?, 109.98);
/// # Ok(())
/// # }
/// ```
pub struct Script {
    inner: Shared<Inner>,
}

struct Inner {
    engine: Shared<Engine>,
    /// The file [`Script::reload`] rereads, when the script came from one.
    path: Option<PathBuf>,
    /// The generation every handle calls. A reload swaps it whole.
    current: Locked<Shared<Generation>>,
    /// The handles looked up so far, by index. A reload checks the live
    /// ones against the new program.
    wants: Locked<Vec<Weak<Want>>>,
}

/// One compile of the script.
struct Program {
    ast: AST,
    signatures: Signatures,
    source: diagnostic::Source,
}

/// A program and the function each handle calls in it, by handle index.
struct Generation {
    program: Shared<Program>,
    functions: Box<[Option<Shared<ScriptFuncDef>>]>,
}

/// What a handle needs from the script.
struct Want {
    index: usize,
    name: ImmutableString,
    parameters: Box<[Ty]>,
    output: Ty,
}

/// A typed handle to one of a [`Script`]'s functions, which
/// [`Script::func`] returns.
///
/// `A` is a tuple of the parameter types and `R` the return type. The
/// handle was checked against the script's signature when it was looked
/// up, so [`call`](Func::call) takes its arguments as plain Rust values.
pub struct Func<A, R> {
    inner: Shared<Inner>,
    want: Shared<Want>,
    types: PhantomData<fn(A) -> R>,
}

impl<A, R> Clone for Func<A, R> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            want: self.want.clone(),
            types: PhantomData,
        }
    }
}

impl<A, R> std::fmt::Debug for Func<A, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&signatures::function_type(
            &self.want.parameters,
            &self.want.output,
        ))
    }
}

impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Script")
            .field("path", &self.inner.path)
            .finish_non_exhaustive()
    }
}

mod sealed {
    pub trait Sealed {}
}

/// The parameter types of a [`Func`]: a tuple of up to eight
/// [`Value`]s.
pub trait Parameters: sealed::Sealed {
    #[doc(hidden)]
    fn types() -> Result<Vec<host::type_ref::Type>, host::Error>;
}

macro_rules! arity {
    ($($ty:ident $value:ident),*) => {
        impl<$($ty: Value),*> sealed::Sealed for ($($ty,)*) {}

        impl<$($ty: Value),*> Parameters for ($($ty,)*) {
            fn types() -> Result<Vec<host::type_ref::Type>, host::Error> {
                Ok(vec![$($ty::ty()?),*])
            }
        }

        impl<$($ty: Value,)* R: Value> Func<($($ty,)*), R> {
            /// Call the script's function. It runs the newest program
            /// [`Script::reload`] published.
            ///
            /// # Errors
            ///
            /// Returns the script's runtime error, such as a panic or a
            /// limit it reached.
            #[allow(clippy::too_many_arguments)]
            pub fn call(&self, $($value: $ty),*) -> Result<R, Box<EvalAltResult>> {
                let arguments: &mut [Dynamic] = &mut [$($value.into_dynamic()),*];
                R::from_dynamic(&self.invoke(arguments)?)
            }
        }
    };
}

arity!();
arity!(A a);
arity!(A a, B b);
arity!(A a, B b, C c);
arity!(A a, B b, C c, D d);
arity!(A a, B b, C c, D d, E e);
arity!(A a, B b, C c, D d, E e, F f);
arity!(A a, B b, C c, D d, E e, F f, G g);
arity!(A a, B b, C c, D d, E e, F f, G g, H h);

impl Engine {
    /// Compile the script at `path` into a [`Script`], whose functions the
    /// app calls through typed handles.
    ///
    /// A path the app names relative to its working directory reads as
    /// rustc names source files in its errors.
    ///
    /// # Errors
    ///
    /// Returns an error when the file can't be read, or the script's
    /// compile error, printed as rustc prints it.
    pub fn load(self, path: impl AsRef<Path>) -> Result<Script, Box<EvalAltResult>> {
        let path = path.as_ref();
        let text = read(path)?;
        let program = compile(&self, text, &path.to_string_lossy())?;
        Ok(Script::new(self, Some(path.to_path_buf()), program))
    }

    /// Compile `text` into a [`Script`]. [`Script::reload`] has no file to
    /// reread for it.
    ///
    /// # Errors
    ///
    /// Returns the script's compile error, printed as rustc prints it.
    pub fn script(self, text: impl Into<String>) -> Result<Script, Box<EvalAltResult>> {
        let program = compile(&self, text.into(), "<script>")?;
        Ok(Script::new(self, None, program))
    }

    /// Call a function a handle resolved, without the script's top-level
    /// statements.
    fn call_resolved(
        &self,
        ast: &AST,
        function: &ScriptFuncDef,
        arguments: &mut [Dynamic],
    ) -> RawrResult {
        self.call_fn_in(
            CallFnOptions::new().eval_ast(false),
            &mut Scope::new(),
            ast,
            &function.name,
            Some(function),
            arguments,
            &mut self.new_global_runtime_state(),
            &mut Caches::new(),
        )
    }
}

fn read(path: &Path) -> Result<String, Box<EvalAltResult>> {
    std::fs::read_to_string(path).map_err(|error| {
        ERR::ErrorSystem(
            format!("Cannot read script file '{}'", path.to_string_lossy()),
            error.into(),
        )
        .into()
    })
}

fn compile(engine: &Engine, text: String, identity: &str) -> Result<Program, Box<EvalAltResult>> {
    let (mut ast, signatures) = engine.compile_script(&text, identity)?;
    if identity != "<script>" {
        ast.set_source(identity);
    }
    let source = diagnostic::Source::new(identity, text)
        .map_err(|error| Box::new(ERR::ErrorRuntime(error.to_string().into(), Position::NONE)))?;
    Ok(Program {
        ast,
        signatures,
        source,
    })
}

impl Script {
    fn new(engine: Engine, path: Option<PathBuf>, program: Program) -> Self {
        Self {
            inner: Shared::new(Inner {
                engine: Shared::new(engine),
                path,
                current: Locked::new(Shared::new(Generation {
                    program: Shared::new(program),
                    functions: Box::default(),
                })),
                wants: Locked::new(Vec::new()),
            }),
        }
    }

    /// The engine that compiled this script.
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.inner.engine
    }

    /// The file this script was loaded from, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    /// A typed handle to the script's function `name`, which takes the
    /// parameters `A`, a tuple, and returns `R`.
    ///
    /// The handle is checked here, once, against the function's signature
    /// in the script. A parameter the script borrows (`&str`, `&T`) takes
    /// the handle's owned `String` or `T`.
    ///
    /// # Errors
    ///
    /// Returns rustc's E0425 when the script has no function `name`, and
    /// E0308 when its signature differs from the handle's, both printed as
    /// rustc prints them.
    pub fn func<A: Parameters, R: Value>(
        &self,
        name: &str,
    ) -> Result<Func<A, R>, Box<EvalAltResult>> {
        let invalid = |error: host::Error| -> Box<EvalAltResult> {
            ERR::ErrorRuntime(error.to_string().into(), Position::NONE).into()
        };
        let parameters = A::types()
            .map_err(invalid)?
            .iter()
            .map(Ty::from_host)
            .collect();
        let output = Ty::from_host(&R::ty().map_err(invalid)?);

        let mut wants = locked_write(&self.inner.wants).ok_or_else(busy)?;
        let want = Shared::new(Want {
            index: wants.len(),
            name: name.into(),
            parameters,
            output,
        });
        let current = self.inner.generation()?;
        let function = check(&self.inner.engine, &current.program, &want)?;

        // The current program, with this handle's function added.
        let mut functions = current.functions.to_vec();
        functions.push(Some(function));
        let generation = Shared::new(Generation {
            program: current.program.clone(),
            functions: functions.into_boxed_slice(),
        });
        *locked_write(&self.inner.current).ok_or_else(busy)? = generation;
        wants.push(Shared::downgrade(&want));

        Ok(Func {
            inner: self.inner.clone(),
            want,
            types: PhantomData,
        })
    }

    /// Reread the script's file. When its text changed, compile it, check
    /// every live handle against it, and publish it, so every handle runs
    /// the new code at its next call. Unchanged text costs nothing.
    ///
    /// Returns whether a new program was published. A script made by
    /// [`Engine::script`] has no file, and reloading it does nothing.
    ///
    /// # Errors
    ///
    /// Returns the new text's compile error, or the E0308 or E0425 of a
    /// handle it no longer satisfies, printed as rustc prints them. The old
    /// program keeps running: a reload publishes all of the new program or
    /// none of it.
    pub fn reload(&self) -> Result<bool, Box<EvalAltResult>> {
        let Some(path) = &self.inner.path else {
            return Ok(false);
        };
        let text = read(path)?;
        if self.inner.generation()?.program.source.text() == text {
            return Ok(false);
        }
        let program = Shared::new(compile(&self.inner.engine, text, &path.to_string_lossy())?);

        let wants = locked_write(&self.inner.wants).ok_or_else(busy)?;
        let functions = wants
            .iter()
            .map(|want| {
                want.upgrade()
                    .map(|want| check(&self.inner.engine, &program, &want))
                    .transpose()
            })
            .collect::<Result<Box<[_]>, _>>()?;
        *locked_write(&self.inner.current).ok_or_else(busy)? =
            Shared::new(Generation { program, functions });
        Ok(true)
    }
}

impl Inner {
    fn generation(&self) -> Result<Shared<Generation>, Box<EvalAltResult>> {
        locked_read(&self.current)
            .map(|current| current.clone())
            .ok_or_else(busy)
    }
}

impl<A, R> Func<A, R> {
    fn invoke(&self, arguments: &mut [Dynamic]) -> RawrResult {
        let generation = self.inner.generation()?;
        let function = generation.functions[self.want.index]
            .as_ref()
            .expect("a live handle has a function in every generation");
        self.inner
            .engine
            .call_resolved(&generation.program.ast, function, arguments)
    }
}

fn busy() -> Box<EvalAltResult> {
    ERR::ErrorRuntime(
        "the script is being reloaded on another thread".into(),
        Position::NONE,
    )
    .into()
}

/// The function `want` names in `program`, when its signature takes the
/// handle's types.
fn check(
    engine: &Engine,
    program: &Program,
    want: &Want,
) -> Result<Shared<ScriptFuncDef>, Box<EvalAltResult>> {
    let expected = signatures::function_type(&want.parameters, &want.output);
    let Some(signature) = program.signatures.get(&want.name) else {
        let report = Report::error(
            code::SCRIPT_NO_FUNCTION,
            format!("cannot find function `{}` in this script", want.name),
        )
        .source(program.source.clone())
        .help(format!("the app calls `{}` as `{expected}`", want.name))
        .finish();
        return Err(failure(
            &report,
            resolve::Error::UnknownFunction {
                name: want.name.clone(),
                import: None,
                reference: crate::Span::NONE,
            },
            Position::NONE,
        ));
    };
    if !signature.accepts(&want.parameters, &want.output, &engine.host_registry) {
        let found = signature.display();
        let label = format!("expected `{expected}`, found `{found}`");
        let report = Report::error(code::SCRIPT_SIGNATURE, "mismatched types").primary(
            program.source.clone(),
            signature.span(),
            Some(label.into()),
        );
        let report = match report {
            Ok(report) => report,
            // No span to point at: name the file.
            Err(_) => Report::error(
                code::SCRIPT_SIGNATURE,
                format!("mismatched types: expected `{expected}`, found `{found}`"),
            )
            .source(program.source.clone()),
        };
        return Err(failure(
            &report.finish(),
            resolve::Error::TypeMismatch {
                expected: expected.into(),
                actual: found.into(),
                expression: signature.span(),
            },
            signature.span().start(),
        ));
    }
    program
        .ast
        .shared_lib()
        .get_script_fn(signature.name(), signature.arity())
        .cloned()
        .ok_or_else(|| ERR::ErrorFunctionNotFound(want.name.to_string(), Position::NONE).into())
}

fn failure(report: &Report, error: resolve::Error, position: Position) -> Box<EvalAltResult> {
    let text = diagnostic::render(report);
    ERR::ErrorParsing(
        ParseErrorType::Resolution(error),
        position,
        Rendered::new(text.trim_end()),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use crate::{Engine, Script};

    fn script(text: &str) -> Script {
        Engine::new()
            .script(text)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn error<T>(result: Result<T, Box<crate::EvalAltResult>>) -> String {
        match result {
            Ok(_) => panic!("the lookup succeeded"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn the_table_keeps_each_free_function_s_signature() {
        let script = script(
            "struct Point { x: i64 }
             impl Point { fn get(&self) -> i64 { self.x } }
             fn price(subtotal: f64, items: i64) -> f64 { subtotal * items as f64 }
             fn greet(name: &str) {}
             fn label(note: Option<String>) -> Option<String> { note }",
        );
        let current = script.inner.generation().unwrap();
        let table = &current.program.signatures;
        let shown = |name: &str| table.get(name).map(super::signatures::Signature::display);
        assert_eq!(shown("price").as_deref(), Some("fn(f64, i64) -> f64"));
        assert_eq!(shown("greet").as_deref(), Some("fn(&str)"));
        assert_eq!(
            shown("label").as_deref(),
            Some("fn(Option<String>) -> Option<String>")
        );
        // A method isn't a free function.
        assert_eq!(shown("get"), None);
        let span = table.get("price").unwrap().span();
        assert_eq!(
            (span.start().line(), span.start().position()),
            (Some(3), Some(14))
        );
        assert_eq!(span.end().position(), Some(56));
    }

    #[test]
    fn a_missing_function_is_e0425() {
        let script = script("fn price(subtotal: f64) -> f64 { subtotal }");
        assert_eq!(
            error(script.func::<(f64,), f64>("cost")),
            "error[E0425]: cannot find function `cost` in this script\n \
             --> <script>\n  \
             = help: the app calls `cost` as `fn(f64) -> f64`"
        );
    }

    #[test]
    fn a_mismatched_signature_is_e0308_at_the_script_s_signature() {
        let script = script("fn price(subtotal: f64, items: i64) -> f64 {\n    subtotal\n}");
        assert_eq!(
            error(script.func::<(f64, f64), f64>("price")),
            "error[E0308]: mismatched types\n \
             --> <script>:1:1-1:43\n \
             1 | fn price(subtotal: f64, items: i64) -> f64 {\n   \
             | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ \
             expected `fn(f64, f64) -> f64`, found `fn(f64, i64) -> f64`"
        );
        assert!(error(script.func::<(f64, i64), i64>("price"))
            .contains("expected `fn(f64, i64) -> i64`, found `fn(f64, i64) -> f64`"));
        assert!(error(script.func::<(f64,), f64>("price"))
            .contains("expected `fn(f64) -> f64`, found `fn(f64, i64) -> f64`"));
        // Not a type rustc would accept for a function: `-> ()`.
        assert!(error(script.func::<(f64, i64), ()>("price"))
            .contains("expected `fn(f64, i64)`, found `fn(f64, i64) -> f64`"));
    }

    #[test]
    fn a_generic_function_takes_no_handle() {
        let script = script("fn same<T>(x: T) -> T { x }");
        assert!(error(script.func::<(i64,), i64>("same")).contains("found `fn(T) -> T (generic)`"));
    }

    #[test]
    fn handles_call_functions_of_each_arity_with_plain_literals() {
        let script = script(
            "fn zero() -> i64 { 0 }
             fn one(a: i64) -> i64 { a }
             fn two(a: i64, b: f64) -> f64 { a as f64 + b }
             fn three(a: i64, b: i64, c: i64) -> i64 { a + b + c }
             fn four(a: i64, b: i64, c: i64, d: i64) -> i64 { a + b + c + d }
             fn five(a: i64, b: i64, c: i64, d: i64, e: i64) -> i64 { a + b + c + d + e }
             fn six(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64) -> i64 { a + b + c + d + e + f }
             fn seven(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64) -> i64 {
                 a + b + c + d + e + f + g
             }
             fn eight(a: String, b: &str, c: bool, d: char, e: Option<i64>, f: f64, g: i64, h: i64) -> String {
                 format!(\"{a}{b}{c}{d}{}{f}{}\", e.unwrap_or(0), g + h)
             }
             fn nothing(a: i64) {}",
        );
        assert_eq!(script.func::<(), i64>("zero").unwrap().call().unwrap(), 0);
        assert_eq!(
            script.func::<(i64,), i64>("one").unwrap().call(1).unwrap(),
            1
        );
        assert_eq!(
            script
                .func::<(i64, f64), f64>("two")
                .unwrap()
                .call(1, 0.5)
                .unwrap(),
            1.5
        );
        let three = script.func::<(i64, i64, i64), i64>("three").unwrap();
        assert_eq!(three.call(1, 2, 3).unwrap(), 6);
        let four = script.func::<(i64, i64, i64, i64), i64>("four").unwrap();
        assert_eq!(four.call(1, 2, 3, 4).unwrap(), 10);
        let five = script
            .func::<(i64, i64, i64, i64, i64), i64>("five")
            .unwrap();
        assert_eq!(five.call(1, 2, 3, 4, 5).unwrap(), 15);
        let six = script
            .func::<(i64, i64, i64, i64, i64, i64), i64>("six")
            .unwrap();
        assert_eq!(six.call(1, 2, 3, 4, 5, 6).unwrap(), 21);
        let seven = script
            .func::<(i64, i64, i64, i64, i64, i64, i64), i64>("seven")
            .unwrap();
        assert_eq!(seven.call(1, 2, 3, 4, 5, 6, 7).unwrap(), 28);
        let eight = script
            .func::<(String, String, bool, char, Option<i64>, f64, i64, i64), String>("eight")
            .unwrap();
        assert_eq!(
            eight
                .call("a".into(), "b".into(), true, 'c', Some(4), 0.5, 6, 7)
                .unwrap(),
            "abtruec40.513"
        );
        script
            .func::<(i64,), ()>("nothing")
            .unwrap()
            .call(1)
            .unwrap();
        // The first handle still calls its own function.
        assert_eq!(three.call(4, 5, 6).unwrap(), 15);
    }

    #[test]
    fn a_handle_skips_the_script_s_top_level_statements() {
        let script = script(
            "fn double(x: i64) -> i64 { x * 2 }
             panic!(\"top level\");",
        );
        let double = script.func::<(i64,), i64>("double").unwrap();
        assert_eq!(double.call(21).unwrap(), 42);
    }
}
