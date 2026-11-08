#!/usr/bin/env python3
"""Decode a typed store's installation ledger (store/ledgers/installation),
read-only: the store's own accounting of each registered allocation."""
import struct
import sys

KINDS = {1: 'toolchain', 2: 'resolution', 3: 'api', 4: 'generic', 5: 'profile', 6: 'facade', 7: 'checked',
         8: 'profile-proof', 9: 'native-product', 10: 'overlay', 11: 'runner'}
CLASSES = {1: 'action', 2: 'content', 3: 'bundle'}
data = open(sys.argv[1], 'rb').read()
assert data[:8] == b'RAWRLEDR', data[:8]
used_bytes, used_allocs, used_members = struct.unpack('<QQQ', data[16:40])
count, alloc_count, next_seq, bundle_count, eviction_count, publisher_count = struct.unpack('<IIIIII', data[40:64])
print(f"used {used_bytes/2**30:.2f} GiB, {used_allocs} allocations, {used_members} members; reservations {count}, "
      f"registered {alloc_count}, bundles {bundle_count}, evictions pending {eviction_count}, publishers {publisher_count}, next seq {next_seq}")
off = 64 + count * 136
publishers = [data[off + 32 * i: off + 32 * (i + 1)].hex()[:12] for i in range(publisher_count)]
off += 32 * publisher_count
off += 4 * bundle_count
rows = []
for i in range(alloc_count):
    tag = data[off]
    cls = CLASSES.get(tag >> 4, '?')
    kind = KINDS.get(tag & 15, '?')
    schema = struct.unpack('<I', data[off + 1:off + 5])[0]
    key = data[off + 5:off + 37].hex()
    charge = struct.unpack('<Q', data[off + 37:off + 45])[0]
    seq = struct.unpack('<I', data[off + 77:off + 81])[0]
    pub = struct.unpack('<H', data[off + 81:off + 83])[0]
    rows.append((seq, kind, cls, charge, key[:12], schema, publishers[pub] if pub < len(publishers) else '?'))
    off += 115
by = {}
for seq, kind, cls, charge, key, schema, pub in sorted(rows):
    print(f"seq {seq:5} {kind:14} {cls:8} {charge/2**20:10.1f} MiB key {key} schema {schema:08x} publisher {pub}")
    by.setdefault((kind, cls), [0, 0])
    by[(kind, cls)][0] += 1
    by[(kind, cls)][1] += charge
print()
for (kind, cls), (n, b) in sorted(by.items(), key=lambda kv: -kv[1][1]):
    print(f"{kind:14} {cls:8} n={n:3} {b/2**30:7.2f} GiB")
