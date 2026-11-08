#!/usr/bin/env python3
"""zstd compression ratios of each sample (no timing: ratios only).
Also, for JSON samples, the share of bytes spent on 32-byte digests written
as arrays of decimal numbers, and what they would cost as hex strings."""
import os
import re
import subprocess
import sys

samples = sys.argv[1]
levels = [1, 3, 9]
DIGEST = re.compile(rb'\[(?:\d{1,3},){31}\d{1,3}\]')


def zsize(path, *flags):
    with open(path, 'rb') as f:
        out = subprocess.run(['zstd', '-q', '-c', '-T1', *flags], stdin=f, stdout=subprocess.PIPE, check=True).stdout
    return len(out)


print(f"{'sample':22} {'MiB':>7} " + ' '.join(f"{'z' + str(l):>6}" for l in levels) + f" {'z19':>6} {'z3long':>6}  digest-arrays")
for name in sorted(os.listdir(samples)):
    path = os.path.join(samples, name)
    size = os.path.getsize(path)
    ratios = [size / zsize(path, f'-{l}') for l in levels]
    z19 = f"{size / zsize(path, '-19'):6.1f}" if size <= 20 << 20 else '     -'
    zlong = f"{size / zsize(path, '-3', '--long=27'):6.1f}" if size >= 32 << 20 else '     -'
    data = open(path, 'rb').read()
    info = ''
    if data[:1] in (b'{', b'[', b',') or b'":' in data[:4096]:
        matches = DIGEST.findall(data)
        spent = sum(len(m) for m in matches)
        hexcost = len(matches) * 66
        info = f"{len(matches)} digests, {100 * spent / size:.0f}% of bytes, as hex {100 * hexcost / size:.0f}%, as raw {100 * 32 * len(matches) / size:.0f}%"
    print(f"{name:22} {size / 2**20:7.1f} " + ' '.join(f"{r:6.1f}" for r in ratios) + f" {z19} {zlong}  {info}")
