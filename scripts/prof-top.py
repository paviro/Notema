#!/usr/bin/env python3
"""Summarize a samply/Firefox-Profiler trace without the GUI.

samply defers symbolication to its viewer, so a saved `.json.gz` carries raw
addresses. On macOS this script resolves them against the recorded binary's
DWARF with `atos` (needs the .dSYM next to the binary, which the `profiling`
profile emits), then aggregates self and inclusive samples per function.

    scripts/prof-top.py target/profiling/prof-bench.json.gz [--limit N] [--filter S]

--filter keeps only functions whose name/file contains S (e.g. `notema`), which
cuts through libc/runtime frames. Foreign modules are collapsed to `[lib]` rows.
For interactive exploration use `samply load <trace>` instead.
"""
import argparse
import gzip
import json
import os
import subprocess
import sys
from collections import defaultdict


def load(path):
    op = gzip.open if path.endswith(".gz") else open
    with op(path, "rt") as f:
        return json.load(f)


def text_vmaddr(binpath):
    try:
        out = subprocess.run(
            ["otool", "-l", binpath], capture_output=True, text=True
        ).stdout
    except FileNotFoundError:
        return 0x100000000
    seg = False
    for line in out.splitlines():
        s = line.split()
        if len(s) >= 2 and s[0] == "segname" and s[1] == "__TEXT":
            seg = True
        elif seg and len(s) >= 2 and s[0] == "vmaddr":
            return int(s[1], 16)
    return 0x100000000


def atos_map(binpath, addrs):
    """module-relative address -> 'symbol (file:line)', resolved in one atos call."""
    if not addrs or not os.path.exists(binpath):
        return {}
    base = text_vmaddr(binpath)
    args = ["atos", "-o", binpath, "-l", hex(base)] + [hex(base + a) for a in addrs]
    try:
        out = subprocess.run(args, capture_output=True, text=True).stdout.splitlines()
    except FileNotFoundError:
        return {}
    m = {}
    for a, line in zip(addrs, out):
        line = line.strip()
        # atos leaves unknown addresses as the bare hex; keep those as-is.
        m[a] = line.split(" (in ")[0] if " (in " in line else line
    return m


def summarize(profile):
    libs = profile["libs"]
    # Own modules: those whose recorded path exists on disk (bench/app binaries).
    own = {}  # lib index -> {addr: symbol}
    self_ct = defaultdict(int)
    incl_ct = defaultdict(int)
    total = 0

    for thread in profile["threads"]:
        strings = thread["stringArray"]
        func = thread["funcTable"]
        frame = thread["frameTable"]
        stab = thread["stackTable"]
        res = thread["resourceTable"]

        def func_lib(fn):
            r = func["resource"][fn]
            if r is None or r < 0:
                return None
            return res["lib"][r] if "lib" in res else None

        # Collect addresses per own-lib for batch symbolication.
        want = defaultdict(set)
        for fr in range(frame["length"]):
            li = func_lib(frame["func"][fr])
            if li is None:
                continue
            path = libs[li].get("path") or ""
            if os.path.exists(path):
                want[li].add(frame["address"][fr])
        for li, addrs in want.items():
            if li not in own:
                own[li] = {}
            own[li].update(atos_map(libs[li]["path"], sorted(addrs)))

        def flabel(fr):
            fn = frame["func"][fr]
            li = func_lib(fn)
            if li is not None and li in own:
                a = frame["address"][fr]
                return own[li].get(a, hex(a))
            return "[" + (libs[li]["name"] if li is not None else "unknown") + "]"

        for s in thread["samples"]["stack"]:
            if s is None:
                continue
            total += 1
            self_ct[flabel(stab["frame"][s])] += 1
            seen = set()
            cur = s
            while cur is not None:
                lbl = flabel(stab["frame"][cur])
                if lbl not in seen:
                    seen.add(lbl)
                    incl_ct[lbl] += 1
                cur = stab["prefix"][cur]
    return self_ct, incl_ct, total


def show(title, counts, total, limit, name_filter):
    print(f"\n=== {title} (total {total} samples) ===")
    rows = counts.items()
    if name_filter:
        nf = name_filter.lower()
        rows = [(k, v) for k, v in rows if nf in k.lower()]
    for name, ct in sorted(rows, key=lambda kv: -kv[1])[:limit]:
        print(f"{ct:7d}  {100*ct/total:5.1f}%  {name}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("trace")
    ap.add_argument("--limit", type=int, default=30)
    ap.add_argument("--filter", default=None)
    args = ap.parse_args()
    self_ct, incl_ct, total = summarize(load(args.trace))
    if not total:
        print("no samples in trace", file=sys.stderr)
        sys.exit(1)
    show("Self time (leaf)", self_ct, total, args.limit, args.filter)
    show("Inclusive time", incl_ct, total, args.limit, args.filter)


if __name__ == "__main__":
    main()
