#!/usr/bin/env python3
"""zstd ratio when a sample is stored as independent ~80 KiB parts (as a parts
store would keep them), against the ratio of the whole stream. Ratios only."""
import os
import subprocess
import sys

samples = sys.argv[1]
names = sys.argv[2:]
PART = 80 * 1024
LIMIT = 16 << 20


def z(data, level):
    return len(subprocess.run(['zstd', '-q', '-c', f'-{level}'], input=data, stdout=subprocess.PIPE, check=True).stdout)


print(f"{'sample':20} {'whole-z3':>8} {'parts-z1':>8} {'parts-z3':>8}")
for name in names:
    data = open(os.path.join(samples, name), 'rb').read()[:LIMIT]
    whole = len(data) / z(data, 3)
    parts = [data[i:i + PART] for i in range(0, len(data), PART)]
    p1 = len(data) / sum(z(p, 1) for p in parts)
    p3 = len(data) / sum(z(p, 3) for p in parts)
    print(f"{name:20} {whole:8.1f} {p1:8.1f} {p3:8.1f}")
