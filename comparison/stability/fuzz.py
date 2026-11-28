#!/usr/bin/env python3
"""Mutation fuzzer for the flux binary (no cargo-fuzz needed).

Seeds: every .fl in the repo (outside comparison/ and target/) and every
```flux block of the book. Each mutant is checked with `flux check`; one that
passes is also run on the VM and the JIT and the outputs compared.

Classifies: panic (exit 101 / "panicked"), crash (killed by a signal),
hang (timeout), vm/jit mismatch. Everything else (exit 0/1 with a
diagnostic) is a normal outcome.

Usage: python3 comparison/stability/fuzz.py [MUTANTS] [SEED] [JOBS]
Writes comparison/results/stability_fuzz.json and keeps each failing input
under comparison/results/raw/fuzz/.
"""
import concurrent.futures as cf
import hashlib, json, os, pathlib, random, re, subprocess, sys, tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
FLUX = str(ROOT / "target" / "release" / "flux")
OUT = ROOT / "comparison" / "results"
RAW = OUT / "raw" / "fuzz"
COUNT = int(sys.argv[1]) if len(sys.argv) > 1 else 5000
SEED = int(sys.argv[2]) if len(sys.argv) > 2 else 1
JOBS = int(sys.argv[3]) if len(sys.argv) > 3 else 6
TIMEOUT = 10

TOKENS = ["fn", "act fn", "track fn", "live fn", "let", "let mut", "derived", "const", "watch", "on", "emit",
          "event", "struct", "enum", "impl", "trait", "match", "if", "else", "while", "for", "in", "return",
          "break", "continue", "pub", "import", "extern fn", "snap", "with", "as", "mut", "self", "Self",
          "{", "}", "(", ")", "[", "]", "<", ">", "=", "==", "+=", "->", "=>", "::", ".", ",", ":", "?", "|",
          "&", "^", "<<", ">>", "!", "-", "+", "*", "/", "%", "&&", "||", "0", "1", "-1",
          "9223372036854775807", "-9223372036854775808", "1e308", "0.0", "\"\"", "\"é𝔸\"", "[]", "()",
          "Int", "Float", "String", "Bool", "Option", "Result", "Some(1)", "None", "Ok(1)", "Err(\"x\")",
          "x", "y", "n", "xs", "main", "Tick", "_", "\n", "//", "/*", "'", "\\", "\u0000", "é", "𝔸"]


def seeds():
    out = []
    for p in ROOT.rglob("*.fl"):
        rel = p.relative_to(ROOT).parts
        if rel[0] in ("target", "comparison") or "node_modules" in rel:
            continue
        out.append(p.read_text(errors="replace"))
    for md in (ROOT / "docs" / "flux" / "src").glob("*.md"):
        for block in re.findall(r"```flux[^\n]*\n(.*?)```", md.read_text(), re.S):
            out.append(block)
    return out


def mutate(src, rng):
    lines = src.split("\n")
    for _ in range(rng.choice([1, 1, 1, 2, 3])):
        op = rng.randrange(9)
        if not lines:
            lines = [""]
        i = rng.randrange(len(lines))
        if op == 0:                                   # delete a line
            del lines[i]
        elif op == 1:                                 # duplicate a line
            lines.insert(i, lines[i])
        elif op == 2 and len(lines) > 1:              # swap two lines
            j = rng.randrange(len(lines))
            lines[i], lines[j] = lines[j], lines[i]
        elif op == 3:                                 # insert a token
            line = lines[i]
            k = rng.randrange(len(line) + 1)
            lines[i] = line[:k] + " " + rng.choice(TOKENS) + " " + line[k:]
        elif op == 4:                                 # delete a word
            words = re.split(r"(\W)", lines[i])
            if words:
                del words[rng.randrange(len(words))]
                lines[i] = "".join(words)
        elif op == 5:                                 # replace a number
            lines[i] = re.sub(r"\b\d+\b", lambda m: rng.choice(["0", "-1", "9223372036854775807", "2147483648", "255", "256"]), lines[i], count=1)
        elif op == 6:                                 # rename an identifier to another one in the file
            names = re.findall(r"\b[a-z_][a-z0-9_]*\b", src)
            if names:
                a, b = rng.choice(names), rng.choice(names)
                lines[i] = re.sub(rf"\b{re.escape(a)}\b", b, lines[i])
        elif op == 7:                                 # truncate the file
            text = "\n".join(lines)
            return text[: rng.randrange(len(text) + 1)]
        else:                                         # splice a line from another place
            lines.insert(rng.randrange(len(lines) + 1), lines[rng.randrange(len(lines))][::-1] if rng.random() < 0.1 else lines[rng.randrange(len(lines))])
    return "\n".join(lines)


def run(args, cwd):
    try:
        p = subprocess.run([FLUX] + args, cwd=cwd, capture_output=True, text=True, timeout=TIMEOUT,
                           stdin=subprocess.DEVNULL, errors="replace")
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return "timeout", "", ""


def classify(code, err):
    if code == "timeout":
        return "hang"
    if code == 101 or "panicked" in err:
        return "panic"
    if isinstance(code, int) and code < 0:
        return f"crash(signal {-code})"
    return None


def one(job):
    idx, src = job
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "main.fl")
        with open(path, "w") as f:
            f.write(src)
        code, out, err = run(["check", path], d)
        bad = classify(code, err)
        if bad:
            return {"i": idx, "stage": "check", "kind": bad, "err": err[-600:], "src": src}
        if code != 0:
            return {"i": idx, "ok": False}
        if re.search(r"\b(now|args|read_line|read_file|every|after)\b", src):
            return {"i": idx, "ok": True, "ran": False}     # nondeterministic or needs input
        vm = run(["run", "--quiet", "--vm", "--no-cache", path], d)
        bad = classify(vm[0], vm[2])
        if bad:
            return {"i": idx, "stage": "run-vm", "kind": bad, "err": vm[2][-600:], "src": src}
        jit = run(["run", "--quiet", "--jit", "--no-cache", path], d)
        bad = classify(jit[0], jit[2])
        if bad:
            return {"i": idx, "stage": "run-jit", "kind": bad, "err": jit[2][-600:], "src": src}
        if (vm[0], vm[1]) != (jit[0], jit[1]):
            return {"i": idx, "stage": "diff", "kind": "vm/jit mismatch",
                    "err": f"vm exit {vm[0]} out {vm[1][-300:]!r}\njit exit {jit[0]} out {jit[1][-300:]!r}", "src": src}
        return {"i": idx, "ok": True, "ran": True}


def signature(r):
    if r["kind"] == "hang":                 # keep every hang: each must be checked by hand
        return f'{r["stage"]}:hang:{hashlib.sha1(r["src"].encode()).hexdigest()[:10]}'
    m = re.search(r"panicked at ([^\n]+)", r["err"])
    return f'{r["stage"]}:{r["kind"]}:{m.group(1) if m else r["err"].strip()[-120:]}'


def main():
    rng = random.Random(SEED)
    corpus = seeds()
    jobs = [(i, mutate(rng.choice(corpus), rng)) for i in range(COUNT)]
    stats = {"mutants": COUNT, "seed": SEED, "corpus": len(corpus), "checked_ok": 0, "ran": 0}
    found = {}
    RAW.mkdir(parents=True, exist_ok=True)
    with cf.ThreadPoolExecutor(JOBS) as pool:
        for n, r in enumerate(pool.map(one, jobs), 1):
            if "kind" in r:
                sig = signature(r)
                if sig not in found:
                    found[sig] = {"kind": r["kind"], "stage": r["stage"], "count": 0, "example": r["i"], "err": r["err"]}
                    name = hashlib.sha1(sig.encode()).hexdigest()[:10]
                    (RAW / f"{name}.fl").write_text(r["src"])
                    found[sig]["file"] = f"results/raw/fuzz/{name}.fl"
                    print(f"[{n}] NEW {r['stage']} {r['kind']}: {sig[:160]}", flush=True)
                found[sig]["count"] += 1
            else:
                stats["checked_ok"] += r.get("ok", False)
                stats["ran"] += r.get("ran", False)
            if n % 1000 == 0:
                print(f"{n}/{COUNT} done, {len(found)} distinct failures", flush=True)
    stats["failures"] = [dict(signature=k, **v) for k, v in found.items()]
    OUT.mkdir(exist_ok=True)
    (OUT / f"stability_fuzz_{SEED}.json").write_text(json.dumps(stats, indent=1))
    print(json.dumps({k: v for k, v in stats.items() if k != "failures"}), "distinct failures:", len(found))


if __name__ == "__main__":
    main()
