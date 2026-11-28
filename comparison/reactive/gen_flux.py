#!/usr/bin/env python3
"""Writes the Flux versions of the reactive scenarios into flux/.

Each program times its update phase with time.now() and prints
"<checksum> <ms>"; the JS versions in bench.mjs print the same checksum.
"""
import pathlib

N = 1000      # nodes in the chain / fan, counters
U = 1000      # updates for chain and fan
T = 100       # ticks for the counters
OUT = pathlib.Path(__file__).parent / "flux"
OUT.mkdir(exist_ok=True)

HEAD = "import io { println }\nimport time { now }\n\nevent Seen(v: Int)\n\non Seen(v) {\n    seen += v\n}\n\n"


def chain():
    lines = [HEAD, "event Set(v: Int)\n\nlet mut s = 0\n\nlet mut seen = 0\n\n"]
    lines.append("derived d0 = s + 1\n")
    for i in range(1, N):
        lines.append(f"derived d{i} = d{i-1} + 1\n")
    lines.append(f"\non Set(v) {{\n    s = v\n}}\n\nwatch d{N-1} {{\n    emit Seen(d{N-1})\n}}\n\n")
    lines.append(main_loop(f"emit Set(i + 1)", U))
    return "".join(lines)


def fan():
    lines = [HEAD, "event Set(v: Int)\n\nlet mut s = 0\n\nlet mut seen = 0\n\n"]
    for i in range(N):
        lines.append(f"derived a{i} = s + {i}\n")
    # sum in groups of 50 so no expression is enormous
    groups = []
    for g in range(0, N, 50):
        name = f"g{g // 50}"
        groups.append(name)
        lines.append(f"derived {name} = " + " + ".join(f"a{i}" for i in range(g, g + 50)) + "\n")
    lines.append("derived total = " + " + ".join(groups) + "\n")
    lines.append("\non Set(v) {\n    s = v\n}\n\nwatch total {\n    emit Seen(total)\n}\n\n")
    lines.append(main_loop("emit Set(i + 1)", U))
    return "".join(lines)


def counters():
    return HEAD + f"""event Tick

event Make(n: Int)

live fn Counter(start: Int) {{
    pub let mut n = start
    pub derived double = n * 2

    on Tick {{
        n += 1
    }}
}}

let mut all: [Counter] = []

let mut seen = 0

derived total = all.map(|c| c.double).reduce(0, |x, y| x + y)

on Make(n) {{
    let mut i = 0
    while i < n {{
        all.push(Counter(i))
        i += 1
    }}
}}

watch total {{
    emit Seen(total)
}}

fn main() {{
    let t0 = now()
    emit Make({N})
    let t1 = now()
    let mut i = 0
    while i < {T} {{
        emit Tick()
        i += 1
    }}
    let t2 = now()
    println("{{snap(seen)}} {{t1 - t0}} {{t2 - t1}}")
}}
"""


def todos():
    return HEAD + f"""struct Todo {{
    id: Int,
    done: Bool,
}}

event Add(id: Int)

event Toggle(id: Int)

event Remove(id: Int)

let mut todos: [Todo] = []

let mut seen = 0

derived remaining = todos.filter(|t| !t.done).len()

on Add(id) {{
    todos.push(Todo {{ id: id, done: false }})
}}

on Toggle(id) {{
    todos = todos.map(|t| if t.id == id {{ t with {{ done: !t.done }} }} else {{ t }})
}}

on Remove(id) {{
    todos = todos.filter(|t| t.id != id)
}}

watch remaining {{
    emit Seen(remaining)
}}

fn main() {{
    let t0 = now()
    let mut i = 0
    while i < {N} {{
        emit Add(i)
        i += 1
    }}
    i = 0
    while i < {N} {{
        emit Toggle(i)
        i += 2
    }}
    i = 0
    while i < {N // 2} {{
        emit Remove(i)
        i += 1
    }}
    let t1 = now()
    println("{{snap(seen)}} {{t1 - t0}}")
}}
"""


def main_loop(body, count):
    return f"""fn main() {{
    let t0 = now()
    let mut i = 0
    while i < {count} {{
        {body}
        i += 1
    }}
    let t1 = now()
    println("{{snap(seen)}} {{t1 - t0}}")
}}
"""


for name, src in [("chain", chain()), ("fan", fan()), ("counters", counters()), ("todos", todos())]:
    (OUT / f"{name}.fl").write_text(src)
    print("wrote", OUT / f"{name}.fl")
