//! Module defining the AST (abstract syntax tree).

use super::{ASTFlags, Expr, FnAccess, Stmt};
use crate::{expose_under_internals, Dynamic, FnNamespace, ImmutableString, Position, ThinVec};
use std::{
    borrow::Borrow,
    fmt,
    hash::Hash,
    ops::{Add, AddAssign},
    ptr,
};

/// Compiled AST (abstract syntax tree) of a Rawr script.
///
/// # Thread Safety
///
/// Currently, [`AST`] is neither `Send` nor `Sync`. Turn on the `sync` feature to make it `Send + Sync`.
#[derive(Clone)]
pub struct AST {
    /// Source of the [`AST`].
    source: Option<ImmutableString>,
    /// Global statements.
    body: ThinVec<Stmt>,
    /// Script-defined functions.
    lib: crate::SharedModule,
    /// Compiler-owned native dependencies required by retained statements and
    /// function bodies. Kept separate so public script-function transforms
    /// cannot discard their proof-carrying operations.
    support: crate::SharedModule,
    /// The exported host items this program calls, bound to its engine's
    /// registry on first evaluation.
    host: Option<crate::Shared<crate::program::hosted::Hosted>>,
    /// Embedded module resolver, if any.
    pub(crate) resolver: Option<crate::Shared<crate::module::resolvers::StaticModuleResolver>>,
    /// [`AST`] documentation.
    #[cfg(feature = "metadata")]
    pub(crate) doc: crate::SmartString,
}

impl Default for AST {
    #[inline(always)]
    fn default() -> Self {
        Self::empty()
    }
}

impl fmt::Debug for AST {
    #[cold]
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fp = f.debug_struct("AST");

        fp.field("source", &self.source);
        #[cfg(feature = "metadata")]
        fp.field("doc", &self.doc);
        fp.field("resolver", &self.resolver);

        fp.field("body", &self.body);

        for (.., fn_def) in self.lib.iter_script_fn() {
            let sig = fn_def.to_string();
            fp.field(&sig, &fn_def.body.statements());
        }

        fp.finish()
    }
}

impl AST {
    /// _(internals)_ Create a new [`AST`].
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline]
    #[must_use]
    fn new(
        statements: impl IntoIterator<Item = Stmt>,
        functions: impl Into<crate::SharedModule>,
    ) -> Self {
        Self {
            source: None,
            #[cfg(feature = "metadata")]
            doc: crate::SmartString::new_const(),
            body: statements.into_iter().collect(),
            lib: functions.into(),
            support: crate::Module::new().into(),
            host: None,
            resolver: None,
        }
    }
    /// _(internals)_ Create a new [`AST`] with a source name.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline]
    #[must_use]
    fn new_with_source(
        statements: impl IntoIterator<Item = Stmt>,
        functions: impl Into<crate::SharedModule>,
        source: impl Into<ImmutableString>,
    ) -> Self {
        let mut ast = Self::new(statements, functions);
        ast.set_source(source);
        ast
    }
    /// Create a new [`AST`] from a shared [`Module`][crate::Module].
    #[inline]
    #[must_use]
    pub fn new_from_module(module: impl Into<crate::SharedModule>) -> Self {
        let module = module.into();

        if let Some(source) = module.id() {
            let source: crate::SmartString = source.into();
            Self::new_with_source(std::iter::empty(), module, source)
        } else {
            Self::new(std::iter::empty(), module)
        }
    }
    /// Create an empty [`AST`].
    #[inline(always)]
    #[must_use]
    pub fn empty() -> Self {
        Self {
            source: None,
            #[cfg(feature = "metadata")]
            doc: crate::SmartString::new_const(),
            body: <_>::default(),
            lib: crate::Module::new().into(),
            support: crate::Module::new().into(),
            host: None,
            resolver: None,
        }
    }
    /// Get the source, if any.
    #[inline(always)]
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
    /// Get a reference to the source.
    #[inline(always)]
    #[must_use]
    pub(crate) const fn source_raw(&self) -> Option<&ImmutableString> {
        self.source.as_ref()
    }
    /// Set the source.
    #[inline]
    pub fn set_source(&mut self, source: impl Into<ImmutableString>) -> &mut Self {
        let source = source.into();

        crate::Shared::get_mut(&mut self.lib)
            .as_mut()
            .map(|m| m.set_id(source.clone()));

        self.source = (!source.is_empty()).then_some(source);

        self
    }
    /// Clear the source.
    #[inline(always)]
    pub fn clear_source(&mut self) -> &mut Self {
        self.source = None;
        self
    }
    /// Get the documentation (if any).
    /// Exported under the `metadata` feature only.
    ///
    /// Documentation is a collection of all comment lines beginning with `//!`.
    ///
    /// Leading white-spaces are stripped, and each line always starts with `//!`.
    #[cfg(feature = "metadata")]
    #[inline(always)]
    #[must_use]
    pub fn doc(&self) -> &str {
        &self.doc
    }
    /// _(internals)_ Get the statements.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline(always)]
    #[must_use]
    fn statements(&self) -> &[Stmt] {
        &self.body
    }
    /// Get the statements.
    #[inline(always)]
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn statements_mut(&mut self) -> &mut ThinVec<Stmt> {
        &mut self.body
    }
    /// Does this [`AST`] contain script-defined functions?
    #[inline(always)]
    #[must_use]
    pub fn has_functions(&self) -> bool {
        self.lib.iter_script_fn().next().is_some()
    }
    /// _(internals)_ Get the internal shared [`Module`][crate::Module] containing all script-defined functions.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline(always)]
    #[must_use]
    const fn shared_lib(&self) -> &crate::SharedModule {
        &self.lib
    }
    /// Get compiler-owned runtime support carried by this AST.
    #[inline(always)]
    pub(crate) const fn shared_support(&self) -> &crate::SharedModule {
        &self.support
    }
    /// The exported host items this program calls.
    pub(crate) fn host(&self) -> Option<&crate::program::hosted::Hosted> {
        self.host.as_deref()
    }
    /// Retain the exported host items this program calls.
    pub(crate) fn set_host(&mut self, host: crate::program::hosted::Hosted) {
        self.host = Some(crate::Shared::new(host));
    }
    /// Merge compiler-owned runtime support into this AST.
    pub(crate) fn add_support(&mut self, support: impl Into<crate::SharedModule>) {
        let support = support.into();
        if !support.is_empty() {
            crate::func::shared_make_mut(&mut self.support).merge(&support);
        }
    }
    /// _(internals)_ Get the embedded [module resolver][crate::ModuleResolver].
    /// Exported under the `internals` feature only.
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub const fn resolver(
        &self,
    ) -> Option<&crate::Shared<crate::module::resolvers::StaticModuleResolver>> {
        self.resolver.as_ref()
    }
    /// Clone the [`AST`]'s functions into a new [`AST`].
    /// No statements are cloned.
    ///
    /// This operation is cheap because functions are shared.
    #[inline(always)]
    #[must_use]
    pub fn clone_functions_only(&self) -> Self {
        self.clone_functions_only_filtered(|_, _, _, _, _| true)
    }
    /// Clone the [`AST`]'s functions into a new [`AST`] based on a filter predicate.
    /// No statements are cloned.
    ///
    /// This operation is cheap because functions are shared.
    #[inline]
    #[must_use]
    pub fn clone_functions_only_filtered(
        &self,
        filter: impl Fn(FnNamespace, FnAccess, bool, &str, usize) -> bool,
    ) -> Self {
        let mut lib = crate::Module::new();
        lib.merge_filtered(&self.lib, &filter);
        Self {
            source: self.source.clone(),
            #[cfg(feature = "metadata")]
            doc: self.doc.clone(),
            body: <_>::default(),
            lib: lib.into(),
            support: self.support.clone(),
            host: self.host.clone(),
            resolver: self.resolver.clone(),
        }
    }
    /// Clone the [`AST`]'s script statements into a new [`AST`].
    /// No functions are cloned.
    #[inline(always)]
    #[must_use]
    pub fn clone_statements_only(&self) -> Self {
        Self {
            source: self.source.clone(),
            #[cfg(feature = "metadata")]
            doc: self.doc.clone(),
            body: self.body.clone(),
            lib: crate::Module::new().into(),
            support: self.support.clone(),
            host: self.host.clone(),
            resolver: self.resolver.clone(),
        }
    }
    /// Merge two [`AST`] into one.  Both [`AST`]'s are untouched and a new, merged,
    /// version is returned.
    ///
    /// Statements in the second [`AST`] are simply appended to the end of the first _without any processing_.
    /// Thus, the return value of the first [`AST`] (if using expression-statement syntax) is buried.
    /// Of course, if the first [`AST`] uses a `return` statement at the end, then
    /// the second [`AST`] will essentially be dead code.
    ///
    /// All script-defined functions in the second [`AST`] overwrite similarly-named functions
    /// in the first [`AST`] with the same number of parameters.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// let ast1 = engine.compile("
    ///     fn foo(x: i64) -> i64 { 42 + x }
    ///     foo(1)
    /// ")?;
    ///
    /// let ast2 = engine.compile("
    ///     fn foo(n: i64) -> i64 { n * 100 }
    ///     foo(5)
    /// ")?;
    ///
    /// let ast = ast1.merge(&ast2);    // Merge 'ast2' into 'ast1'
    ///
    /// // Notice that using the '+' operator also works:
    /// // let ast = &ast1 + &ast2;
    ///
    /// // 'ast' is essentially:
    /// //
    /// //    fn foo(n: i64) -> i64 { n * 100 }  // <- definition of first 'foo' is overwritten
    /// //    foo(1)                             // <- notice this will be 100 instead of 43,
    /// //                                       //    but it is no longer the return value
    /// //    foo(5)                             // returns 500
    ///
    /// // Evaluate it
    /// assert_eq!(engine.eval_ast::<i64>(&ast)?, 500);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        self.merge_filtered_impl(other, |_, _, _, _, _| true)
    }
    /// Combine one [`AST`] with another.  The second [`AST`] is consumed.
    ///
    /// Statements in the second [`AST`] are simply appended to the end of the first _without any processing_.
    /// Thus, the return value of the first [`AST`] (if using expression-statement syntax) is buried.
    /// Of course, if the first [`AST`] uses a `return` statement at the end, then
    /// the second [`AST`] will essentially be dead code.
    ///
    /// All script-defined functions in the second [`AST`] overwrite similarly-named functions
    /// in the first [`AST`] with the same number of parameters.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// let mut ast1 = engine.compile("
    ///     fn foo(x: i64) -> i64 { 42 + x }
    ///     foo(1)
    /// ")?;
    ///
    /// let ast2 = engine.compile("
    ///     fn foo(n: i64) -> i64 { n * 100 }
    ///     foo(5)
    /// ")?;
    ///
    /// ast1.combine(ast2);    // Combine 'ast2' into 'ast1'
    ///
    /// // Notice that using the '+=' operator also works:
    /// // ast1 += ast2;
    ///
    /// // 'ast1' is essentially:
    /// //
    /// //    fn foo(n: i64) -> i64 { n * 100 }  // <- definition of first 'foo' is overwritten
    /// //    foo(1)                             // <- notice this will be 100 instead of 43,
    /// //                                       //    but it is no longer the return value
    /// //    foo(5)                             // returns 500
    ///
    /// // Evaluate it
    /// assert_eq!(engine.eval_ast::<i64>(&ast1)?, 500);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn combine(&mut self, other: Self) -> &mut Self {
        self.combine_filtered_impl(other, |_, _, _, _, _| true)
    }
    /// Merge two [`AST`] into one.  Both [`AST`]'s are untouched and a new, merged, version
    /// is returned.
    ///
    /// Statements in the second [`AST`] are simply appended to the end of the first _without any processing_.
    /// Thus, the return value of the first [`AST`] (if using expression-statement syntax) is buried.
    /// Of course, if the first [`AST`] uses a `return` statement at the end, then
    /// the second [`AST`] will essentially be dead code.
    ///
    /// All script-defined functions in the second [`AST`] are first selected based on a filter
    /// predicate, then overwrite similarly-named functions in the first [`AST`] with the
    /// same number of parameters.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// let ast1 = engine.compile("
    ///     fn foo(x: i64) -> i64 { 42 + x }
    ///     foo(1)
    /// ")?;
    ///
    /// let ast2 = engine.compile("
    ///     fn foo(n: i64) -> i64 { n * 100 }
    ///     fn error() -> i64 { 0 }
    ///     foo(5)
    /// ")?;
    ///
    /// // Merge 'ast2', picking only 'error()' but not 'foo(..)', into 'ast1'
    /// let ast = ast1.merge_filtered(&ast2, |_, _, script, name, params|
    ///                                 script && name == "error" && params == 0);
    ///
    /// // 'ast' is essentially:
    /// //
    /// //    fn foo(x: i64) -> i64 { 42 + x }  // <- definition of 'ast1::foo' is not overwritten
    /// //                                       //    because 'ast2::foo' is filtered away
    /// //    foo(1)                             // <- notice this will be 43 instead of 100,
    /// //                                       //    but it is no longer the return value
    /// //    fn error() -> i64 { 0 }            // <- this function passes the filter and is merged
    /// //    foo(5)                             // <- returns 47
    ///
    /// // Evaluate it
    /// assert_eq!(engine.eval_ast::<i64>(&ast)?, 47);
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    #[must_use]
    pub fn merge_filtered(
        &self,
        other: &Self,
        filter: impl Fn(FnNamespace, FnAccess, bool, &str, usize) -> bool,
    ) -> Self {
        self.merge_filtered_impl(other, filter)
    }
    /// Merge two [`AST`] into one.  Both [`AST`]'s are untouched and a new, merged, version
    /// is returned.
    #[inline]
    #[must_use]
    fn merge_filtered_impl(
        &self,
        other: &Self,
        _filter: impl Fn(FnNamespace, FnAccess, bool, &str, usize) -> bool,
    ) -> Self {
        let merged = match (self.body.as_ref(), other.body.as_ref()) {
            ([], []) => <_>::default(),
            (_, []) => self.body.to_vec(),
            ([], _) => other.body.to_vec(),
            (body, other) => {
                let mut body = body.to_vec();
                body.extend(other.iter().cloned());
                body
            }
        };

        let lib = {
            let mut lib = self.lib.as_ref().clone();
            lib.merge_filtered(&other.lib, &_filter);
            lib
        };

        let support = {
            let mut support = self.support.as_ref().clone();
            support.merge(&other.support);
            support
        };

        let mut _ast = match other.source {
            Some(ref source) => Self::new_with_source(merged, lib, source.clone()),
            None => Self::new(merged, lib),
        };

        {
            _ast.support = support.into();
        }
        _ast.host = self.host.clone().or_else(|| other.host.clone());

        match (
            self.resolver.as_deref().is_none_or(|r| r.is_empty()),
            other.resolver.as_deref().is_none_or(|r| r.is_empty()),
        ) {
            (true, true) => (),
            (false, true) => _ast.resolver.clone_from(&self.resolver),
            (true, false) => _ast.resolver.clone_from(&other.resolver),
            (false, false) => {
                let mut resolver = self.resolver.as_deref().unwrap().clone();
                for (k, v) in other.resolver.as_deref().unwrap() {
                    resolver.insert(k.clone(), v.as_ref().clone());
                }
                _ast.resolver = Some(resolver.into());
            }
        }

        #[cfg(feature = "metadata")]
        match (other.doc.as_str(), _ast.doc.as_str()) {
            ("", _) => (),
            (_, "") => _ast.doc = other.doc.clone(),
            (_, _) => {
                _ast.doc.push_str("\n");
                _ast.doc.push_str(&other.doc);
            }
        }

        _ast
    }
    /// Combine one [`AST`] with another.  The second [`AST`] is consumed.
    ///
    /// Statements in the second [`AST`] are simply appended to the end of the first _without any processing_.
    /// Thus, the return value of the first [`AST`] (if using expression-statement syntax) is buried.
    /// Of course, if the first [`AST`] uses a `return` statement at the end, then
    /// the second [`AST`] will essentially be dead code.
    ///
    /// All script-defined functions in the second [`AST`] are first selected based on a filter
    /// predicate, then overwrite similarly-named functions in the first [`AST`] with the
    /// same number of parameters.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// let mut ast1 = engine.compile("
    ///     fn foo(x: i64) -> i64 { 42 + x }
    ///     foo(1)
    /// ")?;
    ///
    /// let ast2 = engine.compile("
    ///     fn foo(n: i64) -> i64 { n * 100 }
    ///     fn error() -> i64 { 0 }
    ///     foo(5)
    /// ")?;
    ///
    /// // Combine 'ast2', picking only 'error()' but not 'foo(..)', into 'ast1'
    /// ast1.combine_filtered(ast2, |_, _, script, name, params|
    ///                                 script && name == "error" && params == 0);
    ///
    /// // 'ast1' is essentially:
    /// //
    /// //    fn foo(x: i64) -> i64 { 42 + x }  // <- definition of 'ast1::foo' is not overwritten
    /// //                                       //    because 'ast2::foo' is filtered away
    /// //    foo(1)                             // <- notice this will be 43 instead of 100,
    /// //                                       //    but it is no longer the return value
    /// //    fn error() -> i64 { 0 }            // <- this function passes the filter and is merged
    /// //    foo(5)                             // <- returns 47
    ///
    /// // Evaluate it
    /// assert_eq!(engine.eval_ast::<i64>(&ast1)?, 47);
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn combine_filtered(
        &mut self,
        other: Self,
        filter: impl Fn(FnNamespace, FnAccess, bool, &str, usize) -> bool,
    ) -> &mut Self {
        self.combine_filtered_impl(other, filter)
    }
    /// Combine one [`AST`] with another.  The second [`AST`] is consumed.
    fn combine_filtered_impl(
        &mut self,
        other: Self,
        _filter: impl Fn(FnNamespace, FnAccess, bool, &str, usize) -> bool,
    ) -> &mut Self {
        match (
            self.resolver.as_deref().is_none_or(|r| r.is_empty()),
            other.resolver.as_deref().is_none_or(|r| r.is_empty()),
        ) {
            (_, true) => (),
            (true, false) => self.resolver.clone_from(&other.resolver),
            (false, false) => {
                let resolver = crate::func::shared_make_mut(self.resolver.as_mut().unwrap());
                let other_resolver = crate::func::shared_take_or_clone(other.resolver.unwrap());
                for (k, v) in other_resolver {
                    resolver.insert(k, crate::func::shared_take_or_clone(v));
                }
            }
        }

        if self.host.is_none() {
            self.host = other.host.clone();
        }

        match (self.body.as_ref(), other.body.as_ref()) {
            (_, []) => (),
            ([], _) => self.body = other.body,
            (_, _) => self.body.extend(other.body),
        }

        if !other.lib.is_empty() {
            crate::func::shared_make_mut(&mut self.lib).merge_filtered(&other.lib, &_filter);
        }

        if !other.support.is_empty() {
            crate::func::shared_make_mut(&mut self.support).merge(&other.support);
        }

        #[cfg(feature = "metadata")]
        match (other.doc.as_str(), self.doc.as_str()) {
            ("", _) => (),
            (_, "") => self.doc = other.doc,
            (_, _) => {
                self.doc.push_str("\n");
                self.doc.push_str(&other.doc);
            }
        }

        self
    }
    /// Filter out the functions, retaining only some based on a filter predicate.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// # {
    /// use rawr::Engine;
    ///
    /// let engine = Engine::new();
    ///
    /// let mut ast = engine.compile(r#"
    ///     fn foo(n: i64) -> i64 { n + 1 }
    ///     fn bar() { println!("hello"); }
    /// "#)?;
    ///
    /// // Remove all functions except 'foo(..)'
    /// ast.retain_functions(|_, _, name, params| name == "foo" && params == 1);
    ///
    /// assert_eq!(ast.iter_functions().count(), 1);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn retain_functions(
        &mut self,
        filter: impl Fn(FnNamespace, FnAccess, &str, usize) -> bool,
    ) -> &mut Self {
        if self.has_functions() {
            crate::func::shared_make_mut(&mut self.lib).retain_script_functions(filter);
        }
        self
    }
    /// _(internals)_ Iterate through all function definitions.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline]
    fn iter_fn_def(&self) -> impl Iterator<Item = &crate::Shared<super::ScriptFuncDef>> {
        self.lib.iter_script_fn().map(|(.., fn_def)| fn_def)
    }

    pub(crate) fn for_each_fn_def_mut(&mut self, callback: impl FnMut(&mut super::ScriptFuncDef)) {
        crate::func::shared_make_mut(&mut self.lib).for_each_script_fn_mut(callback);
    }

    /// Make the script function `name` also callable as `alias`, keeping it
    /// under `name` for the calls that already name it.
    pub(crate) fn alias_script_function(&mut self, name: &str, alias: &str) {
        let Some(definition) = self
            .lib
            .iter_script_fn()
            .find(|(.., definition)| definition.name.as_str() == name)
            .map(|(.., definition)| definition.as_ref().clone())
        else {
            return;
        };
        let mut aliased = definition;
        aliased.name = alias.into();
        crate::func::shared_make_mut(&mut self.lib).set_script_fn(aliased);
    }

    /// Rebuild the script-function index after compiler-owned definitions are renamed.
    pub(crate) fn reindex_script_functions(&mut self) {
        let definitions: Vec<_> = self
            .lib
            .iter_script_fn()
            .map(|(.., definition)| definition.clone())
            .collect();
        let mut module = crate::Module::new();

        for definition in definitions {
            module.set_script_fn(definition);
        }

        self.lib = module.into();
    }
    /// Iterate through all function definitions.
    #[inline]
    pub fn iter_functions(&self) -> impl Iterator<Item = super::ScriptFnMetadata<'_>> {
        self.lib
            .iter_script_fn()
            .map(|(.., fn_def)| fn_def.as_ref().into())
    }
    /// Clear all function definitions in the [`AST`].
    #[inline(always)]
    pub fn clear_functions(&mut self) -> &mut Self {
        self.lib = crate::Module::new().into();
        self
    }
    /// Clear all statements in the [`AST`], leaving only function definitions.
    #[inline(always)]
    pub fn clear_statements(&mut self) -> &mut Self {
        self.body = <_>::default();
        self
    }
    /// Extract all top-level literal constant and/or variable definitions.
    /// This is useful for extracting all global constants from a script without actually running it.
    ///
    /// A literal definition takes the form of `let VAR = `_value_`;` or
    /// `let mut VAR = `_value_`;` where _value_ is a literal expression.
    /// An immutable `let` counts as a constant and a `let mut` counts as a variable.
    /// A `const` item is not included.
    ///
    /// # Example
    ///
    /// ```
    /// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
    /// use rawr::{Engine, Scope};
    ///
    /// let engine = Engine::new();
    ///
    /// let ast = engine.compile(
    /// r#"
    ///     let a = 42;              // literal constant
    ///     let mut b = 123;         // literal variable
    ///     let c = b * a;           // not a literal
    ///     const D: i64 = 999;      // const item, not included
    ///     b = a + c + D;           // expression
    ///
    ///     {                        // <- new block scope
    ///         let z = 0;           // <- literal constant not at top-level
    ///
    ///         println!("{z}");
    ///     }
    /// "#)?;
    ///
    /// let mut iter = ast.iter_literal_variables(true, false)
    ///                   .map(|(name, is_const, value)| (name, is_const, value.as_int().unwrap()));
    ///
    /// assert_eq!(iter.next(), Some(("a", true, 42)));
    /// assert_eq!(iter.next(), None);
    ///
    /// let mut iter = ast.iter_literal_variables(false, true)
    ///                   .map(|(name, is_const, value)| (name, is_const, value.as_int().unwrap()));
    ///
    /// assert_eq!(iter.next(), Some(("b", false, 123)));
    /// assert_eq!(iter.next(), None);
    ///
    /// let mut iter = ast.iter_literal_variables(true, true)
    ///                   .map(|(name, is_const, value)| (name, is_const, value.as_int().unwrap()));
    ///
    /// assert_eq!(iter.next(), Some(("a", true, 42)));
    /// assert_eq!(iter.next(), Some(("b", false, 123)));
    /// assert_eq!(iter.next(), None);
    ///
    /// let scope: Scope = ast.iter_literal_variables(true, false).collect();
    ///
    /// assert_eq!(scope.len(), 1);
    ///
    /// Ok(())
    /// # }
    /// ```
    pub fn iter_literal_variables(
        &self,
        include_constants: bool,
        include_variables: bool,
    ) -> impl Iterator<Item = (&str, bool, Dynamic)> {
        self.statements().iter().filter_map(move |stmt| match stmt {
            Stmt::Var(x, options, ..)
                if options.intersects(ASTFlags::CONSTANT) && include_constants
                    || !options.intersects(ASTFlags::CONSTANT) && include_variables =>
            {
                let (name, expr, ..) = &**x;
                expr.get_literal_value(None)
                    .map(|value| (name.as_str(), options.intersects(ASTFlags::CONSTANT), value))
            }
            _ => None,
        })
    }
    /// _(internals)_ Recursively walk the [`AST`], including function bodies (if any).
    /// Return `false` from the callback to terminate the walk.
    /// Exported under the `internals` feature only.
    #[cfg(feature = "internals")]
    #[inline(always)]
    pub fn walk(&self, on_node: &mut (impl FnMut(&[ASTNode]) -> bool + ?Sized)) -> bool {
        self._walk(on_node)
    }
    /// Recursively walk the [`AST`], including function bodies (if any).
    /// Return `false` from the callback to terminate the walk.
    pub(crate) fn _walk(&self, on_node: &mut (impl FnMut(&[ASTNode]) -> bool + ?Sized)) -> bool {
        let path = &mut Vec::new();

        for stmt in self.statements() {
            if !stmt.walk(path, on_node) {
                return false;
            }
        }
        for stmt in self.iter_fn_def().flat_map(|f| f.body.iter()) {
            if !stmt.walk(path, on_node) {
                return false;
            }
        }

        true
    }
}

impl<A: AsRef<AST>> Add<A> for &AST {
    type Output = AST;

    #[inline(always)]
    fn add(self, rhs: A) -> Self::Output {
        self.merge(rhs.as_ref())
    }
}

impl<A: Into<Self>> AddAssign<A> for AST {
    #[inline(always)]
    fn add_assign(&mut self, rhs: A) {
        self.combine(rhs.into());
    }
}

impl Borrow<[Stmt]> for AST {
    #[inline(always)]
    fn borrow(&self) -> &[Stmt] {
        self.statements()
    }
}

impl AsRef<[Stmt]> for AST {
    #[inline(always)]
    fn as_ref(&self) -> &[Stmt] {
        self.statements()
    }
}

impl Borrow<crate::Module> for AST {
    #[inline(always)]
    fn borrow(&self) -> &crate::Module {
        self.shared_lib()
    }
}

impl AsRef<crate::Module> for AST {
    #[inline(always)]
    fn as_ref(&self) -> &crate::Module {
        self.shared_lib().as_ref()
    }
}

impl Borrow<crate::SharedModule> for AST {
    #[inline(always)]
    fn borrow(&self) -> &crate::SharedModule {
        self.shared_lib()
    }
}

impl AsRef<crate::SharedModule> for AST {
    #[inline(always)]
    fn as_ref(&self) -> &crate::SharedModule {
        self.shared_lib()
    }
}

/// _(internals)_ An [`AST`] node, consisting of either an [`Expr`] or a [`Stmt`].
/// Exported under the `internals` feature only.
#[derive(Debug, Clone, Copy, Hash)]
#[non_exhaustive]
pub enum ASTNode<'a> {
    /// A statement ([`Stmt`]).
    Stmt(&'a Stmt),
    /// An expression ([`Expr`]).
    Expr(&'a Expr),
}

impl<'a> From<&'a Stmt> for ASTNode<'a> {
    #[inline(always)]
    fn from(stmt: &'a Stmt) -> Self {
        Self::Stmt(stmt)
    }
}

impl<'a> From<&'a Expr> for ASTNode<'a> {
    #[inline(always)]
    fn from(expr: &'a Expr) -> Self {
        Self::Expr(expr)
    }
}

impl PartialEq for ASTNode<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Stmt(x), Self::Stmt(y)) => ptr::eq(*x, *y),
            (Self::Expr(x), Self::Expr(y)) => ptr::eq(*x, *y),
            _ => false,
        }
    }
}

impl Eq for ASTNode<'_> {}

impl ASTNode<'_> {
    /// Is this [`ASTNode`] a [`Stmt`]?
    #[inline(always)]
    #[must_use]
    pub const fn is_stmt(&self) -> bool {
        matches!(self, Self::Stmt(..))
    }
    /// Is this [`ASTNode`] an [`Expr`]?
    #[inline(always)]
    #[must_use]
    pub const fn is_expr(&self) -> bool {
        matches!(self, Self::Expr(..))
    }
    /// Get the [`Position`] of this [`ASTNode`].
    #[inline]
    #[must_use]
    pub fn position(&self) -> Position {
        match self {
            Self::Stmt(stmt) => stmt.position(),
            Self::Expr(expr) => expr.position(),
        }
    }
}

/// _(internals)_ Encapsulated AST environment.
/// Exported under the `internals` feature only.
///
/// 1) stack of scripted functions defined
/// 2) the stack of imported [modules][crate::Module]
/// 3) global constants
#[derive(Debug, Clone)]
pub struct EncapsulatedEnviron {
    /// Stack of loaded [modules][crate::Module] containing script-defined functions.
    pub lib: crate::StaticVec<crate::SharedModule>,
    /// Imported [modules][crate::Module].
    pub imports: crate::ThinVec<(ImmutableString, crate::SharedModule)>,
    /// Globally-defined constants.
    pub constants: Option<crate::eval::SharedGlobalConstants>,
}

impl From<&crate::eval::GlobalRuntimeState> for EncapsulatedEnviron {
    fn from(value: &crate::eval::GlobalRuntimeState) -> Self {
        Self {
            lib: value.lib.clone(),
            imports: value
                .iter_imports_raw()
                .map(|(n, m)| (n.clone(), m.clone()))
                .collect(),
            constants: value.constants.clone(),
        }
    }
}
