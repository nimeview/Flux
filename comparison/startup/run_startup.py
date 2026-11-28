#!/usr/bin/env python3
"""Hello-world start time and compile/check time. Median of 10.
Writes comparison/results/startup.json."""
import json, pathlib, statistics, subprocess, time, tempfile
HERE = pathlib.Path(__file__).parent
ROOT = HERE.parents[1]
FLUX = str(ROOT / "target/release/flux")

def med(cmd, n=10, cwd=HERE):
    ts = []
    for _ in range(n):
        t = time.perf_counter(); subprocess.run(cmd, cwd=cwd, capture_output=True, check=True); ts.append(time.perf_counter() - t)
    return statistics.median(ts) * 1000

tmp = tempfile.mkdtemp()
subprocess.run(["go", "build", "-o", f"{tmp}/hgo", "hello.go"], cwd=HERE, check=True)
subprocess.run(["rustc", "-O", "-o", f"{tmp}/hrs", "hello.rs"], cwd=HERE, check=True)
start = {
    "Flux VM (run, no cache)": [FLUX, "run", "--quiet", "--no-cache", "hello.fl"],
    "Flux VM (run, cached)": [FLUX, "run", "--quiet", "hello.fl"],
    "Python": ["python3", "hello.py"],
    "Ruby": ["ruby", "hello.rb"],
    "Lua": ["lua", "hello.lua"],
    "LuaJIT": ["luajit", "hello.lua"],
    "Node": ["node", "hello.js"],
    "Bun": ["bun", "hello.js"],
    "Go (built binary)": [f"{tmp}/hgo"],
    "Rust (built binary)": [f"{tmp}/hrs"],
}
res = {"hello_start_ms": {k: med(v) for k, v in start.items()}}
# compile / check of a bigger program
big = max((ROOT / "programs").rglob("*.fl"), key=lambda p: p.stat().st_size)
res["flux_check_largest_program"] = {"file": str(big.relative_to(ROOT)), "lines": len(big.read_text().splitlines()),
                                     "ms": med([FLUX, "check", str(big)], cwd=big.parent)}
res["compile_ms"] = {
    "go build hello": med(["go", "build", "-o", f"{tmp}/x", "hello.go"], n=5),
    "rustc -O hello": med(["rustc", "-O", "-o", f"{tmp}/y", "hello.rs"], n=5),
    "tsc hello": med([str(HERE.parent / "reactive/node_modules/.bin/tsc"), "--noEmit", "hello.js", "--allowJs", "--checkJs"], n=5),
}
for k, v in res["hello_start_ms"].items(): print(f"{k:26} {v:7.1f} ms")
print(res["flux_check_largest_program"]); print(res["compile_ms"])
(HERE.parent / "results/startup.json").write_text(json.dumps(res, indent=1))
