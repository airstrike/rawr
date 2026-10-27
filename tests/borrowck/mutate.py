#!/usr/bin/env python3
"""Write mutations of rawr's own programs into tests/borrowck/mutations.

    python3 -I tests/borrowck/mutate.py [kept | closures | moves | temporaries | regions | drops | moved | copied | out sources...]

With an output folder and sources, it mutates those programs into that
folder instead (`tests/borrowck/patterns/mutations` from the pattern
cases). For each `let mut x` in a program, and each later statement in its block
that names `x`, it writes variants that move a borrow's use past that
statement, into a branch, and across a loop's back edge, and variants
where the borrow ends first. With `kept`, the borrow is one a call's value
keeps or a struct, an `Option` or a `Vec` holds, and the variants go to
`kept-mutations`. With `closures`, the borrow is a closure's capture, or one
a closure's call returns, and the variants go to `closures/mutations`.
With `moves`, a closure moves `x` in, or is moved or copied while it
captures `x`, and the variants go to `closure-moves/mutations`. With
`temporaries`, the borrow is held through a temporary: one a `let` extends
(`&__id(&x)`), a method's receiver whose value keeps it
(`vec![&x].first()`), or one whose value keeps only what it holds
(`vec![&x].to_vec()`), and the variants go to `signatures/mutations`. With
`regions`, the borrow is held at one region position of a value: an item a
container gives, one lifetime of a type with two, or what an `Option`'s
`map` returns, and the variants go to `regions/mutations`. With `drops`,
the borrow is held by a value whose type has a `Drop` impl and is used
only where that value drops (at its scope's end, or not at all once it is
moved out), or by a `Vec`, whose drop does not use it, and the variants go
to `drops/mutations`. With `moved`, `x` is moved before the statement, on
a branch, in a loop, by a block's tail or a `match`, moved and given its
value back, or bound by `ref` only, and the variants go to
`moves/mutations`. With `copied`, the same moves are made of each local
(`let` or `let mut`) of the cases in `copy/`, whose types are `Copy` or
not as rustc has it, and the variants go to `copy/mutations`. rustc
decides which are errors: run `record.py` on the folder afterwards.
"""

import glob
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
KEPT = sys.argv[1:] == ["kept"]
CLOSURES = sys.argv[1:] == ["closures"]
MOVES = sys.argv[1:] == ["moves"]
TEMPORARIES = sys.argv[1:] == ["temporaries"]
REGIONS = sys.argv[1:] == ["regions"]
DROPS = sys.argv[1:] == ["drops"]
MOVED = sys.argv[1:] == ["moved"]
COPIED = sys.argv[1:] == ["copied"]
OUT = os.path.join(
    HERE,
    "kept-mutations"
    if KEPT
    else "closures/mutations"
    if CLOSURES
    else "closure-moves/mutations"
    if MOVES
    else "signatures/mutations"
    if TEMPORARIES
    else "regions/mutations"
    if REGIONS
    else "drops/mutations"
    if DROPS
    else "moves/mutations"
    if MOVED
    else "copy/mutations"
    if COPIED
    else "mutations",
)
SOURCES = sorted(glob.glob(os.path.join(HERE, "copy/*.rs"))) if COPIED else sorted(
    glob.glob(os.path.join(ROOT, "play/tests/fixtures/rust/*.rs"))
    + [os.path.join(ROOT, "examples/cart.rs"), os.path.join(ROOT, "examples/nominal_counter.rs")]
    + glob.glob(os.path.join(HERE, "slice1/*.rs"))
    + (glob.glob(os.path.join(HERE, "slice2/*.rs")) if KEPT or TEMPORARIES else [])
    + (glob.glob(os.path.join(HERE, "closures/*.rs")) if CLOSURES or MOVES else [])
    + (glob.glob(os.path.join(HERE, "closure-moves/*.rs")) if MOVES else [])
    + (glob.glob(os.path.join(HERE, "regions/*.rs")) if REGIONS else [])
    + (glob.glob(os.path.join(HERE, "drops/*.rs")) if DROPS else [])
    + (glob.glob(os.path.join(HERE, "moves/*.rs")) if MOVED else [])
)
HELPERS = """
fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool { false }
"""
KEPT_HELPERS = """
fn __keep<T>(x: &T) -> &T { x }
fn __see<T>(_: &T) {}
fn __c() -> bool { false }
struct __Hold<'a, T>(&'a T);
"""
TEMPORARY_HELPERS = """
fn __id<T>(x: &T) -> &T { x }
fn __see<T>(_: &T) {}
fn __c() -> bool { false }
"""
MOVED_HELPERS = """
fn __take<T>(_: T) {}
fn __c() -> bool { false }
"""
REGION_HELPERS = """
fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
"""
DROP_HELPERS = """
fn __c() -> bool { false }
fn __eat<T>(_: T) {}
struct __See<'a, T>(&'a T);
impl<T> Drop for __See<'_, T> {
    fn drop(&mut self) {}
}
struct __Hold<'a, T> {
    s: __See<'a, T>,
}
"""
DECLARATION = re.compile(r"^(\s*)let mut (\w+)\b[^=]*=.*;\s*$")
# `copied` moves locals declared without `mut` too.
ANY_DECLARATION = re.compile(r"^(\s*)let (?:mut )?(\w+)\b[^=]*=.*;\s*$")
PER_LOCAL = 2


def statements(lines, start, indent, name):
    """The single-line statements after `start` in its block that name `name`."""
    word = re.compile(rf"\b{re.escape(name)}\b")
    for index in range(start + 1, len(lines)):
        line = lines[index]
        if line.strip() and len(line) - len(line.lstrip()) < len(indent):
            return
        stripped = line.strip()
        if (
            line.startswith(indent)
            and not line[len(indent)].isspace()
            and stripped.endswith(";")
            and not stripped.startswith(("let ", "return", "break", "continue"))
            and stripped.count("{") == stripped.count("}")
            and word.search(stripped)
        ):
            yield index


def variants(indent, name, statement):
    i = indent
    return {
        "mut_across": [f"{i}let __b = &mut {name};", statement, f"{i}__touch(__b);"],
        "mut_ended": [f"{i}let __b = &mut {name};", f"{i}__touch(__b);", statement],
        "shared_across": [f"{i}let __s = &{name};", statement, f"{i}__see(__s);"],
        "into_branch": [f"{i}let __b = &mut {name};", statement, f"{i}if __c() {{ __touch(__b); }}"],
        "other_arm": [
            f"{i}let __b = &mut {name};",
            f"{i}if __c() {{ __touch(__b); }} else {{",
            statement,
            f"{i}}}",
        ],
        "back_edge": [
            f"{i}let __b = &mut {name};",
            f"{i}for __i in 0..2 {{",
            f"{i}    __touch(__b);",
            statement,
            f"{i}}}",
        ],
        "fresh_each_iteration": [
            f"{i}for __i in 0..2 {{",
            f"{i}    let __b = &mut {name};",
            f"{i}    __touch(__b);",
            statement,
            f"{i}}}",
        ],
    }


def kept_variants(indent, name, statement):
    """A borrow a call's value keeps, or a value holds, used past `statement`."""
    i = indent
    return {
        "kept_across": [f"{i}let __k = __keep(&{name});", statement, f"{i}__see(__k);"],
        "kept_ended": [f"{i}let __k = __keep(&{name});", f"{i}__see(__k);", statement],
        "kept_into_branch": [
            f"{i}let __k = __keep(&{name});",
            statement,
            f"{i}if __c() {{ __see(__k); }}",
        ],
        "kept_back_edge": [
            f"{i}let __k = __keep(&{name});",
            f"{i}for __i in 0..2 {{",
            f"{i}    __see(__k);",
            statement,
            f"{i}}}",
        ],
        "held_across": [f"{i}let __h = __Hold(&{name});", statement, f"{i}__see(&__h);"],
        "option_across": [f"{i}let __o = Some(&{name});", statement, f"{i}__see(&__o);"],
        "vec_across": [f"{i}let __v = vec![&{name}];", statement, f"{i}__see(&__v);"],
        "option_replaced": [
            f"{i}let mut __o = Some(&{name});",
            f"{i}__see(&__o);",
            f"{i}__o = None;",
            statement,
            f"{i}__see(&__o);",
        ],
    }


def temporary_variants(indent, name, statement):
    """A borrow held through a temporary, used past `statement`."""
    i = indent
    return {
        "extended_across": [f"{i}let __t = &__id(&{name});", statement, f"{i}__see(__t);"],
        "extended_ended": [f"{i}let __t = &__id(&{name});", f"{i}__see(__t);", statement],
        "extended_branch": [
            f"{i}let __t = if __c() {{ &__id(&{name}) }} else {{ &__id(&{name}) }};",
            statement,
            f"{i}__see(__t);",
        ],
        "receiver_kept_across": [
            f"{i}let __t = vec![&{name}].first();",
            statement,
            f"{i}__see(&__t);",
        ],
        "receiver_in_statement": [f"{i}__see(&vec![&{name}].first());", statement],
        "receiver_inner_across": [
            f"{i}let __t = vec![&{name}].to_vec();",
            statement,
            f"{i}__see(&__t);",
        ],
        "receiver_inner_ended": [
            f"{i}let __t = vec![&{name}].to_vec();",
            f"{i}__see(&__t);",
            statement,
        ],
    }


def closure_variants(indent, name, statement):
    """A closure that captures `name`, or a borrow its call returns, used past `statement`."""
    i = indent
    return {
        "closure_mut_across": [f"{i}let mut __f = || __touch(&mut {name});", statement, f"{i}__f();"],
        "closure_mut_ended": [f"{i}let mut __f = || __touch(&mut {name});", f"{i}__f();", statement],
        "closure_shared_across": [f"{i}let __g = || __see(&{name});", statement, f"{i}__g();"],
        "closure_into_branch": [
            f"{i}let mut __f = || __touch(&mut {name});",
            statement,
            f"{i}if __c() {{ __f(); }}",
        ],
        "closure_back_edge": [
            f"{i}let mut __f = || __touch(&mut {name});",
            f"{i}for __i in 0..2 {{",
            f"{i}    __f();",
            statement,
            f"{i}}}",
        ],
        "closure_returned_across": [
            f"{i}let __h = || &{name};",
            f"{i}let __k = __h();",
            statement,
            f"{i}__see(__k);",
        ],
    }


def move_variants(indent, name, statement):
    """A closure that moves `name` in, or one moved or copied while it captures `name`."""
    i = indent
    return {
        "move_closure_across": [f"{i}let __m = move || __see(&{name});", statement, f"{i}__m();"],
        "move_closure_after": [statement, f"{i}let __m = move || __see(&{name});", f"{i}__m();"],
        "move_closure_into_branch": [
            f"{i}if __c() {{",
            f"{i}    let __m = move || __see(&{name});",
            f"{i}    __m();",
            f"{i}}}",
            statement,
        ],
        "mut_closure_moved_across": [
            f"{i}let mut __f = || __touch(&mut {name});",
            f"{i}let mut __g = __f;",
            statement,
            f"{i}__g();",
        ],
        "mut_closure_moved_then_called": [
            f"{i}let mut __f = || __touch(&mut {name});",
            f"{i}let mut __g = __f;",
            f"{i}__g();",
            f"{i}__f();",
            statement,
        ],
        "shared_closure_copied": [
            f"{i}let __f = || __see(&{name});",
            f"{i}let __g = __f;",
            f"{i}__f();",
            statement,
            f"{i}__g();",
        ],
    }


def region_variants(indent, name, statement):
    """A borrow held at one region position of a value, used past `statement`."""
    i = indent
    item = [
        f"{i}let __v = vec![&{name}];",
        f"{i}let __k = __v.iter().copied().next().unwrap();",
    ]
    return {
        "item_across": item + [statement, f"{i}__see(&__k);"],
        "item_ended": item + [f"{i}__see(&__k);", statement],
        "lifetime_kept": [
            f"{i}let __u = 0u8;",
            f"{i}let __p = __Two(&{name}, &__u);",
            f"{i}let __k = __p.0;",
            statement,
            f"{i}__see(&__k);",
        ],
        "lifetime_apart": [
            f"{i}let __u = 0u8;",
            f"{i}let __p = __Two(&__u, &{name});",
            f"{i}let __k = __p.0;",
            statement,
            f"{i}__see(&__k);",
        ],
        "option_map_across": [
            f"{i}let __k = Some(0).map(|_| &{name});",
            statement,
            f"{i}__see(&__k);",
        ],
    }


def drop_variants(indent, name, statement):
    """A borrow held by a value with a `Drop` impl, used only where it drops."""
    i = indent
    see = f"{i}let __d = __See(&{name});"
    return {
        "drop_across": [see, statement],
        "drop_ended": [see, f"{i}drop(__d);", statement],
        "drop_moved_out": [see, f"{i}__eat(__d);", statement],
        "drop_maybe_moved": [see, f"{i}if __c() {{ drop(__d); }}", statement],
        "drop_in_block": [f"{i}{{", f"    {see}", f"{i}}}", statement],
        "drop_in_option": [f"{i}let __o = Some(__See(&{name}));", statement],
        "drop_in_holder": [f"{i}let __h = __Hold {{ s: __See(&{name}) }};", statement],
        "vec_no_drop_use": [f"{i}let __v = vec![&{name}];", statement],
    }


def moved_variants(indent, name, statement):
    """`name` moved, on some path or all, before `statement`, or moved and given back."""
    i = indent
    return {
        "moved_before": [f"{i}__take({name});", statement],
        "moved_after": [statement, f"{i}__take({name});"],
        "moved_on_branch": [f"{i}if __c() {{ __take({name}); }}", statement],
        "moved_on_diverging_branch": [
            f"{i}if __c() {{",
            f"{i}    __take({name});",
            f"{i}    panic!();",
            f"{i}}}",
            statement,
        ],
        "moved_in_loop": [f"{i}for __i in 0..2 {{ if __c() {{ __take({name}); }} }}", statement],
        "moved_and_given_back": [f"{i}let __m = {name};", f"{i}{name} = __m;", statement],
        "moved_and_given_back_in_loop": [
            f"{i}for __i in 0..2 {{",
            f"{i}    let __m = {name};",
            f"{i}    {name} = __m;",
            f"{i}}}",
            statement,
        ],
        "moved_by_block_tail": [f"{i}let __m = {{ {name} }};", statement],
        "moved_by_match": [f"{i}match {name} {{ __v => __take(__v) }}", statement],
        "bound_by_ref": [f"{i}let ref __v = {name};", statement],
    }


def main():
    out, sources = OUT, SOURCES
    if len(sys.argv) > 2:
        out, sources = sys.argv[1], sorted(sys.argv[2:])
    os.makedirs(out, exist_ok=True)
    for old in glob.glob(os.path.join(out, "*.rs")):
        os.remove(old)
    count = 0
    for source in sources:
        stem = os.path.splitext(os.path.basename(source))[0]
        lines = open(source, encoding="utf-8").read().splitlines()
        for start, line in enumerate(lines):
            declared = (ANY_DECLARATION if COPIED else DECLARATION).match(line)
            if not declared:
                continue
            indent, name = declared.group(1), declared.group(2)
            for target in list(statements(lines, start, indent, name))[:PER_LOCAL]:
                mutate = (
                    kept_variants
                    if KEPT
                    else closure_variants
                    if CLOSURES
                    else move_variants
                    if MOVES
                    else temporary_variants
                    if TEMPORARIES
                    else region_variants
                    if REGIONS
                    else drop_variants
                    if DROPS
                    else moved_variants
                    if MOVED or COPIED
                    else variants
                )
                written = mutate(indent, name, lines[target])
                # Giving a value back needs `let mut`.
                if COPIED and " mut " not in line:
                    written = {
                        kind: replacement
                        for kind, replacement in written.items()
                        if "given_back" not in kind
                    }
                for kind, replacement in written.items():
                    mutated = lines[:target] + replacement + lines[target + 1 :]
                    path = os.path.join(out, f"{stem}_{name}_{target + 1}_{kind}.rs")
                    with open(path, "w", encoding="utf-8") as file:
                        helpers = (
                            KEPT_HELPERS
                            if KEPT
                            else TEMPORARY_HELPERS
                            if TEMPORARIES
                            else REGION_HELPERS
                            if REGIONS
                            else DROP_HELPERS
                            if DROPS
                            else MOVED_HELPERS
                            if MOVED or COPIED
                            else HELPERS
                        )
                        file.write("\n".join(mutated) + "\n" + helpers)
                    count += 1
    # As the repository's other Rust files are.
    written = sorted(glob.glob(os.path.join(out, "*.rs")))
    subprocess.run(["rustfmt", "+stable", "--edition", "2024", *written], check=True)
    print(count)


if __name__ == "__main__":
    main()
