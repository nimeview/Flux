#!/usr/bin/env python3
"""Leak check: the same workloads with 1x, 2x, 4x, 8x rounds. If memory no
longer reachable is reclaimed, peak RSS stays flat as rounds grow.
Writes comparison/results/stability_stress.json."""
import json, pathlib, subprocess

HERE = pathlib.Path(__file__).parent
FLUX = str(HERE.parents[1] / "target" / "release" / "flux")

COMPONENTS = """event Tick
event Make(n: Int)
event Clear

live fn Counter(start: Int) {
    pub let mut n = start
    pub derived double = n * 2

    on Tick {
        n += 1
    }
}

let mut all: [Counter] = []

derived total = all.map(|c| c.double).reduce(0, |x, y| x + y)

on Make(n) {
    let mut i = 0
    while i < n {
        all.push(Counter(i))
        i += 1
    }
}

on Clear {
    all = []
}

fn main() {
    let mut r = 0
    while r < ROUNDS {
        emit Make(1000)
        emit Tick()
        emit Tick()
        emit Clear()
        r += 1
    }
}
"""

DATA = """struct P {
    name: String,
    n: Int,
}

enum Tree {
    Leaf,
    Node { l: Tree, r: Tree },
}

fn make(d: Int) -> Tree {
    if d == 0 { Tree::Leaf } else { Tree::Node { l: make(d - 1), r: make(d - 1) } }
}

fn main() {
    let mut r = 0
    let mut keep = 0
    while r < ROUNDS {
        let mut xs: [P] = []
        let mut i = 0
        while i < 2000 {
            xs.push(P { name: "item-{i}-{r}", n: i })
            i += 1
        }
        let t = make(10)
        keep = keep + xs.filter(|p| p.n % 2 == 0).len()
        r += 1
    }
}
"""

results = {}
for name, src in [("components", COMPONENTS), ("data", DATA)]:
    rows = []
    for rounds in (25, 50, 100, 200):
        path = HERE / f"_stress_{name}.fl"
        path.write_text(src.replace("ROUNDS", str(rounds)))
        p = subprocess.run(["/usr/bin/time", "-l", FLUX, "run", "--quiet", "--vm", "--no-cache", str(path)],
                           capture_output=True, text=True)
        rss = next(int(l.split()[0]) for l in p.stderr.splitlines() if "maximum resident" in l)
        rows.append({"rounds": rounds, "rss_mb": rss / 1e6, "exit": p.returncode,
                     "err": p.stderr.splitlines()[0] if p.returncode else ""})
        print(name, rounds, f"{rss/1e6:.1f} MB", "exit", p.returncode, rows[-1]["err"][:150], flush=True)
        path.unlink()
    results[name] = rows
(HERE.parent / "results" / "stability_stress.json").write_text(json.dumps(results, indent=1))
