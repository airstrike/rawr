#!/usr/bin/env python3
"""D711 step 0: shadow-compare rawr's hand lists of std against the facts
the driver's `--std-catalog` walk extracted from the sysroot.

    python3 -I scripts/std-catalog-shadow.py CATALOG.json REPO

Reads the hand lists from the source, so it compares what the tree says
today. Prints each disagreement; labelling them is the report's job.
"""
import collections
import json
import re
import sys

catalog = json.load(open(sys.argv[1]))
repo = sys.argv[2]
resolve = open(f"{repo}/src/resolve.rs").read()
nominal = open(f"{repo}/src/resolve/lower_nominal.rs").read()

items = catalog["items"]
impls = catalog["implementations"]
by_hash = {i["id"]["def_path_hash"]: i for i in items}
paths = {}
for p in catalog["paths"]:
    paths.setdefault(p["path"], []).append(p)


def stable_path(path):
    return any(p["stable"] for p in paths.get(path, []))


def path_kind(path):
    return {p["kind"] for p in paths.get(path, []) if p["stable"]}


def section(title):
    print(f"\n## {title}")


# Stable public std traits, by name.
traits = collections.defaultdict(set)
for path, entries in paths.items():
    for entry in entries:
        if entry["kind"] == "trait" and entry["stable"] and path.startswith("std::"):
            traits[path.rsplit("::", 1)[1]].add(path)


def members(trait_hash):
    out = {}
    for item in items:
        if item.get("owner", {}) and item["owner"]["def_path_hash"] == trait_hash:
            out[item["path"][-1]] = item
    return out


def trait_item(name):
    for item in items:
        if item["kind"] == "trait" and item["path"][-1] == name and item["effective"] == "stable":
            yield item


# 1. BUILTIN_TRAITS
section("BUILTIN_TRAITS (src/resolve.rs)")
body = re.search(r"pub\(crate\) const BUILTIN_TRAITS: \[&str; \d+\] = \[(.*?)\];", resolve, re.S)
builtin = re.findall(r'"(\w+)"', body.group(1))
print(f"hand list: {len(builtin)} names")
for name in builtin:
    found = sorted(traits.get(name, []))
    if not found:
        print(f"  NOT A STABLE STD TRAIT: {name}")
implementable = []
for name, found in sorted(traits.items()):
    if name in builtin:
        continue
    for item in trait_item(name):
        shape = item.get("shape") or {}
        if shape.get("auto_trait") or shape.get("unsafe_trait"):
            continue
        required = [m for m in members(item["id"]["def_path_hash"]).values()
                    if m.get("provided") is False and m["effective"] == "stable"]
        implementable.append((name, len(required)))
        break
print(f"stable std traits a program may implement that the list lacks: {len(implementable)}")
print("  " + ", ".join(f"{n}({r})" for n, r in implementable))

# 2. STD_TRAIT_FUNCTIONS
section("STD_TRAIT_FUNCTIONS (src/resolve/lower_nominal.rs)")
body = re.search(r"const STD_TRAIT_FUNCTIONS: &\[\(&str, &\[&str\]\)\] = &\[(.*?)\n\];", nominal, re.S)
for name, functions in re.findall(r'\("(\w+)", &\[([^\]]*)\]\)', body.group(1)):
    listed = set(re.findall(r'"(\w+)"', functions))
    if name == "cmp":
        free = {p.rsplit("::", 1)[1] for p in paths if p.startswith("std::cmp::")
                and path_kind(p) == {"function"}}
        print(f"  cmp: listed {sorted(listed)}; std::cmp's stable functions {sorted(free)}")
        continue
    found = list(trait_item(name))
    if not found:
        print(f"  {name}: no stable std trait of that name")
        continue
    stable_members = {n for n, m in members(found[0]["id"]["def_path_hash"]).items()
                      if m["effective"] == "stable" and m["kind"] == "associated-function"}
    missing = sorted(stable_members - listed)
    extra = sorted(listed - stable_members)
    if missing or extra:
        print(f"  {name}: not listed {missing}; listed but not members {extra}")

# std_fmt_import and the type arity table were compared here in step 0;
# step 1 (#742) replaced them with src/std_table.

# 5. builtin_inherent_method
section("builtin_inherent_method (lower_nominal.rs) for Option, Result, Vec, String")
inherent = collections.defaultdict(dict)
for implementation in impls:
    if implementation["trait_path"] is None:
        # Every impl on the type, as `impl<T> Option<&T>` for `copied`.
        self_ty = implementation["self_ty"]
        base = self_ty if self_ty.startswith("[") else self_ty.split("<", 1)[0]
        for member in implementation["items"]:
            item = by_hash.get(member["def_path_hash"])
            if item and item["effective"] == "stable":
                inherent[base].setdefault(item["path"][-1], item)
body = re.search(r"fn builtin_inherent_method\(ty: &Ty, member: &str\) -> bool \{(.*?)\n\}", nominal, re.S).group(1)
rank_body = re.search(r"fn builtin_inherent_receiver_rank\(ty: &Ty, member: &str\) -> u8 \{(.*?)\n\}", nominal, re.S).group(1)


def arm(source, pattern):
    match = re.search(pattern + r"(.*?)(?:\n        Ty::|\n        _ =>)", source, re.S)
    return match.group(1)


def receiver_rank(item):
    receiver = item["shape"]["signature"]["receiver"]
    if receiver["kind"] == "value":
        return 0
    if receiver["kind"] == "reference":
        return 1 if receiver["mutability"] == "shared" else 2
    return None


def rawr_rank(kind, member):
    if kind == "Option":
        if member in ("is_some", "is_none", "as_ref", "as_deref", "clone"):
            return 1
        if member in ("as_mut", "take", "replace"):
            return 2
        return 0
    if kind == "Result":
        if member in ("is_ok", "is_err", "as_ref", "clone"):
            return 1
        return 2 if member == "as_mut" else 0
    mutating = ("push", "push_str", "pop", "clear", "insert", "insert_str", "remove",
                "truncate", "split_off", "retain")
    return 2 if member in mutating else 1


cases = [
    ("Option", r"Ty::Option\(_\) => matches!\(", ["std::option::Option"], []),
    ("Result", r"Ty::Result\(\.\.\) => matches!\(", ["std::result::Result"], []),
    ("Vec", r"Ty::Array \{ \.\. \} => matches!\(", ["std::vec::Vec"], ["[T]"]),
    ("String", r"Ty::String \| Ty::Str => matches!\(", ["std::string::String"], ["str"]),
]
for kind, pattern, own, deref in cases:
    listed = re.findall(r'"(\w+)"', arm(body, pattern))
    own_methods = {}
    for ty in own:
        own_methods.update(inherent.get(ty, {}))
    deref_methods = {}
    for ty in deref:
        deref_methods.update(inherent.get(ty, {}))
    print(f"  {kind}: {len(listed)} listed; std has {len(own_methods)} stable inherent "
          f"(+{len(deref_methods)} through Deref to {deref})")
    for member in listed:
        if member in own_methods:
            item = own_methods[member]
        elif member in deref_methods:
            item = deref_methods[member]
            print(f"    {member}: on {deref[0]} through Deref, not {own[0]}'s own")
        else:
            print(f"    {member}: NOT an inherent method of {own + deref} (a trait's)")
            continue
        rank = receiver_rank(item)
        if rank is not None and rank != rawr_rank(kind, member):
            print(f"    {member}: receiver rank rawr {rawr_rank(kind, member)}, std {rank}")
    unlisted = sorted(set(own_methods) - set(listed))
    print(f"    std's own stable methods the list lacks: {len(unlisted)}: {', '.join(unlisted)}")
