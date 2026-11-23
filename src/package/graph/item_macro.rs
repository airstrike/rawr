//! A call of a dependency's macro where an item goes, among a module's
//! items or a block's statements, or used as a block's expression: rustc
//! expands it before it resolves a name, and the items it expands to are
//! that module's, that block's or the call's own.
//!
//! The parse leaves these calls as they are (`item::MacroCall`), since
//! which crate's macro a path names, and what kind, is known only once the
//! dependencies' facades are. A `macro_rules!` macro expands from its
//! rules, as one called from a body does; any other is a proc macro,
//! which rustc expands with every other call the check finds (D1010). The
//! expansion is laid out after the source's text, as the program's own
//! macros' expansions are (`parser::item_macro`), and calls among its items
//! expand in turn.

use crate::item::MacroCall;
use crate::parser::item::Expansion;
use crate::resolve::package::Failure;
use crate::{host, ImmutableString};

/// The most calls one source's expansions make, a bound on a macro that
/// keeps expanding to more calls.
const CALL_LIMIT: usize = 65_536;

impl super::Graph {
    /// Expand each call of a dependency's macro in the program's modules
    /// and blocks, now that `host` holds the dependencies' facades, and lay
    /// out what each expands to where it is: a block's expression's when
    /// its rules may expand to an item or a path, as the program's own
    /// macros' are. An inline `mod` among the items an expansion makes
    /// loads as a module of the unit's, whose calls expand in turn.
    ///
    /// # Errors
    ///
    /// A module's call of a proc macro rustc has not expanded yet, which it
    /// expands before the program is checked again (#1125), with every
    /// other such call the graph holds ([`Self::unexpanded_calls`]), and a
    /// module's call whose expansion does not parse as items.
    pub(crate) fn expand_crate_macros(&mut self, host: &host::Registry) -> Result<(), Failure> {
        if self.hosted {
            return Ok(());
        }
        let project = self.entry_package().root().to_path_buf();
        #[cfg(feature = "rust-import")]
        let mut unexpanded = None;
        // The units an inline `mod` adds are expanded in turn.
        let mut index = 0;
        while index < self.units.len() {
            let source = self.units[index].source;
            let Some(package) = self
                .packages
                .iter()
                .find(|package| package.name() == self.sources[source.index()].package())
            else {
                index += 1;
                continue;
            };
            let prelude = crate::resolve::extern_prelude::Prelude::of(package, &project);
            let features = package.features().clone();
            let parsed = &mut self.units[index].parsed;
            let mut calls = parsed.take_calls();
            let mut laid = false;
            // Where the items the first expansion adds start.
            let mut added = None;
            let mut taken = 0;
            while taken < calls.len() {
                let call = calls[taken].clone();
                taken += 1;
                if taken > CALL_LIMIT {
                    return Err(Failure {
                        primary: source,
                        secondary: None,
                        error: crate::resolve::Error::Syntax {
                            message: "recursion limit reached while expanding a macro".into(),
                            label: ImmutableString::new(),
                            help: None,
                            at: call.name,
                        },
                    });
                }
                let Some(path) = crate_path(parsed, &call, &prelude) else {
                    continue;
                };
                // A macro Rawr writes itself, by its name, is the lowering's.
                if crate::resolve::tokio_macro(&path, host).is_some() {
                    continue;
                }
                let name = call.path.last().cloned().unwrap_or_default();
                let rules = host
                    .overloads(&path)
                    .filter(|items| {
                        !items
                            .iter()
                            .any(|item| item.kind() == host::item::Kind::IntrinsicMacro)
                    })
                    .and_then(|items| {
                        items
                            .iter()
                            .find_map(host::item::Definition::macro_rules_source)
                    });
                #[cfg(feature = "rust-import")]
                let kept: (String, Vec<Option<u32>>);
                let expansion = match rules {
                    Some(rules) => {
                        let Ok(parsed_rules) = rules.rules() else {
                            continue;
                        };
                        Expansion::Rules {
                            rules: parsed_rules,
                            krate: rules.krate(),
                        }
                    }
                    // An expression's proc macro is the lowering's to ask
                    // rustc for in its place.
                    #[cfg(feature = "rust-import")]
                    None if !call.expression && prelude.crate_macro(host, &path) => {
                        match expansion_kept(parsed, &call, &path, &project, &mut self.named) {
                            Ok(text) => kept = text,
                            // A block's call is the lowering's to ask for,
                            // with the body's other calls (#2151).
                            Err(_) if call.scope.is_some() => continue,
                            // rustc expands every module's call the graph
                            // holds in one run.
                            Err(missed) => {
                                if let crate::resolve::Error::MacroExpansionRequired {
                                    call, ..
                                } = &missed
                                {
                                    self.unexpanded.push(call.clone());
                                }
                                unexpanded.get_or_insert(Failure {
                                    primary: source,
                                    secondary: None,
                                    error: missed,
                                });
                                continue;
                            }
                        }
                        Expansion::Text {
                            text: &kept.0,
                            locations: &kept.1,
                        }
                    }
                    None => continue,
                };
                let first = parsed.items().len();
                let nested =
                    crate::parser::item::lay_out_call(parsed, &call, &name, &expansion, &features)
                        .map_err(|error| Failure {
                            primary: source,
                            secondary: None,
                            error: crate::resolve::Error::Syntax {
                                message: error.0.to_string().into(),
                                label: ImmutableString::new(),
                                help: None,
                                at: call.span,
                            },
                        })?;
                if let Some(nested) = nested {
                    laid = true;
                    added.get_or_insert(first);
                    calls.extend(nested);
                }
            }
            if laid {
                let text = self.units[index].parsed.source().text().to_owned();
                let expansions = self.units[index].parsed.expansions().into();
                self.sources[source.index()].expand(&text, expansions);
            }
            if let Some(first) = added {
                self.load_inline_modules(index, first, &features)?;
            }
            index += 1;
        }
        #[cfg(feature = "rust-import")]
        if let Some(failure) = unexpanded {
            return Err(failure);
        }
        Ok(())
    }

    /// Every call of a dependency's proc macro among the modules' items
    /// that [`Self::expand_crate_macros`] found no expansion for, encoded
    /// as `resolve::Error::MacroExpansionRequired` holds it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn unexpanded_calls(&self) -> Vec<Box<[u8]>> {
        self.unexpanded.clone()
    }

    /// Load each inline `mod name { .. }` among the items of the unit at
    /// `index` from its `first` on as a module of the unit's, as loading
    /// does a written one: a source of the text inside its braces, where
    /// the unit's text has it, parsed with `features` enabled, and the
    /// inline modules among its own items in turn.
    fn load_inline_modules(
        &mut self,
        index: usize,
        first: usize,
        features: &crate::project::cargo::features::Enabled,
    ) -> Result<(), Failure> {
        let parent = self.units[index].source;
        let items = self.units[index].parsed.items();
        // A block's module is named by its segment, as the resolver names
        // it (`resolve::block::module_segment`).
        let ordinals = crate::resolve::block::ordinals(items);
        let declared = items
            .iter()
            .zip(ordinals)
            .skip(first)
            .filter_map(|(item, ordinal)| match item {
                crate::item::Parsed::Module(module) => Some((
                    module.inline?,
                    module.span,
                    crate::resolve::block::module_segment(&module.name.text, ordinal),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (inline, span, segment) in declared {
            let failed = |message: String| Failure {
                primary: parent,
                secondary: None,
                error: crate::resolve::Error::Syntax {
                    message: message.into(),
                    label: ImmutableString::new(),
                    help: None,
                    at: span,
                },
            };
            let around = &self.sources[parent.index()];
            let mut modules = around.modules().to_vec();
            modules.push(segment);
            let id = crate::package::source::Id::from_index(self.sources.len());
            let mut source = crate::package::source::Source::new(
                id,
                around.path().to_path_buf(),
                around.package().into(),
                modules.into_boxed_slice(),
                super::keep_span(around.expanded_text(), inline),
            )
            .map_err(|error| failed(error.to_string()))?;
            let parsed = crate::Engine::new_raw()
                .parse_unit_with(source.text(), features)
                .map_err(|error| failed(error.0.to_string()))?;
            if !parsed.expansions().is_empty() {
                source.expand(parsed.source().text(), parsed.expansions().into());
            }
            self.sources.push(source);
            self.units.push(super::Unit { source: id, parsed });
            self.load_inline_modules(self.units.len() - 1, 0, features)?;
        }
        Ok(())
    }
}

/// The path from its crate's root of the dependency's macro `call` names,
/// through the `use` that binds its first name: `None` for a path that
/// starts at no dependency.
fn crate_path(
    parsed: &crate::parse::Unit,
    call: &MacroCall,
    prelude: &crate::resolve::extern_prelude::Prelude,
) -> Option<String> {
    let (first, rest) = call.path.split_first()?;
    let at = call.name.start();
    let bound = parsed
        .imports()
        .iter()
        .filter(|used| {
            used.scope
                .is_none_or(|scope| scope.start() <= at && at < scope.end())
        })
        .flat_map(crate::item::Use::bindings)
        .rfind(|(name, _)| name.text == *first)
        .map(|(_, path)| path);
    let mut segments: Vec<ImmutableString> = bound.map_or_else(|| vec![first.clone()], Vec::from);
    segments.extend(rest.iter().cloned());
    let root = segments.first()?;
    (segments.len() > 1 && prelude.dependency(root)).then(|| {
        segments
            .iter()
            .map(ImmutableString::as_str)
            .collect::<Vec<_>>()
            .join("::")
    })
}

/// rustc's expansion of `call`, the dependency's proc macro at `path`, as
/// the project at `project` keeps it (`rawr_import::expand::call`), in the
/// place a block's statement or a module's item is: rustc expands both as
/// a body's statements, whose items are items, named in `named` as it is
/// read, with where each location call in it reads
/// (`rawr_import::expand::call::Location`). The error that asks rustc for
/// it when it has none yet.
#[cfg(feature = "rust-import")]
fn expansion_kept(
    parsed: &crate::parse::Unit,
    call: &MacroCall,
    path: &str,
    project: &std::path::Path,
    named: &mut rawr_import::expand::kept::Named,
) -> Result<(String, Vec<Option<u32>>), crate::resolve::Error> {
    use rawr_import::expand::call as expand;
    let tokens = crate::resolve::lower_nominal::lex_rust(
        &crate::Engine::new_raw(),
        &parsed.source().only([call.group]),
    );
    let tokens = tokens
        .iter()
        .filter(|(token, _)| *token != crate::tokenizer::Token::EOF)
        .collect::<Vec<_>>();
    let delimiter = match tokens.first().map(|(token, _)| token) {
        Some(crate::tokenizer::Token::LeftBracket) => expand::Delimiter::Bracket,
        Some(crate::tokenizer::Token::LeftBrace | crate::tokenizer::Token::MapStart) => {
            expand::Delimiter::Brace
        }
        _ => expand::Delimiter::Parenthesis,
    };
    let inner = tokens
        .get(1..tokens.len().saturating_sub(1))
        .unwrap_or_default()
        .iter()
        .map(|token| (*token).clone())
        .collect::<Vec<_>>();
    let call_of = expand::Call {
        path: path.to_owned(),
        delimiter,
        input: crate::resolve::lower_nominal::token_text(&inner),
        place: expand::Place::Statements,
    };
    let source = call_of.source_path(project);
    let text = std::fs::read_to_string(&source).ok();
    let kept = text
        .as_deref()
        .and_then(|text| Some((expand::kept(text)?, expand::locations(text)?)));
    match kept {
        Some((expansion, locations)) => {
            named.name(&source);
            Ok((expansion.to_owned(), locations))
        }
        None => Err(crate::resolve::Error::MacroExpansionRequired {
            path: path.into(),
            call: serde_json::to_vec(&call_of).unwrap_or_default().into(),
            reference: call.name,
        }),
    }
}
