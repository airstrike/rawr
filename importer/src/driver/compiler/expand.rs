//! `--expand`: the items a crate's derive macro writes for a program's type
//! (#593), and what a crate's function-like macro called from a program's
//! body expands to (#1125). rustc expands a small crate of the items, with
//! the `use`s that name their derives, and of the calls, each in a function
//! of its own, and each item a derive produced and each call's expansion is
//! written as source text. Expansion runs before names resolve, so the
//! item's field types need not resolve in that crate: only the macros must.

use std::path::{Path, PathBuf};

use rustc_ast::{ItemKind, LocalKind, ModKind, StmtKind};
use rustc_ast_pretty::pprust;
use rustc_driver::{Callbacks, Compilation};
use rustc_middle::ty::TyCtxt;
use rustc_span::hygiene::{ExpnKind, MacroKind};

use crate::expand_request::{Called, Expanded, Expansion};

/// The function whose one `let` a call that expands to an expression
/// initializes (`expand::call`).
const EXPRESSION: &str = "__rawr_expression";
/// The function whose body a call in statement position is.
const STATEMENTS: &str = "__rawr_statements";

pub(crate) struct Expand {
    pub(crate) out: PathBuf,
    pub(crate) protocol: u32,
    pub(crate) result: Option<Result<(), String>>,
}

impl Callbacks for Expand {
    fn after_expansion<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        self.result = Some(write(tcx, &self.out, self.protocol));
        Compilation::Stop
    }
}

/// Each item of the crate a derive produced, grouped by the derive's name
/// where the source writes it, and each call's expansion, at the module
/// that holds it, into `out`. The crate holds each program item and each
/// call in a module of its own, whose items are read too.
fn write(tcx: TyCtxt<'_>, out: &Path, protocol: u32) -> Result<(), String> {
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
                    if let Some(items) = called(function) {
                        calls.push(Called { at: module, items });
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

/// What the call `function` holds expanded to: the initializer of its one
/// `let`, or each of its statements. `None` for a function that holds no
/// call.
fn called(function: &rustc_ast::Fn) -> Option<Vec<String>> {
    let body = function.body.as_deref()?;
    match function.ident.as_str() {
        EXPRESSION => {
            let [statement] = body.stmts.as_slice() else {
                return None;
            };
            let StmtKind::Let(local) = &statement.kind else {
                return None;
            };
            let LocalKind::Init(expression) = &local.kind else {
                return None;
            };
            Some(vec![pprust::expr_to_string(expression)])
        }
        STATEMENTS => Some(
            body.stmts
                .iter()
                .filter(|statement| !matches!(statement.kind, StmtKind::Empty))
                .map(pprust::stmt_to_string)
                .collect(),
        ),
        _ => None,
    }
}
