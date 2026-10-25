//! Async bodies as stackless state machines, the way rustc lowers them.
//!
//! The compiler renders each `.await` in an async body as a call of
//! [`AWAIT`]. After the body compiles, [`lower`] splits it at those calls
//! into blocks of ordinary statements, each ending in a jump, a branch, an
//! await or a return. The evaluator runs a block's statements as it runs any
//! statement, in the function's own [`Scope`]. At an await it returns
//! [`Poll::Pending`] with the awaited value and a [`Frame`] that owns the
//! function's locals and the position to resume at. The host awaits the
//! value and calls the function again with the frame, which carries the
//! awaited output. The body goes on from that position. Work already done is
//! never done again, and nothing is replayed.
//!
//! Locals live in the scope under their names, as in a synchronous call.
//! Blocks that Rust scopes, such as an `if` arm or a loop body, record the
//! scope's length when they are entered, and a jump out of them rewinds to
//! it. Values an expression computed before an await are kept in hidden
//! locals named `__rawr_async_<n>`, so they are computed once.

use crate::ast::{ASTFlags, BinaryExpr, Expr, FlowControl, Ident, ScriptFuncDef, Stmt, StmtBlock};
use crate::eval::{Caches, GlobalRuntimeState};
use crate::types::dynamic::AccessMode;
use crate::{Dynamic, Engine, EvalAltResult, ImmutableString, Position, RawrResult, Scope, INT};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The function an async body's `.await` is rendered as.
pub(crate) const AWAIT: &str = "__rawr_await";

/// The prefix of the hidden locals the lowering introduces.
const TEMPORARY: &str = "__rawr_async_";

/// An async body lowered into blocks.
pub(crate) struct Machine {
    blocks: Box<[Block]>,
    loops: Box<[Loop]>,
}

impl fmt::Debug for Machine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Machine({} blocks)", self.blocks.len())
    }
}

/// Straight-line statements and how the block ends.
struct Block {
    /// Whether entering the block opens a scope that a jump out rewinds.
    enter: bool,
    ops: Vec<Op>,
    term: Term,
    /// The innermost lowered loop around the block, which a `break` or
    /// `continue` raised by its statements targets.
    within: Option<usize>,
}

/// A loop whose body awaits.
struct Loop {
    exit: Jump,
    next: Jump,
    /// Where a `break value` puts its value.
    value: Option<ImmutableString>,
}

/// A transfer to `target` with `depth` scopes open.
#[derive(Clone, Copy)]
struct Jump {
    target: usize,
    depth: usize,
}

enum Op {
    /// Evaluate a statement in place, as a block's statement.
    Run(Stmt),
    /// Push a local holding a statement's value.
    Let(ImmutableString, Stmt),
    /// Store a statement's value in an existing local.
    Set(ImmutableString, Stmt),
    /// Push a local holding an iterator over an expression's value.
    Cursor(ImmutableString, Expr),
    /// Push a local holding element `index` of the array in `from`.
    Bind {
        from: ImmutableString,
        index: usize,
        name: ImmutableString,
    },
    /// Store element `index` of the array in `from` in an existing local.
    Assign {
        from: ImmutableString,
        index: usize,
        name: ImmutableString,
    },
}

enum Term {
    Goto(Jump),
    If(Expr, Jump, Jump),
    /// Suspend with the expression's value. The output arrives in a new
    /// local, and the body goes on at the jump.
    Await(Expr, ImmutableString, Jump),
    Return(Expr),
    /// Take the next item from a cursor: into a new local in the body, or
    /// out to `done`.
    Next {
        cursor: ImmutableString,
        name: ImmutableString,
        body: Jump,
        done: Jump,
    },
    /// Go to the target the first element of the array in a local selects.
    Table(ImmutableString, Box<[Jump]>),
}

/// The body could not be lowered.
#[derive(Debug)]
pub(crate) struct Unsupported(pub &'static str, pub Position);

/// Lower `function` when its body awaits. Its last parameter is the resume
/// value: `()` to start, or a [`Frame`].
pub(crate) fn lower(function: &mut ScriptFuncDef) -> Result<(), Unsupported> {
    if !function.body.iter().any(stmt_awaits) || function.params.is_empty() {
        return Ok(());
    }
    let mut builder = Builder {
        blocks: vec![Block::new(false, None)],
        loops: Vec::new(),
        current: 0,
        depth: 0,
        within: None,
        temporaries: 0,
    };
    let mut body = function.body.statements().to_vec();
    body.iter_mut().for_each(forget_indices);
    let output = builder.temporary();
    builder.push(Op::Let(output.clone(), unit_stmt()));
    builder.statements_into(body, Some(&output))?;
    builder.finish(Term::Return(variable(&output)));
    function.machine = Some(crate::Shared::new(Machine {
        blocks: builder.blocks.into_boxed_slice(),
        loops: builder.loops.into_boxed_slice(),
    }));
    Ok(())
}

impl Block {
    fn new(enter: bool, within: Option<usize>) -> Self {
        Self {
            enter,
            ops: Vec::new(),
            term: Term::Return(Expr::Unit(Position::NONE)),
            within,
        }
    }
}

struct Builder {
    blocks: Vec<Block>,
    loops: Vec<Loop>,
    current: usize,
    depth: usize,
    within: Option<usize>,
    temporaries: usize,
}

impl Builder {
    fn temporary(&mut self) -> ImmutableString {
        self.temporaries += 1;
        format!("{TEMPORARY}{}", self.temporaries).into()
    }

    fn push(&mut self, op: Op) {
        self.blocks[self.current].ops.push(op);
    }

    fn block(&mut self, enter: bool) -> usize {
        self.blocks.push(Block::new(enter, self.within));
        self.blocks.len() - 1
    }

    fn jump(&self, target: usize) -> Jump {
        Jump {
            target,
            depth: self.depth,
        }
    }

    fn finish(&mut self, term: Term) {
        self.blocks[self.current].term = term;
    }

    /// Continue in `block`, entered from depth `depth`.
    fn at(&mut self, block: usize, depth: usize) {
        self.current = block;
        self.depth = depth + usize::from(self.blocks[block].enter);
    }

    fn statements_into(
        &mut self,
        statements: Vec<Stmt>,
        value: Option<&ImmutableString>,
    ) -> Result<(), Unsupported> {
        let last = statements.len().saturating_sub(1);
        for (index, statement) in statements.into_iter().enumerate() {
            if index == last {
                self.statement(statement, value)?;
            } else {
                self.statement(statement, None)?;
            }
        }
        Ok(())
    }

    /// Lower a statement; with `value`, store its value there.
    fn statement(
        &mut self,
        statement: Stmt,
        value: Option<&ImmutableString>,
    ) -> Result<(), Unsupported> {
        if !stmt_awaits(&statement) {
            self.push(match value {
                Some(name) => Op::Set(name.clone(), statement),
                None => Op::Run(statement),
            });
            return Ok(());
        }
        match statement {
            Stmt::Expr(expr) => {
                let expr = self.expr(*expr)?;
                self.push(store(value, Stmt::Expr(expr.into())));
            }
            Stmt::FnCall(call, position) => {
                let expr = self.expr(Expr::FnCall(call, position))?;
                self.push(store(value, Stmt::Expr(expr.into())));
            }
            Stmt::Var(var, flags, position) => {
                let (name, expr, _) = *var;
                let expr = self.expr(expr)?;
                self.push(Op::Run(Stmt::Var(
                    (name, expr, None).into(),
                    flags,
                    position,
                )));
            }
            Stmt::Assignment(assignment) => {
                let (op, BinaryExpr { lhs, rhs }) = *assignment;
                if expr_awaits(&lhs) {
                    return Err(Unsupported(
                        "`.await` in the place an assignment writes",
                        lhs.position(),
                    ));
                }
                let rhs = self.expr(rhs)?;
                self.push(Op::Run(Stmt::Assignment(
                    (op, BinaryExpr { lhs, rhs }).into(),
                )));
            }
            Stmt::Block(block) => self.scoped(block.statements().to_vec(), value)?,
            Stmt::If(flow, _) => {
                let FlowControl { expr, body, branch } = *flow;
                self.branch(expr, body, branch, value)?;
            }
            Stmt::While(flow, _) => self.while_loop(*flow, value)?,
            Stmt::For(for_loop, position) => {
                let (name, counter, flow) = *for_loop;
                if counter.is_some() {
                    return Err(Unsupported("a counted `for` loop that awaits", position));
                }
                self.for_loop(name, flow)?;
            }
            Stmt::Switch(switch, position) => self.switch(*switch, value, position)?,
            Stmt::Return(Some(expr), flags, position) => {
                let expr = self.expr(*expr)?;
                if flags.intersects(ASTFlags::BREAK) {
                    self.push(Op::Run(Stmt::Return(Some(expr.into()), flags, position)));
                } else {
                    self.finish(Term::Return(expr));
                    // What follows a `return` is unreachable.
                    let next = self.block(false);
                    self.at(next, self.depth);
                }
            }
            Stmt::BreakLoop(Some(expr), flags, position) => {
                let expr = self.expr(*expr)?;
                self.push(Op::Run(Stmt::BreakLoop(Some(expr.into()), flags, position)));
            }
            statement => {
                return Err(Unsupported(
                    "`.await` in this statement",
                    statement.position(),
                ))
            }
        }
        Ok(())
    }

    /// A block Rust scopes: entered, run, and left for the code after it.
    fn scoped(
        &mut self,
        statements: Vec<Stmt>,
        value: Option<&ImmutableString>,
    ) -> Result<(), Unsupported> {
        let depth = self.depth;
        let inner = self.block(true);
        let after = self.block(false);
        self.finish(Term::Goto(self.jump(inner)));
        self.at(inner, depth);
        self.statements_into(statements, value)?;
        self.finish(Term::Goto(Jump {
            target: after,
            depth,
        }));
        self.at(after, depth);
        Ok(())
    }

    fn branch(
        &mut self,
        condition: Expr,
        body: StmtBlock,
        otherwise: StmtBlock,
        value: Option<&ImmutableString>,
    ) -> Result<(), Unsupported> {
        let condition = self.expr(condition)?;
        let depth = self.depth;
        let then = self.block(true);
        let other = self.block(true);
        let after = self.block(false);
        self.finish(Term::If(condition, self.jump(then), self.jump(other)));
        let exit = Jump {
            target: after,
            depth,
        };
        for (block, statements) in [(then, body), (other, otherwise)] {
            self.at(block, depth);
            self.statements_into(statements.statements().to_vec(), value)?;
            self.finish(Term::Goto(exit));
        }
        self.at(after, depth);
        Ok(())
    }

    fn while_loop(
        &mut self,
        flow: FlowControl,
        value: Option<&ImmutableString>,
    ) -> Result<(), Unsupported> {
        let FlowControl { expr, body, .. } = flow;
        let depth = self.depth;
        let head = self.block(false);
        let exit = self.block(false);
        self.finish(Term::Goto(self.jump(head)));
        let outer = self.within;
        self.loops.push(Loop {
            exit: Jump {
                target: exit,
                depth,
            },
            next: Jump {
                target: head,
                depth,
            },
            value: value.cloned(),
        });
        let index = self.loops.len() - 1;
        self.at(head, depth);
        let forever = matches!(expr, Expr::Unit(..) | Expr::BoolConstant(true, ..));
        let condition = if forever {
            None
        } else {
            Some(self.expr(expr)?)
        };
        self.within = Some(index);
        let inner = self.block(true);
        self.within = outer;
        let into = self.jump(inner);
        self.finish(match condition {
            None => Term::Goto(into),
            Some(condition) => Term::If(condition, into, self.jump(exit)),
        });
        self.within = Some(index);
        self.at(inner, depth);
        self.statements_into(body.statements().to_vec(), None)?;
        self.finish(Term::Goto(Jump {
            target: head,
            depth,
        }));
        self.within = outer;
        self.at(exit, depth);
        Ok(())
    }

    fn for_loop(&mut self, name: Ident, flow: FlowControl) -> Result<(), Unsupported> {
        let FlowControl { expr, body, .. } = flow;
        let iterable = self.expr(expr)?;
        let cursor = self.temporary();
        self.push(Op::Cursor(cursor.clone(), iterable));
        let depth = self.depth;
        let head = self.block(false);
        let exit = self.block(false);
        self.finish(Term::Goto(self.jump(head)));
        let outer = self.within;
        self.loops.push(Loop {
            exit: Jump {
                target: exit,
                depth,
            },
            next: Jump {
                target: head,
                depth,
            },
            value: None,
        });
        let index = self.loops.len() - 1;
        self.within = Some(index);
        let inner = self.block(true);
        self.at(head, depth);
        self.blocks[head].within = outer;
        self.finish(Term::Next {
            cursor,
            name: name.name,
            body: self.jump(inner),
            done: self.jump(exit),
        });
        self.at(inner, depth);
        self.statements_into(body.statements().to_vec(), None)?;
        self.finish(Term::Goto(Jump {
            target: head,
            depth,
        }));
        self.within = outer;
        self.at(exit, depth);
        Ok(())
    }

    /// A `switch` whose arms await. The switch itself only selects: each
    /// arm yields `[index, bindings..]`, or `[index, value]` when it does
    /// not await, and the lowered arm runs after it.
    fn switch(
        &mut self,
        switch: (Expr, crate::ast::SwitchCasesCollection),
        value: Option<&ImmutableString>,
        position: Position,
    ) -> Result<(), Unsupported> {
        let (subject, mut cases) = switch;
        let subject = self.expr(subject)?;
        let mut arms = Vec::with_capacity(cases.expressions.len());
        for (index, expression) in cases.expressions.iter_mut().enumerate() {
            if expr_awaits(&expression.lhs) {
                return Err(Unsupported(
                    "`.await` in a match guard",
                    expression.lhs.position(),
                ));
            }
            let at = expression.rhs.position();
            let index_expr = Expr::IntegerConstant(index as INT, at);
            let body = std::mem::take(&mut expression.rhs);
            if expr_awaits(&body) {
                let bindings: Vec<ImmutableString> = cases
                    .enum_cases
                    .iter()
                    .filter(|case| case.index == index)
                    .flat_map(|case| case.bindings.iter().flatten().cloned())
                    .fold(Vec::new(), |mut names, name| {
                        if !names.contains(&name) {
                            names.push(name);
                        }
                        names
                    });
                expression.rhs = Expr::Array(
                    std::iter::once(index_expr)
                        .chain(bindings.iter().map(variable))
                        .collect(),
                    at,
                );
                arms.push((Some(body), bindings));
            } else {
                expression.rhs = Expr::Array([index_expr, body].into_iter().collect(), at);
                arms.push((None, Vec::new()));
            }
        }
        let selected = self.temporary();
        self.push(Op::Let(
            selected.clone(),
            Stmt::Switch((subject, cases).into(), position),
        ));
        let depth = self.depth;
        let after = self.block(false);
        let targets: Vec<usize> = arms.iter().map(|_| self.block(true)).collect();
        self.finish(Term::Table(
            selected.clone(),
            targets.iter().map(|&target| self.jump(target)).collect(),
        ));
        let exit = Jump {
            target: after,
            depth,
        };
        for ((body, bindings), target) in arms.into_iter().zip(targets) {
            self.at(target, depth);
            match body {
                None => {
                    if let Some(name) = value {
                        self.push(Op::Assign {
                            from: selected.clone(),
                            index: 1,
                            name: name.clone(),
                        });
                    }
                }
                Some(body) => {
                    for (offset, name) in bindings.into_iter().enumerate() {
                        self.push(Op::Bind {
                            from: selected.clone(),
                            index: offset + 1,
                            name,
                        });
                    }
                    match body {
                        Expr::Stmt(block) => {
                            self.statements_into(block.statements().to_vec(), value)?;
                        }
                        body => self.statement(Stmt::Expr(body.into()), value)?,
                    }
                }
            }
            self.finish(Term::Goto(exit));
        }
        self.at(after, depth);
        Ok(())
    }

    /// Lower an expression's awaits into blocks before it, and return what
    /// is left of it, which does not await.
    fn expr(&mut self, expr: Expr) -> Result<Expr, Unsupported> {
        if !expr_awaits(&expr) {
            return Ok(expr);
        }
        match expr {
            Expr::FnCall(mut call, position) if call.name == AWAIT && call.args.len() == 1 => {
                let future = self.expr(call.args.remove(0))?;
                let output = self.temporary();
                let next = self.block(false);
                self.finish(Term::Await(future, output.clone(), self.jump(next)));
                self.at(next, self.depth);
                Ok(variable_at(&output, position))
            }
            Expr::FnCall(mut call, position) => {
                self.sequence(call.args.iter_mut().collect())?;
                Ok(Expr::FnCall(call, position))
            }
            Expr::MethodCall(mut call, position) => {
                self.sequence(call.args.iter_mut().collect())?;
                Ok(Expr::MethodCall(call, position))
            }
            Expr::Dot(mut binary, flags, position) => {
                let BinaryExpr { lhs, rhs } = &mut *binary;
                let mut operands = vec![lhs];
                chain_operands(rhs, &mut operands)?;
                self.sequence(operands)?;
                Ok(Expr::Dot(binary, flags, position))
            }
            Expr::Index(mut binary, flags, position) => {
                let BinaryExpr { lhs, rhs } = &mut *binary;
                self.sequence(vec![lhs, rhs])?;
                Ok(Expr::Index(binary, flags, position))
            }
            Expr::Array(mut items, position) => {
                self.sequence(items.iter_mut().collect())?;
                Ok(Expr::Array(items, position))
            }
            Expr::InterpolatedString(mut items, position) => {
                self.sequence(items.iter_mut().collect())?;
                Ok(Expr::InterpolatedString(items, position))
            }
            Expr::Map(mut map, position) => {
                self.sequence(map.0.iter_mut().map(|(_, value)| value).collect())?;
                Ok(Expr::Map(map, position))
            }
            Expr::And(operands, position) => {
                self.logic(operands.into_iter().collect(), true, position)
            }
            Expr::Or(operands, position) => {
                self.logic(operands.into_iter().collect(), false, position)
            }
            Expr::Stmt(block) => {
                let output = self.temporary();
                self.push(Op::Let(output.clone(), unit_stmt()));
                let position = block.position();
                self.scoped(block.statements().to_vec(), Some(&output))?;
                Ok(variable_at(&output, position))
            }
            expr => Err(Unsupported("`.await` in this expression", expr.position())),
        }
    }

    /// Operands Rust evaluates left to right. Those before the last one
    /// that awaits are computed once into hidden locals, unless they are a
    /// constant or a local, which a call may change in place.
    fn sequence(&mut self, operands: Vec<&mut Expr>) -> Result<(), Unsupported> {
        let Some(last) = operands.iter().rposition(|operand| expr_awaits(operand)) else {
            return Ok(());
        };
        for (index, operand) in operands.into_iter().enumerate().take(last + 1) {
            let lowered = self.expr(std::mem::take(operand))?;
            *operand = if index < last && !settled(&lowered) {
                let name = self.temporary();
                let position = lowered.position();
                self.push(Op::Let(name.clone(), Stmt::Expr(lowered.into())));
                variable_at(&name, position)
            } else {
                lowered
            };
        }
        Ok(())
    }

    /// `a && b` and `a || b` whose later operands await: each runs only
    /// when the ones before it leave the answer open.
    fn logic(
        &mut self,
        operands: Vec<Expr>,
        and: bool,
        position: Position,
    ) -> Result<Expr, Unsupported> {
        let output = self.temporary();
        let mut operands = operands.into_iter();
        let first = operands.next().unwrap_or(Expr::BoolConstant(and, position));
        let first = self.expr(first)?;
        self.push(Op::Let(output.clone(), Stmt::Expr(first.into())));
        for operand in operands {
            let depth = self.depth;
            let open = self.block(true);
            let after = self.block(false);
            let (then, other) = if and {
                (self.jump(open), self.jump(after))
            } else {
                (self.jump(after), self.jump(open))
            };
            self.finish(Term::If(variable(&output), then, other));
            self.at(open, depth);
            let operand = self.expr(operand)?;
            self.push(Op::Set(output.clone(), Stmt::Expr(operand.into())));
            self.finish(Term::Goto(Jump {
                target: after,
                depth,
            }));
            self.at(after, depth);
        }
        Ok(variable_at(&output, position))
    }
}

/// The operands of a method chain after its receiver: each call's
/// arguments and each index, in the order the evaluator computes them.
fn chain_operands<'a>(
    expr: &'a mut Expr,
    operands: &mut Vec<&'a mut Expr>,
) -> Result<(), Unsupported> {
    match expr {
        Expr::MethodCall(call, _) => operands.extend(call.args.iter_mut()),
        Expr::Dot(binary, ..) => {
            let BinaryExpr { lhs, rhs } = &mut **binary;
            chain_operands(lhs, operands)?;
            chain_operands(rhs, operands)?;
        }
        Expr::Index(binary, ..) => {
            let BinaryExpr { lhs, rhs } = &mut **binary;
            chain_operands(lhs, operands)?;
            operands.push(rhs);
        }
        Expr::Property(..) | Expr::Variable(..) => {}
        expr if expr_awaits(expr) => {
            return Err(Unsupported(
                "`.await` in this method chain",
                expr.position(),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn store(value: Option<&ImmutableString>, statement: Stmt) -> Op {
    match value {
        Some(name) => Op::Set(name.clone(), statement),
        None => Op::Run(statement),
    }
}

/// A value that reads the same whenever it is evaluated, or a local that a
/// call may change in place and so must stay itself.
fn settled(expr: &Expr) -> bool {
    matches!(expr, Expr::Variable(..) | Expr::ThisPtr(..)) || expr.is_constant()
}

fn unit_stmt() -> Stmt {
    Stmt::Expr(Expr::Unit(Position::NONE).into())
}

fn variable(name: &ImmutableString) -> Expr {
    variable_at(name, Position::NONE)
}

fn variable_at(name: &ImmutableString, position: Position) -> Expr {
    #[cfg(not(feature = "no_module"))]
    return Expr::Variable(
        (None, name.clone(), crate::ast::Namespace::NONE, 0).into(),
        None,
        position,
    );
    #[cfg(feature = "no_module")]
    return Expr::Variable((None, name.clone()).into(), None, position);
}

fn stmt_awaits(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::If(flow, _) | Stmt::While(flow, _) | Stmt::Do(flow, ..) | Stmt::TryCatch(flow, _) => {
            flow_awaits(flow)
        }
        Stmt::For(for_loop, _) => flow_awaits(&for_loop.2),
        Stmt::Switch(switch, _) => {
            expr_awaits(&switch.0)
                || switch
                    .1
                    .expressions
                    .iter()
                    .any(|case| expr_awaits(&case.lhs) || expr_awaits(&case.rhs))
        }
        Stmt::Var(var, ..) => expr_awaits(&var.1),
        Stmt::Assignment(assignment) => {
            expr_awaits(&assignment.1.lhs) || expr_awaits(&assignment.1.rhs)
        }
        Stmt::FnCall(call, _) => call.name == AWAIT || call.args.iter().any(expr_awaits),
        Stmt::Block(block) => block.iter().any(stmt_awaits),
        Stmt::Expr(expr) => expr_awaits(expr),
        Stmt::BreakLoop(Some(expr), ..) | Stmt::Return(Some(expr), ..) => expr_awaits(expr),
        _ => false,
    }
}

fn flow_awaits(flow: &FlowControl) -> bool {
    expr_awaits(&flow.expr)
        || flow.body.iter().any(stmt_awaits)
        || flow.branch.iter().any(stmt_awaits)
}

fn expr_awaits(expr: &Expr) -> bool {
    match expr {
        Expr::FnCall(call, _) | Expr::MethodCall(call, _) => {
            call.name == AWAIT || call.args.iter().any(expr_awaits)
        }
        Expr::Stmt(block) => block.iter().any(stmt_awaits),
        Expr::InterpolatedString(items, _) | Expr::Array(items, _) => items.iter().any(expr_awaits),
        Expr::Map(map, _) => map.0.iter().any(|(_, value)| expr_awaits(value)),
        Expr::Dot(binary, ..) | Expr::Index(binary, ..) => {
            expr_awaits(&binary.lhs) || expr_awaits(&binary.rhs)
        }
        Expr::And(operands, _) | Expr::Or(operands, _) | Expr::Coalesce(operands, _) => {
            operands.iter().any(expr_awaits)
        }
        #[cfg(not(feature = "no_custom_syntax"))]
        Expr::Custom(custom, _) => custom.inputs.iter().any(expr_awaits),
        _ => false,
    }
}

/// Clear the scope offsets the parser cached: hidden locals move them. The
/// machine looks every local up by name.
fn forget_indices(stmt: &mut Stmt) {
    match stmt {
        Stmt::Var(var, ..) => {
            var.2 = None;
            forget_expr(&mut var.1);
        }
        #[cfg(not(feature = "no_closure"))]
        Stmt::Share(shared) => shared.iter_mut().for_each(|(_, index)| *index = None),
        Stmt::If(flow, _) | Stmt::While(flow, _) | Stmt::Do(flow, ..) | Stmt::TryCatch(flow, _) => {
            forget_flow(flow);
        }
        Stmt::For(for_loop, _) => forget_flow(&mut for_loop.2),
        Stmt::Switch(switch, _) => {
            forget_expr(&mut switch.0);
            for case in switch.1.expressions.iter_mut() {
                forget_expr(&mut case.lhs);
                forget_expr(&mut case.rhs);
            }
        }
        Stmt::Assignment(assignment) => {
            forget_expr(&mut assignment.1.lhs);
            forget_expr(&mut assignment.1.rhs);
        }
        Stmt::FnCall(call, _) => call.args.iter_mut().for_each(forget_expr),
        Stmt::Block(block) => block.statements_mut().iter_mut().for_each(forget_indices),
        Stmt::Expr(expr) => forget_expr(expr),
        Stmt::BreakLoop(Some(expr), ..) | Stmt::Return(Some(expr), ..) => forget_expr(expr),
        _ => {}
    }
}

fn forget_flow(flow: &mut FlowControl) {
    forget_expr(&mut flow.expr);
    flow.body
        .statements_mut()
        .iter_mut()
        .for_each(forget_indices);
    flow.branch
        .statements_mut()
        .iter_mut()
        .for_each(forget_indices);
}

fn forget_expr(expr: &mut Expr) {
    match expr {
        Expr::FnCall(call, _) | Expr::MethodCall(call, _) => {
            call.args.iter_mut().for_each(forget_expr)
        }
        Expr::Stmt(block) => block.statements_mut().iter_mut().for_each(forget_indices),
        Expr::InterpolatedString(items, _) | Expr::Array(items, _) => {
            items.iter_mut().for_each(forget_expr);
        }
        Expr::Map(map, _) => map.0.iter_mut().for_each(|(_, value)| forget_expr(value)),
        Expr::Dot(binary, ..) | Expr::Index(binary, ..) => {
            forget_expr(&mut binary.lhs);
            forget_expr(&mut binary.rhs);
        }
        Expr::And(operands, _) | Expr::Or(operands, _) | Expr::Coalesce(operands, _) => {
            operands.iter_mut().for_each(forget_expr);
        }
        _ => {}
    }
}

/// What an async body's function returns each time it runs.
#[derive(Clone)]
pub enum Poll {
    /// The body finished with this output.
    Ready(Dynamic),
    /// The body awaits `future`. Call the function again with `frame`,
    /// given the future's output with [`Frame::resume`].
    Pending {
        /// The awaited value.
        future: Dynamic,
        /// The suspended body.
        frame: Frame,
    },
}

impl fmt::Debug for Poll {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready(value) => f.debug_tuple("Ready").field(value).finish(),
            Self::Pending { future, .. } => {
                f.debug_struct("Pending").field("future", future).finish()
            }
        }
    }
}

/// A suspended async body: its locals, the scopes it has open, and where
/// it goes on. It resumes once. Copies share it.
#[derive(Clone)]
pub struct Frame(crate::Shared<crate::Locked<Option<Saved>>>);

pub(crate) struct Saved {
    machine: crate::Shared<Machine>,
    next: Jump,
    output: ImmutableString,
    input: Dynamic,
    locals: Vec<(ImmutableString, AccessMode, Dynamic)>,
    marks: Vec<usize>,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Frame")
    }
}

impl Frame {
    /// This frame with the awaited future's output, ready to pass back as
    /// the function's last argument.
    pub fn resume(self, output: Dynamic) -> Dynamic {
        if let Some(mut saved) = crate::func::locked_write(&self.0)
            && let Some(saved) = saved.as_mut()
        {
            saved.input = output;
        }
        Dynamic::from(self)
    }

    fn take(&self) -> Option<Saved> {
        crate::func::locked_write(&self.0)?.take()
    }
}

/// An iterator a suspended `for` loop is part way through. The evaluator's
/// iterators are not `Send`, so it is only advanced on the thread that made
/// it, and leaked if another thread drops it.
#[derive(Clone)]
struct Cursor(crate::Shared<crate::Locked<Iteration>>);

struct Iteration {
    #[cfg(not(feature = "no_std"))]
    thread: std::thread::ThreadId,
    iterator: std::mem::ManuallyDrop<Box<dyn Iterator<Item = crate::RawrResultOf<Dynamic>>>>,
}

// SAFETY: the iterator is only advanced and dropped on the thread that made
// it (`next`, `Drop`); on any other thread it is leaked.
#[cfg(feature = "sync")]
unsafe impl Send for Iteration {}
// SAFETY: as above: no shared access advances or drops the iterator on a
// thread other than the one that made it.
#[cfg(feature = "sync")]
unsafe impl Sync for Iteration {}

impl Iteration {
    fn owned(&self) -> bool {
        #[cfg(not(feature = "no_std"))]
        return self.thread == std::thread::current().id();
        #[cfg(feature = "no_std")]
        return true;
    }
}

impl Drop for Iteration {
    fn drop(&mut self) {
        if self.owned() {
            // SAFETY: dropped once, here.
            unsafe { std::mem::ManuallyDrop::drop(&mut self.iterator) };
        }
    }
}

/// How a block ends when it runs.
enum Step {
    Jump(Jump),
    /// Jump, then push a local in the scope entered: a loop's variable.
    Bind(Jump, ImmutableString, Dynamic),
    /// Suspend with the awaited value.
    Suspend(Dynamic),
}

impl Engine {
    /// Run a lowered async body from its start or from the frame in its
    /// last parameter. The parameters are the last locals in `scope`, from
    /// `base` on.
    pub(crate) fn run_machine(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        mut this_ptr: Option<&mut Dynamic>,
        machine: &crate::Shared<Machine>,
        base: usize,
    ) -> RawrResult {
        let searching = global.always_search_scope;
        global.always_search_scope = true;
        let result = (|| {
            let resume = scope.get_mut_by_index(scope.len() - 1).take().flatten();
            let mut marks = Vec::new();
            let start = if resume.is::<Frame>() {
                let saved = resume
                    .cast::<Frame>()
                    .take()
                    .ok_or_else(|| runtime("this future already resumed"))?;
                if !crate::Shared::ptr_eq(&saved.machine, machine) {
                    return Err(runtime("this future's code changed while it was suspended"));
                }
                scope.rewind(base);
                for (name, access, value) in saved.locals {
                    scope.push_entry(name, access, value);
                }
                marks.extend(saved.marks.into_iter().map(|mark| mark + base));
                scope.push_entry(saved.output, AccessMode::ReadWrite, saved.input);
                saved.next
            } else {
                Jump {
                    target: 0,
                    depth: 0,
                }
            };
            let mut current = enter(machine, scope, &mut marks, start);
            loop {
                let block = &machine.blocks[current];
                let step = self.run_block(global, caches, scope, this_ptr.as_deref_mut(), block);
                current = match step {
                    Ok(Step::Jump(jump)) => enter(machine, scope, &mut marks, jump),
                    Ok(Step::Bind(jump, name, value)) => {
                        let target = enter(machine, scope, &mut marks, jump);
                        scope.push_entry(name, AccessMode::ReadWrite, value);
                        target
                    }
                    Ok(Step::Suspend(future)) => {
                        let Term::Await(_, output, next) = &block.term else {
                            unreachable!("only an await suspends");
                        };
                        let mut locals = Vec::with_capacity(scope.len() - base);
                        while scope.len() > base {
                            let (name, value, _) =
                                scope.pop_entry().expect("the scope holds the locals");
                            locals.push((name, value.access_mode(), value));
                        }
                        locals.reverse();
                        let frame = Frame(crate::Shared::new(crate::Locked::new(Some(Saved {
                            machine: machine.clone(),
                            next: *next,
                            output: output.clone(),
                            input: Dynamic::UNIT,
                            locals,
                            marks: marks.iter().map(|mark| mark - base).collect(),
                        }))));
                        return Ok(Dynamic::from(Poll::Pending { future, frame }));
                    }
                    Err(error) => match *error {
                        EvalAltResult::Return(value, ..) => {
                            return Ok(Dynamic::from(Poll::Ready(value)));
                        }
                        EvalAltResult::LoopBreak(is_break, value, position) => {
                            let Some(within) = block.within else {
                                return Err(
                                    EvalAltResult::LoopBreak(is_break, value, position).into()
                                );
                            };
                            let lowered = &machine.loops[within];
                            let jump = if is_break {
                                if let Some(slot) =
                                    lowered.value.as_ref().and_then(|name| scope.get_mut(name))
                                {
                                    *slot = value;
                                }
                                lowered.exit
                            } else {
                                lowered.next
                            };
                            enter(machine, scope, &mut marks, jump)
                        }
                        _ => return Err(error),
                    },
                };
            }
        })();
        global.always_search_scope = searching;
        result
    }

    /// Run a block's statements and its terminator.
    fn run_block(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        mut this_ptr: Option<&mut Dynamic>,
        block: &Block,
    ) -> crate::RawrResultOf<Step> {
        for op in &block.ops {
            match op {
                Op::Run(statement) => {
                    let _ = self.eval_stmt(
                        global,
                        caches,
                        scope,
                        this_ptr.as_deref_mut(),
                        statement,
                        true,
                    )?;
                }
                Op::Let(name, statement) => {
                    let value = self
                        .eval_stmt(
                            global,
                            caches,
                            scope,
                            this_ptr.as_deref_mut(),
                            statement,
                            true,
                        )?
                        .flatten();
                    scope.push_entry(name.clone(), AccessMode::ReadWrite, value);
                }
                Op::Set(name, statement) => {
                    let value = self
                        .eval_stmt(
                            global,
                            caches,
                            scope,
                            this_ptr.as_deref_mut(),
                            statement,
                            true,
                        )?
                        .flatten();
                    *scope.get_mut(name).ok_or_else(|| missing(name))? = value;
                }
                Op::Cursor(name, expr) => {
                    let value = self
                        .eval_expr(global, caches, scope, this_ptr.as_deref_mut(), expr)?
                        .flatten();
                    let cursor = self.cursor(global, value, expr.start_position())?;
                    scope.push_entry(name.clone(), AccessMode::ReadWrite, Dynamic::from(cursor));
                }
                Op::Bind { from, index, name } => {
                    let value = element(scope, from, *index)?;
                    scope.push_entry(name.clone(), AccessMode::ReadWrite, value);
                }
                Op::Assign { from, index, name } => {
                    let value = element(scope, from, *index)?;
                    *scope.get_mut(name).ok_or_else(|| missing(name))? = value;
                }
            }
        }
        Ok(match &block.term {
            Term::Goto(jump) => Step::Jump(*jump),
            Term::If(condition, then, other) => {
                let value = self
                    .eval_expr(global, caches, scope, this_ptr, condition)?
                    .as_bool()
                    .map_err(|typ| {
                        self.make_type_mismatch_err::<bool>(typ, condition.position())
                    })?;
                Step::Jump(if value { *then } else { *other })
            }
            Term::Await(future, ..) => Step::Suspend(
                self.eval_expr(global, caches, scope, this_ptr, future)?
                    .flatten(),
            ),
            Term::Return(expr) => {
                let value = self.eval_expr(global, caches, scope, this_ptr, expr)?;
                return Err(EvalAltResult::Return(value, Position::NONE).into());
            }
            Term::Next {
                cursor,
                name,
                body,
                done,
            } => {
                let cursor = scope
                    .get(cursor)
                    .and_then(|value| value.read_lock::<Cursor>().map(|cursor| cursor.clone()))
                    .ok_or_else(|| missing(cursor))?;
                let next = {
                    let mut iteration = crate::func::locked_write(&cursor.0)
                        .ok_or_else(|| runtime("a loop's iterator is busy"))?;
                    if !iteration.owned() {
                        return Err(runtime("a loop's iterator moved to another thread"));
                    }
                    iteration.iterator.next()
                };
                match next {
                    Some(value) => Step::Bind(*body, name.clone(), value?.flatten()),
                    None => Step::Jump(*done),
                }
            }
            Term::Table(selected, targets) => {
                let index = element(scope, selected, 0)?
                    .as_int()
                    .map_err(|typ| self.make_type_mismatch_err::<INT>(typ, Position::NONE))?;
                Step::Jump(
                    *usize::try_from(index)
                        .ok()
                        .and_then(|index| targets.get(index))
                        .ok_or_else(|| runtime("no match arm was selected"))?,
                )
            }
        })
    }

    fn cursor(
        &self,
        global: &GlobalRuntimeState,
        value: Dynamic,
        position: Position,
    ) -> crate::RawrResultOf<Cursor> {
        let iter_type = value.type_id();
        let iter_func = self
            .global_modules
            .iter()
            .find_map(|m| m.get_iter(iter_type));
        #[cfg(not(feature = "no_module"))]
        let iter_func = iter_func
            .or_else(|| global.get_iter(iter_type))
            .or_else(|| {
                self.global_sub_modules
                    .values()
                    .find_map(|m| m.get_qualified_iter(iter_type))
            });
        #[cfg(feature = "no_module")]
        let _ = global;
        let iter_func = iter_func.ok_or_else(|| EvalAltResult::ErrorFor(position))?;
        Ok(Cursor(crate::Shared::new(crate::Locked::new(Iteration {
            #[cfg(not(feature = "no_std"))]
            thread: std::thread::current().id(),
            iterator: std::mem::ManuallyDrop::new(iter_func(value)),
        }))))
    }
}

/// Leave the scopes a jump leaves, and enter its target.
fn enter(machine: &Machine, scope: &mut Scope, marks: &mut Vec<usize>, jump: Jump) -> usize {
    if marks.len() > jump.depth {
        scope.rewind(marks[jump.depth]);
        marks.truncate(jump.depth);
    }
    if machine.blocks[jump.target].enter {
        marks.push(scope.len());
    }
    jump.target
}

fn element(scope: &Scope, from: &ImmutableString, index: usize) -> crate::RawrResultOf<Dynamic> {
    let array = scope.get(from).ok_or_else(|| missing(from))?;
    let array = array
        .read_lock::<crate::Array>()
        .ok_or_else(|| runtime("a lowered match arm lost its bindings"))?;
    array
        .get(index)
        .cloned()
        .ok_or_else(|| runtime("a lowered match arm lost its bindings"))
}

fn missing(name: &str) -> Box<EvalAltResult> {
    runtime(&format!("the async body lost its local `{name}`"))
}

fn runtime(message: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

#[cfg(test)]
mod tests {
    use super::{lower, Poll};
    use crate::{Dynamic, Engine, Scope, INT};

    /// Run `body(resume)` to its end. Each awaited integer `n` resolves to
    /// `n * 10`. Returns the output and the awaited values in order.
    fn drive(script: &str) -> (INT, Vec<INT>) {
        let engine = Engine::new();
        let mut ast = engine
            .compile_legacy_script_with_scope_raw(
                None,
                script,
                #[cfg(not(feature = "no_optimize"))]
                crate::OptimizationLevel::None,
            )
            .unwrap();
        ast.for_each_fn_def_mut(|function| lower(function).unwrap());
        let mut awaited = Vec::new();
        let mut resume = Dynamic::UNIT;
        loop {
            let poll: Poll = engine
                .call_fn(&mut Scope::new(), &ast, "body", (resume,))
                .unwrap_or_else(|error| panic!("{error}"));
            match poll {
                Poll::Ready(value) => return (value.as_int().unwrap(), awaited),
                Poll::Pending { future, frame } => {
                    let value = future.as_int().unwrap();
                    awaited.push(value);
                    resume = frame.resume(Dynamic::from(value * 10));
                }
            }
        }
    }

    #[test]
    fn awaits_in_order_and_resumes_with_each_output() {
        let (output, awaited) =
            drive("fn body(r) { let a = __rawr_await(1); let b = __rawr_await(a + 1); a + b }");
        assert_eq!((output, awaited), (120, vec![1, 11]));
    }

    #[test]
    fn a_for_loop_resumes_part_way_through_its_iterator() {
        let (output, awaited) = drive(
            "fn body(r) { let total = 0; for i in 0..4 { total += __rawr_await(i); } total }",
        );
        assert_eq!((output, awaited), (60, vec![0, 1, 2, 3]));
    }

    #[test]
    fn a_branch_scope_ends_and_its_shadow_goes_with_it() {
        let (output, awaited) = drive(
            "fn body(r) { let x = 1; let y = 0; if x == 1 { let x = __rawr_await(5); y = x; } x + y }",
        );
        assert_eq!((output, awaited), (51, vec![5]));
    }

    #[test]
    fn operands_before_an_await_are_computed_once() {
        let (output, awaited) = drive(
            "fn body(r) { let n = 2; let v = [n * 3, __rawr_await(n), n + __rawr_await(4)]; v[0] + v[1] + v[2] }",
        );
        assert_eq!((output, awaited), (6 + 20 + 42, vec![2, 4]));
    }

    #[test]
    fn return_and_break_leave_lowered_code() {
        let (output, awaited) = drive(
            "fn body(r) { let i = 0; let found = loop { i += 1; if __rawr_await(i) > 20 { break i; } }; if __rawr_await(found) > 5 { return found * 100; } 0 }",
        );
        assert_eq!((output, awaited), (300, vec![1, 2, 3, 3]));
    }

    #[test]
    fn a_block_value_and_an_early_return_inside_it() {
        let script = "fn body(r) { let x = { let t = __rawr_await(1); if t > 5 { t } else { return -1; } }; x + 1 }";
        assert_eq!(drive(script), (11, vec![1]));
        let script = "fn body(r) { let x = { let t = __rawr_await(0); if t > 5 { t } else { return -1; } }; x + 1 }";
        assert_eq!(drive(script), (-1, vec![0]));
    }

    #[test]
    fn a_switch_arm_awaits_and_others_give_their_value() {
        let script = "fn body(r) { let total = 0; for k in 0..3 { total += switch k { 1 => __rawr_await(7) + 1, _ => k } } total }";
        assert_eq!(drive(script), (71 + 2, vec![7]));
    }

    #[test]
    fn while_and_continue_run_each_iteration_in_a_fresh_scope() {
        let script = "fn body(r) { let i = 0; let sum = 0; while i < 5 { i += 1; if i == 2 { continue; } let v = __rawr_await(i); sum += v; } sum }";
        assert_eq!(drive(script), (130, vec![1, 3, 4, 5]));
    }

    #[test]
    fn a_frame_resumes_once() {
        let engine = Engine::new();
        let mut ast = engine
            .compile_legacy_script_with_scope_raw(
                None,
                "fn body(r) { __rawr_await(1) }",
                #[cfg(not(feature = "no_optimize"))]
                crate::OptimizationLevel::None,
            )
            .unwrap();
        ast.for_each_fn_def_mut(|function| lower(function).unwrap());
        let Poll::Pending { frame, .. } = engine
            .call_fn::<Poll>(&mut Scope::new(), &ast, "body", (Dynamic::UNIT,))
            .unwrap()
        else {
            panic!("the body awaits");
        };
        let again = frame.clone();
        let _: Poll = engine
            .call_fn(
                &mut Scope::new(),
                &ast,
                "body",
                (frame.resume(Dynamic::from(1_i64)),),
            )
            .unwrap();
        let error = engine
            .call_fn::<Poll>(
                &mut Scope::new(),
                &ast,
                "body",
                (again.resume(Dynamic::UNIT),),
            )
            .unwrap_err();
        assert!(error.to_string().contains("already resumed"), "{error}");
    }
}
