//! A call of a dependency's macro where an item goes, among a module's
//! items or a block's statements: rustc expands it before it resolves a
//! name, and the items it expands to are that module's or that block's.
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
    /// Expand each call of a dependency's macro where an item goes, now
    /// that `host` holds the dependencies' facades, and lay out the items
    /// each expands to where it is.
    ///
    /// # Errors
    ///
    /// A module's call of a proc macro rustc has not expanded yet, which it
    /// expands before the program is checked again (#1125), and a module's
    /// call whose expansion does not parse as items.
    pub(crate) fn expand_crate_macros(&mut self, host: &host::Registry) -> Result<(), Failure> {
        if self.hosted {
            return Ok(());
        }
        let project = self.entry_package().root().to_path_buf();
        for index in 0..self.units.len() {
            let source = self.units[index].source;
            let Some(package) = self
                .packages
                .iter()
                .find(|package| package.name() == self.sources[source.index()].package())
            else {
                continue;
            };
            let prelude = crate::resolve::extern_prelude::Prelude::of(package, &project);
            let features = package.features().clone();
            let parsed = &mut self.units[index].parsed;
            let mut calls = parsed.take_calls();
            let mut laid = false;
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
                let kept: String;
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
                    #[cfg(feature = "rust-import")]
                    None if prelude.crate_macro(host, &path) => {
                        match expansion_kept(parsed, &call, &path, &project) {
                            Ok(text) => kept = text,
                            // A block's call is the lowering's to ask for,
                            // with the body's other calls (#2151).
                            Err(_) if call.scope.is_some() => continue,
                            Err(missed) => {
                                return Err(Failure {
                                    primary: source,
                                    secondary: None,
                                    error: missed,
                                });
                            }
                        }
                        Expansion::Text(&kept)
                    }
                    None => continue,
                };
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
                laid |= nested.is_some();
                calls.extend(nested.into_iter().flatten());
            }
            if laid {
                let text = self.units[index].parsed.source().text().to_owned();
                let expansions = self.units[index].parsed.expansions().into();
                self.sources[source.index()].expand(&text, expansions);
            }
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
/// a body's statements, whose items are items. The error that asks rustc
/// for it when it has none yet.
#[cfg(feature = "rust-import")]
fn expansion_kept(
    parsed: &crate::parse::Unit,
    call: &MacroCall,
    path: &str,
    project: &std::path::Path,
) -> Result<String, crate::resolve::Error> {
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
        path: host::path::visible(path),
        delimiter,
        input: crate::resolve::lower_nominal::token_text(&inner),
        place: expand::Place::Statements,
    };
    let source = call_of.source_path(project);
    let text = std::fs::read_to_string(&source).ok();
    match text.as_deref().and_then(expand::kept) {
        Some(expansion) => {
            rawr_import::expand::kept::name(&source);
            Ok(expansion.to_owned())
        }
        None => Err(crate::resolve::Error::MacroExpansionRequired {
            path: path.into(),
            call: serde_json::to_vec(&call_of).unwrap_or_default().into(),
            reference: call.name,
        }),
    }
}
