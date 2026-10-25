//! Module defining the AST (abstract syntax tree).

#[allow(clippy::module_inception)]
pub mod ast;
pub mod expr;
pub mod flags;
pub mod ident;
pub mod namespace;
pub mod script_fn;
pub mod stmt;

pub use ast::{ASTNode, EncapsulatedEnviron, AST};
pub use expr::CustomExpr;
pub use expr::{BinaryExpr, Expr, FnCallExpr, FnCallHashes};
pub use flags::{ASTFlags, FnAccess};
pub use ident::Ident;
pub use namespace::Namespace;
pub use script_fn::{ScriptFnMetadata, ScriptFuncDef};
pub use stmt::{
    CaseBlocksList, EnumCase, FlowControl, OpAssignment, RangeCase, Stmt, StmtBlock,
    StmtBlockContainer, SwitchCasesCollection,
};
pub(crate) use stmt::{
    NominalEnumPattern, NominalLiteral, NominalPattern, NominalPayloadType, NOMINAL_MATCH_SENTINEL,
};
