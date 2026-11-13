//! Rewriting a compiled body's syntax tree: positions moved from the
//! lowered text to the script's, and calls renamed to the runtime's.

use super::naming::{NATIVE_ONLY, SENTINEL};
use crate::ast::{
    BinaryExpr, FlowControl, FnCallExpr, FnCallHashes, NominalPattern, Stmt, NOMINAL_MATCH_SENTINEL,
};
use crate::{ast, calc_fn_hash, Dynamic, ImmutableString, Position, AST};
use std::collections::BTreeMap;

/// Move every position in `ast`'s statements and functions by `locate`:
/// from the lowered program to the script's text.
pub(crate) fn relocate(ast: &mut AST, locate: &mut dyn FnMut(Position) -> Position) {
    relocate_statements(ast.statements_mut(), locate);
    ast.for_each_fn_def_mut(|function| {
        relocate_statements(function.body.statements_mut(), locate);
    });
}

pub(super) fn relocate_statements(
    statements: &mut [Stmt],
    locate: &mut dyn FnMut(Position) -> Position,
) {
    for statement in statements {
        relocate_statement(statement, locate);
    }
}

pub(super) fn relocate_block(
    block: &mut ast::StmtBlock,
    locate: &mut dyn FnMut(Position) -> Position,
) {
    let span = block.span();
    block.set_position(locate(span.start()), locate(span.end()));
    relocate_statements(block.statements_mut(), locate);
}

pub(super) fn relocate_statement(
    statement: &mut Stmt,
    locate: &mut dyn FnMut(Position) -> Position,
) {
    // An expression statement's or a block's position is its expression's
    // or its block's, which are moved below.
    if !matches!(statement, Stmt::Expr(..) | Stmt::Block(..)) {
        let position = locate(statement.position());
        statement.set_position(position);
    }
    match statement {
        Stmt::Var(value, ..) => {
            value.0.pos = locate(value.0.pos);
            relocate_expr(&mut value.1, locate);
        }
        Stmt::Assignment(value) => {
            relocate_expr(&mut value.1.lhs, locate);
            relocate_expr(&mut value.1.rhs, locate);
        }
        Stmt::FnCall(call, ..) => relocate_call(call, locate),
        Stmt::Expr(expr) => relocate_expr(expr, locate),
        Stmt::Block(block) => relocate_block(block, locate),
        Stmt::If(flow, ..) | Stmt::While(flow, ..) => {
            relocate_expr(&mut flow.expr, locate);
            relocate_block(&mut flow.body, locate);
            relocate_block(&mut flow.branch, locate);
        }
        Stmt::For(value, ..) => {
            value.0.pos = locate(value.0.pos);
            if let Some(counter) = &mut value.1 {
                counter.pos = locate(counter.pos);
            }
            relocate_expr(&mut value.2.expr, locate);
            relocate_block(&mut value.2.body, locate);
        }
        Stmt::Switch(value, ..) => {
            relocate_expr(&mut value.0, locate);
            for expression in &mut value.1.expressions {
                relocate_expr(&mut expression.lhs, locate);
                relocate_expr(&mut expression.rhs, locate);
            }
        }
        Stmt::Return(expr, ..) | Stmt::BreakLoop(expr, ..) => {
            if let Some(expr) = expr {
                relocate_expr(expr, locate);
            }
        }
        _ => {}
    }
}

pub(super) fn relocate_call(call: &mut FnCallExpr, locate: &mut dyn FnMut(Position) -> Position) {
    for arg in &mut call.args {
        relocate_expr(arg, locate);
    }
}

pub(super) fn relocate_expr(expr: &mut ast::Expr, locate: &mut dyn FnMut(Position) -> Position) {
    if !matches!(expr, ast::Expr::Stmt(..)) {
        let position = locate(expr.position());
        expr.set_position(position);
    }
    match expr {
        ast::Expr::FnCall(call, ..) | ast::Expr::MethodCall(call, ..) => {
            relocate_call(call, locate)
        }
        ast::Expr::Array(values, ..) => {
            for value in values {
                relocate_expr(value, locate);
            }
        }
        ast::Expr::And(values, ..) | ast::Expr::Or(values, ..) => {
            for value in values.iter_mut() {
                relocate_expr(value, locate);
            }
        }
        ast::Expr::Map(value, ..) => {
            for (_, value) in &mut value.0 {
                relocate_expr(value, locate);
            }
        }
        ast::Expr::Stmt(block) => relocate_block(block, locate),
        ast::Expr::Dot(binary, ..) | ast::Expr::Index(binary, ..) => {
            relocate_expr(&mut binary.lhs, locate);
            relocate_expr(&mut binary.rhs, locate);
        }
        ast::Expr::DynamicConstant(value, ..) => relocate_constant(value, locate),
        _ => {}
    }
}

/// A folded closure's body, or those of the closures an array holds.
pub(super) fn relocate_constant(value: &mut Dynamic, locate: &mut dyn FnMut(Position) -> Position) {
    if let Some(pointer) = value.downcast_mut::<crate::FnPtr>() {
        if let crate::types::fn_ptr::FnPtrType::Script(definition) = &mut pointer.typ {
            let definition = crate::func::shared_make_mut(definition);
            relocate_statements(definition.body.statements_mut(), locate);
        }
        for value in pointer.curry.iter_mut() {
            relocate_constant(value, locate);
        }
        return;
    }
    if let Some(array) = value.downcast_mut::<crate::Array>() {
        for element in array.iter_mut() {
            relocate_constant(element, locate);
        }
    }
}

pub(super) fn rewrite_statements(
    statements: &mut [Stmt],
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    for statement in statements {
        rewrite_statement(statement, rewrites, patterns);
    }
}

pub(super) fn rewrite_statement(
    statement: &mut Stmt,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    match statement {
        Stmt::Var(value, ..) => {
            rewrite_expr(&mut value.1, rewrites, patterns);
            rewrite_variable(&mut value.0.name, rewrites);
        }
        Stmt::Assignment(value) => {
            rewrite_expr(&mut value.1.lhs, rewrites, patterns);
            rewrite_expr(&mut value.1.rhs, rewrites, patterns);
        }
        Stmt::FnCall(call, ..) => rewrite_call(call, rewrites, patterns, false),
        Stmt::Expr(expr) => rewrite_expr(expr, rewrites, patterns),
        Stmt::Block(block) => rewrite_statements(block.statements_mut(), rewrites, patterns),
        Stmt::If(flow, ..) | Stmt::While(flow, ..) => rewrite_flow(flow, rewrites, patterns),
        Stmt::For(value, ..) => {
            rewrite_expr(&mut value.2.expr, rewrites, patterns);
            rewrite_statements(value.2.body.statements_mut(), rewrites, patterns);
        }
        Stmt::Switch(value, ..) => {
            rewrite_expr(&mut value.0, rewrites, patterns);
            for expression in &mut value.1.expressions {
                rewrite_expr(&mut expression.lhs, rewrites, patterns);
                rewrite_expr(&mut expression.rhs, rewrites, patterns);
            }
            for case in &mut value.1.enum_cases {
                if case.enum_name.starts_with(NOMINAL_MATCH_SENTINEL) {
                    let pattern = patterns
                        .get(case.enum_name.as_str())
                        .expect("compiler match sentinel must carry a nominal pattern proof")
                        .clone();
                    match &pattern {
                        NominalPattern::Enum(pattern) => {
                            case.enum_name = pattern.enum_name.clone();
                            case.variant_name = pattern.variant.name().into();
                            case.variant_index = pattern
                                .variants
                                .iter()
                                .position(|variant| variant == pattern.variant.name())
                                .expect("a nominal pattern variant must belong to its proven enum");
                        }
                        NominalPattern::Option(some) => {
                            case.enum_name = "Option".into();
                            case.variant_name = if some.is_some() { "Some" } else { "None" }.into();
                            case.variant_index = usize::from(some.is_some());
                        }
                        NominalPattern::Result(variant) => {
                            case.enum_name = "Result".into();
                            case.variant_name = if variant.is_ok() { "Ok" } else { "Err" }.into();
                            case.variant_index = usize::from(variant.is_err());
                        }
                        _ => {
                            case.enum_name = "_".into();
                            case.variant_name = "_".into();
                            case.variant_index = usize::MAX;
                        }
                    }
                    case.nominal = Some(pattern);
                }
            }
        }
        Stmt::Return(expr, ..) | Stmt::BreakLoop(expr, ..) => {
            if let Some(expr) = expr {
                rewrite_expr(expr, rewrites, patterns);
            }
        }
        Stmt::Noop(..) => {}
        Stmt::Share(..) => {}
    }
}

pub(super) fn rewrite_flow(
    flow: &mut FlowControl,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    rewrite_expr(&mut flow.expr, rewrites, patterns);
    rewrite_statements(flow.body.statements_mut(), rewrites, patterns);
    rewrite_statements(flow.branch.statements_mut(), rewrites, patterns);
}

pub(super) fn rewrite_expr(
    expr: &mut ast::Expr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    match expr {
        ast::Expr::FnCall(call, ..) => rewrite_call(call, rewrites, patterns, false),
        ast::Expr::MethodCall(call, ..) => rewrite_call(call, rewrites, patterns, true),
        ast::Expr::Array(values, ..) => {
            for value in values {
                rewrite_expr(value, rewrites, patterns);
            }
        }
        ast::Expr::And(values, ..) | ast::Expr::Or(values, ..) => {
            for value in values.iter_mut() {
                rewrite_expr(value, rewrites, patterns);
            }
        }
        ast::Expr::Map(value, ..) => {
            for (_, value) in &mut value.0 {
                rewrite_expr(value, rewrites, patterns);
            }
        }
        ast::Expr::Stmt(block) => rewrite_statements(block.statements_mut(), rewrites, patterns),
        ast::Expr::Dot(binary, ..) | ast::Expr::Index(binary, ..) => {
            rewrite_binary(binary, rewrites, patterns);
        }
        ast::Expr::Variable(variable, ..) => {
            rewrite_variable(&mut variable.1, rewrites);
        }
        // A closure carries its own function definition.
        ast::Expr::DynamicConstant(value, ..) => rewrite_constant(value, rewrites, patterns),
        ast::Expr::BoolConstant(..)
        | ast::Expr::IntegerConstant(..)
        | ast::Expr::CharConstant(..)
        | ast::Expr::StringConstant(..)
        | ast::Expr::Unit(..)
        | ast::Expr::ThisPtr(..)
        | ast::Expr::Property(..) => {}
        ast::Expr::FloatConstant(..) => {}
    }
}

/// A constant the optimizer folded: a closure, or an array holding one
/// (a `fn` pointer choice is `[index, closure]`).
pub(super) fn rewrite_constant(
    value: &mut Dynamic,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    if let Some(pointer) = value.downcast_mut::<crate::FnPtr>() {
        rewrite_pointer(pointer, rewrites, patterns);
        return;
    }
    if let Some(array) = value.downcast_mut::<crate::Array>() {
        for element in array.iter_mut() {
            rewrite_constant(element, rewrites, patterns);
        }
    }
}

/// A closure's definition, and those of the closures it captured, which
/// the optimizer folds into its curried values.
pub(super) fn rewrite_pointer(
    pointer: &mut crate::FnPtr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    if let crate::types::fn_ptr::FnPtrType::Script(definition) = &mut pointer.typ {
        let definition = crate::func::shared_make_mut(definition);
        rewrite_statements(definition.body.statements_mut(), rewrites, patterns);
    }
    for value in pointer.curry.iter_mut() {
        if let Some(captured) = value.downcast_mut::<crate::FnPtr>() {
            rewrite_pointer(captured, rewrites, patterns);
        }
    }
}

pub(super) fn rewrite_variable(
    name: &mut ImmutableString,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
) {
    let Some((helper, argument)) = name.rsplit_once("_arg_") else {
        return;
    };
    if !helper.starts_with(SENTINEL) || argument.parse::<usize>().is_err() {
        return;
    }
    let Some(private) = rewrites.get(helper) else {
        return;
    };
    *name = format!("{private}.arg.{argument}").into();
}

pub(super) fn rewrite_binary(
    binary: &mut BinaryExpr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
) {
    rewrite_expr(&mut binary.lhs, rewrites, patterns);
    rewrite_expr(&mut binary.rhs, rewrites, patterns);
}

pub(super) fn rewrite_call(
    call: &mut FnCallExpr,
    rewrites: &BTreeMap<ImmutableString, ImmutableString>,
    patterns: &BTreeMap<ImmutableString, NominalPattern>,
    is_method: bool,
) {
    for arg in &mut call.args {
        rewrite_expr(arg, rewrites, patterns);
    }
    if let Some(name) = rewrites.get(&call.name)
        && name.as_str() == NATIVE_ONLY
    {
        let arguments = call.args.len() + usize::from(is_method);
        call.hashes = FnCallHashes::from_native_only(calc_fn_hash(None, &call.name, arguments));
    } else if let Some(name) = rewrites.get(&call.name) {
        call.name = name.clone();
        call.hashes = if is_method {
            {
                FnCallHashes::from_script_and_native(
                    calc_fn_hash(None, name, call.args.len()),
                    calc_fn_hash(None, name, call.args.len() + 1),
                )
            }
        } else {
            FnCallHashes::from_hash(calc_fn_hash(None, name, call.args.len()))
        };
    }
}
