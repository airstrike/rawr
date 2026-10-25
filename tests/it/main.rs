//! Rawr's integration tests, as one binary rather than one per file, each
//! linking its own copy of Rawr.
//!
//! Files gate themselves with an inner `#![cfg]` where they need a feature.
//! Tests that reach the importer's native Cargo lane hold
//! `rawr_import::store::test_turn()`: the lane admits one owner per process.

mod arrays;
mod assignments;
mod binary_ops;
mod bit_fields;
mod blobs;
mod bool_op;
mod build_type;
mod call_fn;
mod chars;
mod closures;
mod comments;
mod compile_boundary;
mod compile_fixtures;
mod compound_equality;
mod constants;
mod data_size;
mod debugging;
mod decrement;
mod diagnostic;
mod enum_switch;
mod export;
mod expressions;
mod float;
mod fn_ptr;
mod r#for;
mod functions;
mod header_tuples;
mod host_facade;
mod host_overloads;
mod if_block;
mod increment;
mod looping;
mod maps;
mod math;
mod method_call;
mod mismatched_op;
mod modules;
mod native;
mod nominal_data;
mod nominal_match;
mod nominal_methods;
mod not;
mod number_literals;
mod operations;
mod ops;
mod optimizer;
mod options;
mod package_graph;
mod packages;
mod parse_json;
mod plugins;
mod polyglot;
mod power_of;
mod print;
mod program_authority;
mod project_discovery;
mod serde;
mod side_effects;
mod stack;
mod static_host_imports;
mod static_stream_operations;
mod string;
mod switch;
mod symbolic_contract;
mod throw;
mod time;
mod tokens;
mod unary_after_binary;
mod unary_minus;
mod unit;
mod var_scope;
mod while_loop;
mod widget_protocol;

#[cfg(feature = "rust-import-driver")]
mod rust_import;

// The grain harnesses keep their `grain::*` module paths and stay in
// `tests/grain/`. `allocation` is deliberately absent: it owns a counting
// global allocator and is its own binary, declared in Cargo.toml.
#[cfg(feature = "grain")]
#[path = "../grain"]
mod grain {
    // Declared once here rather than in each harness, which are modules of
    // this binary rather than crate roots.
    mod corpus;

    mod call_fn;
    mod callback;
    mod differential;
    mod format;
    // Both are about execution staying inside a bound, which `unchecked`
    // removes outright — and the artifact fuzzer needs `max_operations` to stop
    // a corrupted chunk looping forever rather than failing.
    #[cfg(not(feature = "unchecked"))]
    mod fuzz;
    #[cfg(not(feature = "unchecked"))]
    mod limits;
    mod scope;
}
