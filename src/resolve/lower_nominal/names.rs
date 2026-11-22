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
        if let Some(error) =
            self.trait_as_value(name, "function, tuple struct or tuple variant", reference)
        {
            return Some(error);
        }
        if let Some(error) = self.std_not_yet(&written, reference) {
            return Some(error);
        }
        if let Some(error) = self.macro_without_bang(name, "function", reference) {
            return Some(error);
        }
        Some(Error::UnknownFunction {
            name: written.as_ref().into(),
            imports: self.imports(name, Wanted::Call),
            reference: Span::new(at.start(), text_end(at.start(), &written)),
        })
    }

    /// The items named `name` a `use` could bring into scope where the
    /// name is `wanted`, as rustc offers them: the program's own that
    /// another module declares and this one sees, and std's, by path; a
    /// library's function where neither has one.
    pub(super) fn imports(&self, name: &str, wanted: Wanted) -> Box<Imports> {
        let mut candidates = self.own_candidates(name, wanted);
        candidates.extend(crate::resolve::imports::std(name, wanted));
        if candidates.is_empty() && wanted != Wanted::Type {
            let mut library = self.models.host.functions_named(name).collect::<Vec<_>>();
            library.sort_by_key(|path| (path.len(), path.clone()));
            candidates.extend(library.into_iter().next().map(|path| Candidate {
                path: path.into(),
                noun: "function",
            }));
        }
        Imports::new(candidates, self.models.use_placement())
    }

    /// The program's own items named `name` where it is `wanted` that
    /// another module of this package declares at its top and opens to
    /// this one, each by its path from the crate's root.
    pub(super) fn own_candidates(&self, name: &str, wanted: Wanted) -> Vec<Candidate> {
        let Some(here) = self.models.namespaces.get(&self.models.context) else {
            return Vec::new();
        };
        let path = |source: &crate::program::source::Id, header: &crate::item::Header| {
            let namespace = self.models.namespaces.get(source)?;
            (*source != self.models.context
                && namespace.package == here.package
                && header.scope.is_none()
                && namespace.opens_to(header.visibility.as_ref(), here))
            .then(|| {
                let modules = namespace.modules.iter().map(ImmutableString::as_str);
                std::iter::once("crate")
                    .chain(modules)
                    .chain([name])
                    .collect::<Vec<_>>()
                    .join("::")
            })
        };
        let functions = self
            .models
            .functions
            .iter()
            .filter(|((_, named), _)| {
                matches!(wanted, Wanted::Value | Wanted::Call) && named == name
            })
            .filter_map(|((source, _), function)| {
                Some(Candidate {
                    path: path(source, &function.definition.header)?.into(),
                    noun: "function",
                })
            });
        let types = self
            .models
            .by_definition
            .iter()
            .filter(|(_, nominal)| nominal.name() == name)
            .filter_map(|(id, nominal)| {
                let (header, noun) = match nominal {
                    Nominal::Struct { item, .. } => (&item.header, wanted.struct_noun(item.form)?),
                    Nominal::Enum { item, .. } => {
                        let named = matches!(wanted, Wanted::Type | Wanted::Path);
                        (&item.header, named.then_some("enum")?)
                    }
                };
                Some(Candidate {
                    path: path(self.models.declared_in.get(id)?, header)?.into(),
                    noun,
                })
            });
        functions.chain(types).collect()
    }

    /// The std path `path` names in this scope, as rustc resolves it:
    /// through the `use` that binds its first segment, from std's root, or
    /// through the edition's prelude. `None` for a path whose first
    /// segment the program declares (a type, a generic parameter, an alias)
    /// or nothing in scope makes std's. A module the std facade holds
    /// (`rawr_std::sync`) is its std path, and a std type Rawr represents
    /// its canonical path (`std_path::canonical`).
    pub(super) fn std_scoped(&self, path: &str) -> Option<String> {
        let first = path.split("::").next().unwrap_or(path);
        if self.generics.contains_key(first)
            || self.models.get(first).is_some()
            || self.models.alias(first).is_some()
        {
            return None;
        }
        let binding = self.models.import(first);
        let qualified = crate::resolve::std_path::qualified(
            path,
            binding.map(|binding| binding.qualified.as_str()),
        )?;
        let qualified = match qualified
            .strip_prefix(crate::std_facade::ROOT)
            .and_then(|rest| rest.strip_prefix("::"))
        {
            Some(rest) => format!("std::{rest}"),
            None => qualified,
        };
        // A std type written through another of std's modules reads as the
        // path the spelling helpers know.
        Some(crate::resolve::std_path::canonical(&qualified).map_or(qualified, String::from))
    }

    /// The std path the shortest body path starting with `name` (already
    /// read) names through the scope (`std_scoped`) that `accepts` takes,
    /// with the number of tokens after `name` it spans and the token after
    /// them: `HashMap` after `use std::collections::HashMap;`,
    /// `collections::HashMap` after `use std::collections;`.
    pub(super) fn std_path_ahead(
        &self,
        name: &str,
        accepts: impl Fn(&str) -> bool,
    ) -> Option<(String, usize, Token)> {
        let mut ahead = self.input.clone();
        let mut written = name.to_owned();
        let mut consumed = 0;
        loop {
            let next = ahead.next().map_or(Token::EOF, |(token, _)| token);
            if let Some(path) = self.std_scoped(&written).filter(|path| accepts(path)) {
                return Some((path, consumed, next));
            }
            if next != Token::DoubleColon {
                return None;
            }
            let Some((Token::Identifier(segment), _)) = ahead.next() else {
                return None;
            };
            written.push_str("::");
            written.push_str(&segment);
            consumed += 2;
        }
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
            annotation: None,
        })
    }

    /// rustc's E0423 for a trait's name used where a function or a value
    /// is `expected`: the program's own trait in scope, or std's, as
    /// `Fn("main")` or `let c = Clone;`.
    pub(super) fn trait_as_value(
        &self,
        name: &str,
        expected: &'static str,
        reference: Span,
    ) -> Option<Error> {
        let written = crate::tokenizer::raw_names(name);
        let std_trait = || {
            let first = name.split("::").next().unwrap_or(name);
            let binding = self.models.import(first);
            let binding = binding.map(|binding| binding.qualified.as_str());
            crate::resolve::std_path::qualified(&written, binding).is_some_and(|path| {
                crate::std_table::path(&path, crate::std_table::Namespace::Type)
                    .is_some_and(|path| path.kind == "trait")
            })
        };
        let single = !name.contains("::");
        let is_trait = single && self.models.crate_trait_in_scope(name) || std_trait();
        // rustc offers the `:` only for a lone name that starts the value.
        let annotation = self
            .unannotated_let
            .filter(|between| single && between.end() == reference.start());
        is_trait.then(|| Error::WrongKind {
            expected,
            found: "trait",
            name: written.as_ref().into(),
            reference,
            annotation,
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
        if let Some(error) = self.trait_as_value(name, "value", reference) {
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
                annotation: None,
            };
        }
        Error::UnknownVariable {
            name: written.as_ref().into(),
            imports: self.imports(name, Wanted::Value),
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
