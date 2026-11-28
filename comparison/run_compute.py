"""Runs every compute benchmark in every language; records the best of N
runs: wall time, CPU time, retired instructions, and peak memory (macOS
`/usr/bin/time -l`). Every program must print the same result as Flux's.

  python3 comparison/run_compute.py [RUNS] [BENCH ...]
"""
import json, os, re, subprocess, sys, time

ROOT = os.path.dirname(os.path.abspath(__file__))
COMPUTE = os.path.join(ROOT, "compute")
FLUX = os.path.join(ROOT, "..", "target", "release", "flux")
BENCHES = ["fib", "binary_trees", "spectral_norm", "nbody", "fannkuch", "strings", "maps"]

def commands(bench):
    c = COMPUTE
    lj = os.path.join(c, f"{bench}.luajit.lua")
    return {
        "Flux VM": [FLUX, "run", "--quiet", "--no-cache", "--vm", os.path.join(c, f"{bench}.fl")],
        "Flux JIT": [FLUX, "run", "--quiet", "--no-cache", "--jit", os.path.join(c, f"{bench}.fl")],
        "Python": ["python3", os.path.join(c, f"{bench}.py")],
        "Ruby (YJIT)": ["ruby", "--yjit", os.path.join(c, f"{bench}.rb")],
        "Lua": ["lua", os.path.join(c, f"{bench}.lua")],
        "LuaJIT": ["luajit", lj if os.path.exists(lj) else os.path.join(c, f"{bench}.lua")],
        "Node": ["node", os.path.join(c, f"{bench}.js")],
        "Bun": ["bun", os.path.join(c, f"{bench}.js")],
        "Go": [os.path.join(c, "bin", f"{bench}.go.bin")],
        "Rust": [os.path.join(c, "bin", f"{bench}.rs.bin")],
    }

def measure(cmd):
    t = time.perf_counter()
    r = subprocess.run(["/usr/bin/time", "-l"] + cmd, capture_output=True, text=True)
    wall = time.perf_counter() - t
    err = r.stderr
    num = lambda pat: int(m.group(1)) if (m := re.search(pat, err)) else None
    real_user = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", err)
    return {
        "out": r.stdout.strip(),
        "ok": r.returncode == 0,
        "wall": wall,
        "cpu": float(real_user.group(2)) + float(real_user.group(3)) if real_user else None,
        "instructions": num(r"(\d+)\s+instructions retired"),
        "rss": num(r"(\d+)\s+maximum resident set size"),
    }

def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    benches = sys.argv[2:] or BENCHES
    out_path = os.path.join(ROOT, "results", "compute.json")
    results = json.load(open(out_path)) if os.path.exists(out_path) else {}
    for bench in benches:
        cmds = commands(bench)
        expected = None
        results[bench] = {}
        for lang, cmd in cmds.items():
            best = None
            for _ in range(runs):
                m = measure(cmd)
                if not m["ok"]:
                    best = m
                    break
                if best is None or m["wall"] < best["wall"]:
                    best = {**m, "instructions": min(filter(None, [m["instructions"], (best or {}).get("instructions")]), default=None)}
            if expected is None:
                expected = best["out"]
            best["matches"] = best["out"].split() == expected.split()
            results[bench][lang] = best
            print(f"{bench:14} {lang:12} {best['wall']:8.3f}s  rss {(best['rss'] or 0) / 1e6:7.1f} MB  {'ok' if best['matches'] else 'MISMATCH ' + best['out']}", flush=True)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    json.dump(results, open(out_path, "w"), indent=1)

main()
