#!/usr/bin/env python3
"""Physical (deduplicated) disk usage of directory trees on APFS.

`du` counts an APFS clone at its full size. This maps every regular file's
extents with fcntl(F_LOG2PHYS_EXT), a read-only query, then sweeps the
physical ranges: each byte counts once. Per group (a path prefix of the
given depth under each root) it prints logical bytes, the physical bytes of
its own union, the bytes only it holds, and which groups share the rest.

Usage: physical.py [--depth N] ROOT [ROOT ...]
"""
import fcntl
import os
import struct
import sys
from collections import defaultdict

F_LOG2PHYS_EXT = 65
FMT = '=Iqq'  # packed(4): flags, contigbytes, devoffset

args = sys.argv[1:]
depth = 1
if args and args[0] == '--depth':
    depth = int(args[1])
    args = args[2:]


def extents(path, size):
    out = []
    try:
        fd = os.open(path, os.O_RDONLY)
    except OSError:
        return None
    try:
        offset = 0
        while offset < size:
            want = size - offset
            try:
                res = fcntl.fcntl(fd, F_LOG2PHYS_EXT, struct.pack(FMT, 0, want, offset))
            except OSError:
                return None
            _, contig, dev = struct.unpack(FMT, res)
            if contig <= 0:
                return None
            take = min(contig, want)
            out.append((dev, dev + take))
            offset += take
    finally:
        os.close(fd)
    return out


groups = []
group_index = {}
logical = defaultdict(int)
files = defaultdict(int)
unmapped = defaultdict(int)
events = []  # (position, +1/-1, group)


def group_of(root, path):
    rel = os.path.relpath(path, root)
    parts = rel.split(os.sep)
    if depth == 0:
        name = root.rstrip('/').replace('/Users/andy/projects/inboard/target/', 'T/').replace('/Users/andy/.cargo/rawr/', 'H/')
    else:
        name = root.rstrip('/').split('/')[-1] + '/' + '/'.join(parts[:depth])
    if name not in group_index:
        group_index[name] = len(groups)
        groups.append(name)
    return group_index[name]


for root in args:
    for dirpath, dirnames, filenames in os.walk(root):
        for f in filenames:
            p = os.path.join(dirpath, f)
            try:
                st = os.lstat(p)
            except OSError:
                continue
            if (st.st_mode & 0o170000) != 0o100000 or st.st_size == 0:
                continue
            g = group_of(root, p)
            logical[g] += st.st_size
            files[g] += 1
            ex = extents(p, st.st_size)
            if ex is None:
                unmapped[g] += st.st_size
                continue
            for a, b in ex:
                events.append((a, 1, g))
                events.append((b, -1, g))

events.sort(key=lambda e: (e[0], e[1]))
active = defaultdict(int)
own_union = defaultdict(int)
exclusive = defaultdict(int)
shared_sets = defaultdict(int)
total_union = 0
last = None
for pos, delta, g in events:
    if last is not None and pos > last and active:
        length = pos - last
        total_union += length
        keys = tuple(sorted(active))
        for k in keys:
            own_union[k] += length
        if len(keys) == 1:
            exclusive[keys[0]] += length
        else:
            shared_sets[keys] += length
    active[g] += delta
    if active[g] == 0:
        del active[g]
    last = pos


def h(n):
    for u in ['B', 'K', 'M', 'G', 'T']:
        if abs(n) < 1024:
            return f"{n:.1f}{u}"
        n /= 1024
    return f"{n:.1f}P"


print(f"{'logical':>9} {'physical':>9} {'only-here':>9} {'files':>7}  group")
for g in sorted(range(len(groups)), key=lambda g: -logical[g]):
    extra = f"  (unmapped {h(unmapped[g])})" if unmapped[g] else ''
    print(f"{h(logical[g]):>9} {h(own_union[g] + unmapped[g]):>9} {h(exclusive[g] + unmapped[g]):>9} {files[g]:>7}  {groups[g]}{extra}")
print(f"{h(sum(logical.values())):>9} {h(total_union + sum(unmapped.values())):>9} {'':>9} {sum(files.values()):>7}  TOTAL (physical is the union)")
print("\nlargest shared sets:")
for keys, length in sorted(shared_sets.items(), key=lambda kv: -kv[1])[:25]:
    print(f"  {h(length):>9}  {' & '.join(groups[k] for k in keys)}")
