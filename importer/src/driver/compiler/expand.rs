//! `--expand`: the items a crate's derive macro writes for a program's type
//! (#593), and what a crate's function-like macro called from a program's
//! body expands to (#1125). rustc expands a small crate of the items, with
//! the `use`s that name their derives, and of the calls, each in a function
//! of its own, and each item a derive produced and each call's expansion is
//! written as source text. Expansion runs before names resolve, so the
//! item's field types need not resolve in that crate: only the macros must.

use std::path::{Path, PathBuf};

use rustc_ast::mut_visit::{self, MutVisitor};
use rustc_ast::token::{self, Delimiter};
use rustc_ast::tokenstream::{DelimSpan, TokenStream};
use rustc_ast::{DelimArgs, Expr, ExprKind, ItemKind, LocalKind, MacCall, ModKind, StmtKind};
use rustc_ast_pretty::pprust;
use rustc_driver::{Callbacks, Compilation};
use rustc_metadata::creader::{CStore, LoadedMacro};
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::DefId;
use rustc_span::hygiene::{ExpnKind, MacroKind};
use rustc_span::source_map::SourceMap;
use rustc_span::{sym, Ident, Symbol};

use crate::expand_request::{Called, Expanded, Expansion};

/// The function whose one `let` a call that expands to an expression
/// initializes (`expand::call`).
const EXPRESSION: &str = "__rawr_expression";
/// The function whose body a call in statement position is.
const STATEMENTS: &str = "__rawr_statements";
/// The function whose one `let` a call in type position is the type of.
const TYPE: &str = "__rawr_type";
/// The function whose one `let` a call in pattern position is the
/// pattern of.
const PATTERN: &str = "__rawr_pattern";

pub(crate) struct Expand {
    pub(crate) out: PathBuf,
    pub(crate) protocol: u32,
    /// The small crate's source, as `file!()` names it there.
    pub(crate) source: String,
    pub(crate) result: Option<Result<(), String>>,
}

impl Callbacks for Expand {
    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        self.result = Some(write(tcx, &self.out, self.protocol, &self.source));
        Compilation::Stop
    }
}

/// Each item of the crate a derive produced, grouped by the derive's name
/// where the source writes it, and each call's expansion, at the module
/// that holds it, into `out`. The crate holds each program item and each
/// call in a module of its own, whose items are read too.
fn write(tcx: TyCtxt<'_>, out: &Path, protocol: u32, source: &str) -> Result<(), String> {
    let (_, krate) = tcx.resolver_for_lowering();
    let krate = krate.borrow();
    let source_map = tcx.sess.source_map();
    let mut expansions: Vec<Expansion> = Vec::new();
    let mut calls: Vec<Called> = Vec::new();
    // Each item with the offset of the module that holds it.
    let mut pending = krate
        .items
        .iter()
        .map(|item| (&**item, 0))
        .collect::<Vec<_>>();
    while let Some((item, module)) = pending.pop() {
        let data = item.span.ctxt().outer_expn_data();
        let ExpnKind::Macro(MacroKind::Derive, name) = data.kind else {
            match &item.kind {
                ItemKind::Mod(.., ModKind::Loaded(items, ..)) => {
                    let at = source_map.lookup_byte_offset(item.span.lo()).pos.0;
                    pending.extend(items.iter().map(|item| (&**item, at)));
                }
                ItemKind::Fn(function) => {
                    let mut locations = Locations {
                        source_map,
                        source,
                        found: Vec::new(),
                    };
                    if let Some(items) = called(function, &mut locations) {
                        calls.push(Called {
                            at: module,
                            items,
                            locations: locations.found,
                        });
                    }
                }
                _ => {}
            }
            continue;
        };
        let at = source_map.lookup_byte_offset(data.call_site.lo()).pos.0;
        let text = pprust::item_to_string(item);
        let derive = name.to_string();
        match expansions
            .iter_mut()
            .find(|expansion| expansion.at == at && expansion.derive == derive)
        {
            Some(expansion) => expansion.items.push(text),
            None => expansions.push(Expansion {
                derive,
                at,
                items: vec![text],
                helpers: data
                    .macro_def_id
                    .map(|macro_| helpers(tcx, macro_))
                    .unwrap_or_default(),
            }),
        }
    }
    // Each derive's items in the order rustc wrote them, which the stack
    // above reads backwards.
    for expansion in &mut expansions {
        expansion.items.reverse();
    }
    expansions.sort_by_key(|expansion| expansion.at);
    calls.sort_by_key(|call| call.at);
    let bytes = serde_json::to_vec(&Expanded {
        protocol,
        expansions,
        calls,
    })
    .map_err(|error| error.to_string())?;
    std::fs::write(out, bytes).map_err(|error| format!("writing {}: {error}", out.display()))
}

/// The helper attributes the crate derive `macro_` declares, as its
/// `#[proc_macro_derive(Name, attributes(..))]` names them: the attributes
/// an item that derives it, and its members, may carry. A derive of no
/// proc-macro crate, as std's are, declares none here.
fn helpers(tcx: TyCtxt<'_>, macro_: DefId) -> Vec<String> {
    if macro_.is_local() {
        return Vec::new();
    }
    match CStore::from_tcx(tcx).load_macro_untracked(tcx, macro_) {
        LoadedMacro::ProcMacro(extension) => extension
            .helper_attrs
            .iter()
            .map(ToString::to_string)
            .collect(),
        LoadedMacro::MacroDef { .. } => Vec::new(),
    }
}

/// What the call `function` holds expanded to: the initializer, type or
/// pattern of its one `let`, or each of its statements, with each value a
/// location macro gave written as that macro's call again, which
/// `locations` records the place of. `None` for a function that holds no
/// call.
fn called(function: &rustc_ast::Fn, locations: &mut Locations<'_>) -> Option<Vec<String>> {
    let body = function.body.as_deref()?;
    let local = || {
        let [statement] = body.stmts.as_slice() else {
            return None;
        };
        match &statement.kind {
            StmtKind::Let(local) => Some(local),
            _ => None,
        }
    };
    match function.ident.as_str() {
        EXPRESSION => {
            let LocalKind::Init(expression) = &local()?.kind else {
                return None;
            };
            let mut expression = expression.clone();
            locations.visit_expr(&mut expression);
            Some(vec![pprust::expr_to_string(&expression)])
        }
        TYPE => {
            let mut ty = local()?.ty.clone()?;
            locations.visit_ty(&mut ty);
            Some(vec![pprust::ty_to_string(&ty)])
        }
        PATTERN => {
            let mut pattern = local()?.pat.clone();
            locations.visit_pat(&mut pattern);
            Some(vec![pprust::pat_to_string(&pattern)])
        }
        STATEMENTS => Some(
            body.stmts
                .iter()
                .filter(|statement| !matches!(statement.kind, StmtKind::Empty))
                .flat_map(|statement| locations.flat_map_stmt(statement.clone()))
                .map(|statement| pprust::stmt_to_string(&statement))
                .collect(),
        ),
        _ => None,
    }
}

/// Where each value `line!()`, `column!()` and `file!()` gave in a call's
/// expansion came from (#2291). rustc writes such a value as a literal at
/// the place it reads: the outermost call the source writes, or a call the
/// call's input holds. Each is written as its macro's call again, so the
/// program reads it where the program writes the call, and `found` holds
/// the byte offset of each place in the source, in the order they print.
struct Locations<'a> {
    source_map: &'a SourceMap,
    /// The small crate's source, as `file!()` names it.
    source: &'a str,
    found: Vec<u32>,
}

impl Locations<'_> {
    /// The location macro whose value `expression` is: a literal whose
    /// span is in the source as written, whose text there is not the
    /// literal's own, and whose value is that macro's at the span's start.
    /// The library lays every call out on lines numbered past any column.
    fn location(&self, expression: &Expr) -> Option<Symbol> {
        let ExprKind::Lit(literal) = &expression.kind else {
            return None;
        };
        let span = expression.span;
        if !span.ctxt().is_root()
            || self.source_map.span_to_snippet(span).ok()? == literal.to_string()
        {
            return None;
        }
        let at = self.source_map.lookup_char_pos(span.lo());
        match literal.kind {
            token::LitKind::Integer if literal.suffix == Some(sym::u32) => {
                let value = literal.symbol.as_str().parse::<usize>().ok()?;
                if value == at.line {
                    Some(sym::line)
                } else if value == at.col.0 + 1 {
                    Some(sym::column)
                } else {
                    None
                }
            }
            token::LitKind::Str => (literal.symbol.as_str() == self.source).then_some(sym::file),
            _ => None,
        }
    }
}

impl MutVisitor for Locations<'_> {
    fn visit_expr(&mut self, expression: &mut Expr) {
        let Some(name) = self.location(expression) else {
            mut_visit::walk_expr(self, expression);
            return;
        };
        let span = expression.span;
        self.found
            .push(self.source_map.lookup_byte_offset(span.lo()).pos.0);
        expression.kind = ExprKind::MacCall(Box::new(MacCall {
            path: rustc_ast::Path::from_ident(Ident::new(name, span)),
            args: Box::new(DelimArgs {
                dspan: DelimSpan::from_single(span),
                delim: Delimiter::Parenthesis,
                tokens: TokenStream::new(Vec::new()),
            }),
        }));
    }
}
