#!/usr/bin/env python3
"""Compiles/runs each case in every language and stores the first message.
Writes comparison/results/errors.json. Usage: python3 comparison/errors/run_errors.py"""
import json, pathlib, re, shutil, subprocess, sys, tempfile
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from cases import CASES

HERE = pathlib.Path(__file__).parent
ROOT = HERE.parents[1]
NM = HERE.parent / "reactive" / "node_modules" / ".bin"
ELM_JSON = (HERE.parent / "reactive" / "elm" / "elm.json").read_text()
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def sh(cmd, cwd):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=120)
    return p.returncode, ANSI.sub("", (p.stdout + p.stderr)).strip()


def run(lang, src, d):
    d = pathlib.Path(d)
    if lang == "fl":
        (d / "main.fl").write_text(src)
        code, out = sh([str(ROOT / "target/release/flux"), "check", "main.fl"], d)
        stage = "check"
        if code == 0:
            code, out = sh([str(ROOT / "target/release/flux"), "run", "--quiet", "main.fl"], d)
            stage = "run"
    elif lang == "rs":
        (d / "main.rs").write_text(src)
        code, out = sh(["rustc", "--edition", "2024", "-o", "main", "main.rs"], d)
        stage = "compile"
        if code == 0:
            code, out = sh(["./main"], d); stage = "run"
    elif lang == "ts":
        (d / "main.ts").write_text(src)
        code, out = sh([str(NM / "tsc"), "--strict", "--noEmit", "--target", "es2022", "--lib", "es2022,dom", "main.ts"], d)
        stage = "compile"
        if code == 0:
            stage = "run"
            shutil.copy(d / "main.ts", d / "main.mts")
            code, out = sh(["node", "--experimental-strip-types", "--no-warnings", "main.mts"], d)
            out = out or "(no error; printed nothing)"
    elif lang == "py":
        (d / "main.py").write_text(src)
        code, out = sh([sys.executable, "main.py"], d)
        stage = "run"
    else:
        (d / "src").mkdir()
        (d / "src" / "Main.elm").write_text(src)
        (d / "elm.json").write_text(ELM_JSON)
        code, out = sh([str(NM / "elm"), "make", "src/Main.elm", "--output=/dev/null"], d)
        stage = "compile"
    return {"stage": stage, "exit": code, "message": out[:2500]}


results = {}
for name, case in CASES.items():
    results[name] = {"what": case["what"]}
    for lang in ("fl", "rs", "ts", "py", "elm"):
        with tempfile.TemporaryDirectory() as d:
            r = run(lang, case[lang], d)
        r["source"] = case[lang]
        results[name][lang] = r
        first = r["message"].splitlines()[0] if r["message"] else ""
        print(f"{name:14} {lang:4} {r['stage']:8} exit {r['exit']}: {first[:110]}")
(HERE.parent / "results" / "errors.json").write_text(json.dumps(results, indent=1, ensure_ascii=False))
