//! `--expand`: the items a crate's derive macro writes for a program's type
//! (#593). rustc expands a small crate of the item and the `use`s that name
//! its derives, and each item a derive produced is written as source text.
//! Expansion runs before names resolve, so the item's field types need not
//! resolve in that crate: only the derives must.

use std::path::{Path, PathBuf};

use rustc_driver::{Callbacks, Compilation};
use rustc_middle::ty::TyCtxt;
use rustc_span::hygiene::{ExpnKind, MacroKind};

use crate::expand_request::{Expanded, Expansion};

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
/// where the source writes it, into `out`. The crate holds each program
/// item in a module of its own, whose items are read too.
fn write(tcx: TyCtxt<'_>, out: &Path, protocol: u32) -> Result<(), String> {
    let (_, krate) = tcx.resolver_for_lowering();
    let krate = krate.borrow();
    let source_map = tcx.sess.source_map();
    let mut expansions: Vec<Expansion> = Vec::new();
    let mut pending = krate.items.iter().map(|item| &**item).collect::<Vec<_>>();
    while let Some(item) = pending.pop() {
        let data = item.span.ctxt().outer_expn_data();
        let ExpnKind::Macro(MacroKind::Derive, name) = data.kind else {
            if let rustc_ast::ItemKind::Mod(.., rustc_ast::ModKind::Loaded(items, ..)) = &item.kind
            {
                pending.extend(items.iter().map(|item| &**item));
            }
            continue;
        };
        let at = source_map.lookup_byte_offset(data.call_site.lo()).pos.0;
        let text = rustc_ast_pretty::pprust::item_to_string(item);
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
    let bytes = serde_json::to_vec(&Expanded {
        protocol,
        expansions,
    })
    .map_err(|error| error.to_string())?;
    std::fs::write(out, bytes).map_err(|error| format!("writing {}: {error}", out.display()))
}
