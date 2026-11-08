//! Names in scope: the Rust paths a body sees at a point, and rustc's
//! errors for a name that resolves to nothing.

use super::*;

impl<'a> Parser<'a> {
    /// The error for a call of `name` when no function of the crate's has
    /// that name. It is `None` when the engine has a native function of that
    /// name, as `Engine::register_fn` makes, which takes its arguments
    /// untyped at runtime. The error offers the library function of that
    /// name to import when there is one.
    pub(super) fn unknown_function(&self, name: &ImmutableString, at: Span) -> Option<Error> {
        // The untyped standard library (`len`, `pad`, `type_of`) is the
        // evaluator's: Rust has no such free functions. A package built on
        // it keeps the functions it adds.
        let native = self.native_modules.iter().any(|module| {
            module.iter_fn().any(|(_, metadata)| {
                metadata.name.as_str() == name.as_str()
                    && !(module.is_standard_lib() && standard_function(name))
            })
        });
        if native {
            return None;
        }
        let written = crate::tokenizer::raw_names(name);
        let reference = Span::new(at.start(), text_end(at.start(), &written));
        if let Some(error) = self.std_not_yet(&written, reference) {
            return Some(error);
        }
        if let Some(error) = self.macro_without_bang(name, "function", reference) {
            return Some(error);
        }
        let mut imports = self.models.host.functions_named(name).collect::<Vec<_>>();
        imports.sort_by_key(|path| (path.len(), path.clone()));
        Some(Error::UnknownFunction {
            name: written.as_ref().into(),
            import: imports.into_iter().next().map(Into::into),
            reference: Span::new(at.start(), text_end(at.start(), &written)),
        })
    }

    /// rustc's E0423 for a macro's name used without `!` where a function
    /// or a value is `expected`: the program's own macro, or std's, as
    /// `print` or `std::print`.
    pub(super) fn macro_without_bang(
        &self,
        name: &str,
        expected: &'static str,
        reference: Span,
    ) -> Option<Error> {
        let written = crate::tokenizer::raw_names(name);
        let std_macro = || {
            let first = name.split("::").next().unwrap_or(name);
            let binding = self.models.import(first);
            let binding = binding.map(|binding| binding.qualified.as_str());
            crate::resolve::std_path::qualified(&written, binding)
                .is_some_and(|path| crate::resolve::std_path::is_macro(&path))
        };
        let is_macro = self.program_macro(name, reference.start()).is_some() || std_macro();
        is_macro.then(|| Error::WrongKind {
            expected,
            found: "macro",
            name: written.as_ref().into(),
            reference,
        })
    }

    /// A name a `use` binds to a crate module only, used as a value: the
    /// crate's value of that path, as a function beside a module of its name
    /// (`mod text` and `fn text`), is one Rawr can't call, and the import
    /// says why.
    pub(super) fn module_value(&self, name: &str, reference: Span) -> Option<Error> {
        let binding = self.models.import(name)?;
        (matches!(binding.target, item::Import::Module)
            && self.models.host.entry(&binding.qualified).is_some())
        .then(|| Error::UnknownImport {
            path: binding.qualified.clone(),
            reference,
            missing: Unresolved::Path,
        })
    }

    /// A path that names no local, constant or function in scope. A library
    /// function of that name is offered to import.
    pub(super) fn unknown_variable(&mut self, name: &ImmutableString, at: Span) -> Error {
        // `switch x { .. }` and `import "m" as m;`: a name followed by a
        // token no expression continues with is rustc's syntax error there,
        // which it reports before it resolves the name.
        if let Some(error) = self.expected_operator(Continuation::Path) {
            return error;
        }
        let written = crate::tokenizer::raw_names(name);
        let reference = Span::new(at.start(), text_end(at.start(), &written));
        if let Some(error) = self.module_value(name, reference) {
            return error;
        }
        if let Some(error) = self.std_not_yet(&written, reference) {
            return error;
        }
        if let Some(error) = self.macro_without_bang(name, "value", reference) {
            return error;
        }
        // A struct with fields isn't a value: rustc's E0423.
        if let Some(Nominal::Struct { .. }) = self.models.get(name) {
            return Error::WrongKind {
                expected: "value",
                found: "struct",
                name: written.as_ref().into(),
                reference,
            };
        }
        let mut imports = self.models.host.functions_named(name).collect::<Vec<_>>();
        if imports.is_empty() {
            return Error::UnknownVariable {
                name: written.as_ref().into(),
                reference,
            };
        }
        imports.sort_by_key(|path| (path.len(), path.clone()));
        Error::UnknownFunction {
            name: written.as_ref().into(),
            import: imports.into_iter().next().map(Into::into),
            reference,
        }
    }

    /// The Rust import paths in lexical scope at `at` in this source:
    /// module-level imports and those of every block that contains it.
    /// A glob is kept as its module path and `*`.
    #[cfg(feature = "rust-import")]
    pub(super) fn lexical_scope(&self, at: Span) -> Result<ScopePaths, Error> {
        let mut paths = Vec::new();
        let Some(imports) = self.models.lexical.get(&self.models.context) else {
            return Ok(Box::new([]));
        };
        for used in imports.iter() {
            if let Some(scope) = used.scope
                && !(scope.start() <= at.start() && at.end() <= scope.end())
            {
                continue;
            }
            // A glob is its module path and `*`: the proof enumerates the
            // module's traits from the crate's complete exports.
            if let crate::item::UseTree::Glob { prefix, .. } = &used.tree {
                paths.push(
                    prefix
                        .segments
                        .iter()
                        .map(|segment| Box::<str>::from(segment.name.text.as_str()))
                        .chain(std::iter::once(Box::<str>::from("*")))
                        .collect(),
                );
                continue;
            }
            for (_, path) in used.bindings() {
                let path = path
                    .iter()
                    .map(|segment| Box::<str>::from(segment.as_str()))
                    .collect::<Box<[_]>>();
                paths.push(path);
            }
        }
        Ok(paths.into_boxed_slice())
    }
}
