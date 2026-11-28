#!/usr/bin/env python3
"""Runs the reactive scenarios: Flux (VM, JIT) and the JS libraries on Node
and Bun. Best of RUNS per cell. Writes results/reactive.json.

Usage: python3 comparison/run_reactive.py [RUNS]
"""
import json, pathlib, subprocess, sys, time

HERE = pathlib.Path(__file__).parent
R = HERE / "reactive"
FLUX = HERE.parent / "target" / "release" / "flux"
RUNS = int(sys.argv[1]) if len(sys.argv) > 1 else 3
SCENARIOS = ["chain", "fan", "counters", "todos"]
EXPECT = {"chain": 1501500, "fan": 1000499500, "counters": 110999000, "todos": 968875}
LIBS = ["SolidJS 2", "Vue reactivity 3.5", "MobX 7", "Preact signals", "Svelte 5 runes"]


def timed(cmd, cwd):
    t = time.perf_counter()
    p = subprocess.run(["/usr/bin/time", "-l"] + cmd, cwd=cwd, capture_output=True, text=True)
    wall = time.perf_counter() - t
    rss = next((int(l.split()[0]) for l in p.stderr.splitlines() if "maximum resident" in l), 0)
    return p, wall, rss / 1e6


def flux(engine, s):
    p, wall, rss = timed([str(FLUX), "run", "--quiet", f"--{engine}", f"flux/{s}.fl"], R)
    if p.returncode != 0:
        return {"error": (p.stdout + p.stderr).strip().splitlines()[-1][:200]}
    parts = p.stdout.split()
    out = {"checksum": int(parts[0]), "wall_s": wall, "rss_mb": rss}
    if s == "counters":
        out["create_ms"], out["update_ms"] = float(parts[1]), float(parts[2])
    else:
        out["update_ms"] = float(parts[1])
    return out


def flux_plain(engine, s):
    p, wall, rss = timed([str(FLUX), "run", "--quiet", f"--{engine}", "flux/plain.fl", "--", s], R)
    if p.returncode != 0:
        return {"error": (p.stdout + p.stderr).strip().splitlines()[-1][:200]}
    seen, ms = p.stdout.split()
    return {"checksum": int(seen), "update_ms": float(ms), "wall_s": wall, "rss_mb": rss}


def elm(s):
    p, wall, rss = timed(["node", "elm/run.mjs", s], R)
    if p.returncode != 0:
        return {"error": p.stderr.strip()[-200:]}
    out = json.loads(p.stdout.strip().splitlines()[-1])
    out.update(wall_s=wall, rss_mb=rss)
    return out


def js(runtime, i, s):
    cmd = (["node", "--conditions=browser"] if runtime == "node" else ["bun", "--conditions=browser"])
    p, wall, rss = timed(cmd + ["bench.mjs", str(i), s], R)
    if p.returncode != 0:
        err = [l for l in (p.stderr).splitlines() if "Error" in l]
        return {"error": (err[0] if err else p.stderr.strip()[-200:])[:200]}
    out = json.loads(p.stdout.strip().splitlines()[-1])
    out.update(wall_s=wall, rss_mb=rss)
    return out


def best(fn):
    runs = [fn() for _ in range(RUNS)]
    ok = [r for r in runs if "error" not in r]
    return min(ok, key=lambda r: r["update_ms"]) if ok else runs[0]


subprocess.run(["node", "adapters/build.mjs"], cwd=R, check=True)
subprocess.run([str(R / "node_modules/.bin/elm"), "make", "src/Main.elm", "--optimize", "--output=main.js"],
               cwd=R / "elm", check=True, capture_output=True)
results = []
for s in SCENARIOS:
    cells = [("Flux VM", lambda s=s: flux("vm", s)), ("Flux JIT", lambda s=s: flux("jit", s))]
    cells += [("Flux VM, no graph", lambda s=s: flux_plain("vm", s)),
              ("Flux JIT, no graph", lambda s=s: flux_plain("jit", s)),
              ("Elm 0.19, no graph (node)", lambda s=s: elm(s))]
    for rt in ("node", "bun"):
        for i, name in enumerate(LIBS):
            cells.append((f"{name} ({rt})", lambda rt=rt, i=i, s=s: js(rt, i, s)))
    for name, fn in cells:
        r = best(fn)
        r.update(runner=name, scenario=s)
        r["matches"] = r.get("checksum") == EXPECT[s]
        results.append(r)
        if "error" in r:
            print(f"{s:9} {name:28} ERROR {r['error']}")
        else:
            extra = f" create {r['create_ms']:7.1f}ms" if "create_ms" in r else ""
            print(f"{s:9} {name:28} update {r['update_ms']:8.1f}ms{extra}  wall {r['wall_s']:6.2f}s  rss {r['rss_mb']:6.1f}MB  {'ok' if r['matches'] else 'MISMATCH'}")

out = HERE / "results" / "reactive.json"
out.parent.mkdir(exist_ok=True)
out.write_text(json.dumps({"runs": RUNS, "results": results}, indent=1))
print("wrote", out)
