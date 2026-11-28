#!/usr/bin/env python3
"""Builds comparison/report/index.html from comparison/results/*.json.

Usage: python3 comparison/report/build.py
The text is Russian (the report is for the project owner); the numbers all
come from the result files, so rerunning the benchmarks and this script
gives an updated page.
"""
import html
import json
import math
import pathlib

HERE = pathlib.Path(__file__).parent
NOW = HERE.parent / "results"
# The study itself is of 0.4; its results are kept in results/v0.4 once
# newer ones are measured. The "0.5" section compares the two.
RES = NOW / "v0.4" if (NOW / "v0.4").is_dir() else NOW


def load(name):
    return json.loads((RES / name).read_text())


def load_now(name):
    return json.loads((NOW / name).read_text())


compute = load("compute.json")
reactive = load("reactive.json")["results"]
startup = load("startup.json")
stress = load("stability_stress.json")
fuzz = [load("stability_fuzz_1.json"), load("stability_fuzz_2.json")]
errors = load("errors.json")
restrictions = load("restrictions.json")
matrix = json.loads((HERE.parent / "features" / "matrix.json").read_text())

e = html.escape


# ---------------------------------------------------------------- helpers

def fmt_s(x):
    if x < 0.1:
        return f"{x * 1000:.0f} мс"
    if x < 10:
        return f"{x:.2f} с"
    return f"{x:.1f} с"


def fmt_ms(x):
    return f"{x:.0f}" if x >= 10 else f"{x:.1f}"


def ratio_text(r):
    if r >= 1:
        return f"{r:.1f}×" if r < 10 else f"{r:.0f}×"
    inv = 1 / r
    return f"1/{inv:.1f}" if inv < 10 else f"1/{inv:.0f}"


# ---------------------------------------------------------------- compute

BENCH_NAMES = {
    "fib": "fib(30)", "binary_trees": "binary-trees 16", "spectral_norm": "spectral-norm 250",
    "nbody": "n-body 100k", "fannkuch": "fannkuch 9", "strings": "строки 200k", "maps": "словари 200k",
}
BENCH_WHAT = {
    "fib": "рекурсивные вызовы, целые",
    "binary_trees": "выделение и обход деревьев (enum)",
    "spectral_norm": "float, вложенные циклы",
    "nbody": "float, массивы структур",
    "fannkuch": "перестановки массивов",
    "strings": "сборка и разбор строк",
    "maps": "вставки и чтения словаря",
}
benches = list(compute.keys())
langs = list(compute[benches[0]].keys())


def geomean(lang):
    return math.exp(sum(math.log(compute[b][lang]["wall"] / compute[b]["Python"]["wall"]) for b in benches) / len(benches))


def heat_cell(r, wall):
    # log10 distance from Python, capped at 100x either way
    t = min(1.0, abs(math.log10(r)) / 2)
    pct = round(t * 100)
    side = "var(--slow)" if r > 1 else "var(--fast)"
    ink = "var(--on-strong)" if t > 0.55 else "var(--fg)"
    tip = f"{e(fmt_s(wall))} · {ratio_text(r)} от Python"
    return (f'<td class="heat" style="background: color-mix(in oklab, {side} {pct}%, var(--mid)); color: {ink}" '
            f'data-tip="{tip}">{ratio_text(r)}</td>')


def compute_table():
    head = "".join(f'<th scope="col"><span>{e(BENCH_NAMES[b])}</span><small>{e(BENCH_WHAT[b])}</small></th>' for b in benches)
    rows = []
    for lang in langs:
        cls = ' class="flux"' if lang.startswith("Flux") else ""
        cells = "".join(heat_cell(compute[b][lang]["wall"] / compute[b]["Python"]["wall"], compute[b][lang]["wall"]) for b in benches)
        g = geomean(lang)
        rows.append(f'<tr{cls}><th scope="row">{e(lang)}</th>{cells}{heat_cell(g, 0).replace(" data-tip=", " data-x=")}</tr>')
    return (f'<div class="scroll"><table class="heatmap"><thead><tr><th></th>{head}'
            f'<th scope="col"><span>среднее</span><small>геометрическое</small></th></tr></thead>'
            f'<tbody>{"".join(rows)}</tbody></table></div>')


def bar_chart(items, unit_fmt, log=True, width=640, row=26, label_w=190, note=""):
    """items: (label, value, is_flux, tip). Horizontal bars, one series."""
    vals = [v for _, v, _, _ in items if v]
    lo, hi = min(vals), max(vals)
    if log:
        lo = 10 ** math.floor(math.log10(lo))
        hi = 10 ** math.ceil(math.log10(hi))
        scale = lambda v: (math.log10(v) - math.log10(lo)) / (math.log10(hi) - math.log10(lo))
        ticks = [10 ** k for k in range(int(math.log10(lo)), int(math.log10(hi)) + 1)]
    else:
        lo = 0
        scale = lambda v: v / hi
        ticks = [hi * k / 4 for k in range(5)]
    plot_w = width - label_w - 70
    h = row * len(items) + 30
    out = [f'<svg class="bars" viewBox="0 0 {width} {h}" role="img" aria-label="{e(note)}">']
    for t in ticks:
        x = label_w + scale(t) * plot_w
        out.append(f'<line x1="{x:.1f}" x2="{x:.1f}" y1="0" y2="{h - 22}" class="grid"/>')
        out.append(f'<text x="{x:.1f}" y="{h - 6}" class="tick" text-anchor="middle">{e(unit_fmt(t))}</text>')
    for i, (label, v, flux, tip) in enumerate(items):
        y = i * row + 4
        out.append(f'<text x="{label_w - 10}" y="{y + row / 2 + 1}" class="lbl{" flux" if flux else ""}" text-anchor="end" dominant-baseline="middle">{e(label)}</text>')
        if v is None:
            out.append(f'<text x="{label_w + 6}" y="{y + row / 2 + 1}" class="val err" dominant-baseline="middle">{e(tip)}</text>')
            continue
        w = max(3, scale(v) * plot_w) if log else max(3, scale(v) * plot_w)
        out.append(f'<g class="hit" data-tip="{e(label)}: {e(tip)}"><rect x="{label_w}" y="{y + 5}" width="{w:.1f}" height="{row - 10}" rx="3" class="bar{" flux" if flux else ""}"/>'
                   f'<rect x="{label_w}" y="{y}" width="{plot_w + 60}" height="{row}" fill="transparent"/></g>')
        out.append(f'<text x="{label_w + w + 6:.1f}" y="{y + row / 2 + 1}" class="val" dominant-baseline="middle">{e(unit_fmt(v))}</text>')
    out.append("</svg>")
    return "".join(out)


def geomean_chart():
    items = sorted(((l, geomean(l)) for l in langs), key=lambda x: -x[1])
    return bar_chart([(l, g, l.startswith("Flux"), f"{ratio_text(g)} от Python") for l, g in items],
                     lambda v: ratio_text(v), note="Среднее время относительно Python")


# ---------------------------------------------------------------- reactive

SCEN = {
    "chain": ("Цепочка", "1000 derived подряд, 1000 обновлений источника"),
    "fan": ("Веер", "1000 derived от одного источника, сумма группами, 1000 обновлений"),
    "counters": ("1000 компонентов", "1000 счётчиков, общая сумма, 100 тиков"),
    "todos": ("Список дел", "1000 добавлений, 500 переключений, 500 удалений"),
}
REACT_ORDER = ["Flux VM", "Flux VM, no graph", "Flux JIT, no graph", "Elm 0.19, no graph (node)",
               "SolidJS 2 (node)", "Vue reactivity 3.5 (node)", "MobX 7 (node)", "Preact signals (node)", "Svelte 5 runes (node)"]
REACT_LABEL = {
    "Flux VM": "Flux (граф)", "Flux VM, no graph": "Flux без графа", "Flux JIT, no graph": "Flux JIT без графа",
    "Elm 0.19, no graph (node)": "Elm (без графа)", "SolidJS 2 (node)": "SolidJS 2", "Vue reactivity 3.5 (node)": "Vue 3.5",
    "MobX 7 (node)": "MobX 7", "Preact signals (node)": "Preact signals", "Svelte 5 runes (node)": "Svelte 5",
}


def rget(runner, scen):
    for r in reactive:
        if r["runner"] == runner and r["scenario"] == scen:
            return r
    return None


def reactive_charts():
    panels = []
    for s, (title, what) in SCEN.items():
        items = []
        for runner in REACT_ORDER:
            r = rget(runner, s)
            if r is None:
                continue
            if "error" in r:
                items.append((REACT_LABEL[runner], None, False, "переполнение стека"))
            else:
                items.append((REACT_LABEL[runner], max(r["update_ms"], 0.5), runner.startswith("Flux"),
                              f'{fmt_ms(r["update_ms"])} мс, память {r["rss_mb"]:.0f} МБ'))
        panels.append(f'<figure class="panel"><figcaption><b>{e(title)}</b><span>{e(what)}</span></figcaption>'
                      f'{bar_chart(items, lambda v: (fmt_ms(v) + " мс"), width=560, label_w=150, note=title)}</figure>')
    return '<div class="panels">' + "".join(panels) + "</div>"


def reactive_table():
    runners = []
    for r in reactive:
        if r["runner"] not in runners:
            runners.append(r["runner"])
    head = "".join(f"<th>{e(SCEN[s][0])}</th>" for s in SCEN)
    rows = []
    for runner in runners:
        cells = []
        for s in SCEN:
            r = rget(runner, s)
            if r is None:
                cells.append("<td>–</td>")
            elif "error" in r:
                cells.append('<td class="bad">стек</td>')
            else:
                cells.append(f'<td class="num">{fmt_ms(r["update_ms"])}<small> мс · {r["rss_mb"]:.0f} МБ</small></td>')
        cls = ' class="flux"' if runner.startswith("Flux") else ""
        rows.append(f"<tr{cls}><th scope=\"row\">{e(runner)}</th>{''.join(cells)}</tr>")
    return (f'<details><summary>Все прогоны, включая Bun и Flux JIT (время обновлений · пиковая память процесса)</summary>'
            f'<div class="scroll"><table class="plain"><thead><tr><th></th>{head}</tr></thead><tbody>{"".join(rows)}</tbody></table></div></details>')


# ---------------------------------------------------------------- startup

def startup_chart():
    items = sorted(startup["hello_start_ms"].items(), key=lambda x: -x[1])
    return bar_chart([(k, v, k.startswith("Flux"), f"{v:.1f} мс") for k, v in items],
                     lambda v: f"{v:.0f} мс" if v >= 10 else f"{v:.1f} мс", log=False, note="Запуск hello world")


# ---------------------------------------------------------------- matrix

LEVEL = {"y": ("да", "lv-y"), "p": ("частично", "lv-p"), "n": ("нет", "lv-n")}
AREA_RU = {"Types": "Типы", "Errors": "Ошибки", "Data": "Данные", "Modules": "Модули", "Concurrency": "Параллельность",
           "Reactivity": "Реактивность", "Host": "Хост", "Platforms": "Платформы", "Tooling": "Инструменты", "Maturity": "Зрелость"}
FEATURE_RU = {
    "Static type checking": "Статическая проверка типов", "No null / explicit absence": "Нет null, отсутствие явно",
    "Generics": "Обобщения", "Traits / interfaces": "Трейты / интерфейсы", "Sum types + exhaustive match": "Суммы типов и полный match",
    "Expected errors as values": "Ожидаемые ошибки как значения", "Runtime faults recoverable": "Сбои времени выполнения перехватываются",
    "No silent integer wrap": "Целые не переполняются молча", "Closures": "Замыкания", "Memory management": "Управление памятью",
    "Modules and visibility": "Модули и видимость", "Package manager and registry": "Пакеты и реестр",
    "Concurrency / parallelism": "Потоки / параллельность", "Timers / event loop": "Таймеры, цикл событий",
    "Reactive state in the language": "Реактивное состояние в языке", "Glitch-free, batched dependency graph": "Граф зависимостей, пачки без «глитчей»",
    "FFI / embedding": "FFI / встраивание", "Capability limits for programs": "Ограничение прав программы",
    "Web / browser": "Веб / браузер", "Native executables": "Нативные исполняемые файлы", "Formatter": "Форматтер",
    "Language server": "Языковой сервер (LSP)", "Test runner": "Запуск тестов", "REPL": "REPL",
    "Debugger / profiler": "Отладчик / профилировщик", "Doc generation": "Генерация документации", "Age / users": "Возраст / пользователи",
}


FLUX_NOTE_RU = {
    "inferred locals, nominal structs/enums": "вывод для локальных, номинальные struct/enum",
    "Option, no null": "Option, null нет",
    "bounded, monomorphized; no explicit type arguments": "с ограничениями, мономорфизация; без явных аргументов типа",
    "traits + bounds; no trait objects / dynamic dispatch": "трейты и ограничения; нет трейт-объектов и динамической диспетчеризации",
    "exhaustive; a tuple variant holds one value": "полнота проверяется; вариант-кортеж держит одно значение",
    "Result/Option, ? with exact error type": "Result/Option, ? с точным типом ошибки",
    "overflow, bounds, limits stop the batch/program": "переполнение, границы и лимиты останавливают пачку или программу",
    "overflow is a runtime error; checked_* methods": "переполнение — ошибка выполнения; методы checked_*",
    "capture locals by value; no mutable captures": "захват локальных по значению; изменять захваченное нельзя",
    "reference counting of immutable values; no cycles possible": "счётчик ссылок неизменяемых значений; циклов быть не может",
    "file modules, pub, private fields, source ownership (E032)": "модули-файлы, pub, приватные поля, владение источником (E032)",
    "path dependencies + lockfile; no registry": "зависимости по пути и lock-файл; реестра нет",
    "single-threaded; host may send events from threads": "один поток; хост может слать события из других потоков",
    "after, every, now, stop": "after, every, now, stop",
    "sources, derived, events, watches, components; static purity and cycle checks": "источники, derived, события, watch, компоненты; статическая проверка чистоты и циклов",
    "specified batches; each watch once per batch": "пачки по спецификации; каждый watch раз за пачку",
    "extern fn supplied by a Rust host": "extern fn, которые даёт хост на Rust",
    "CLI has full file/process access; --target only removes modules at compile time": "у CLI полный доступ к файлам и процессам; --target лишь убирает модули при компиляции",
    "web target is a compile-time check; wasm is a stub": "цель web — только проверка при компиляции; wasm — заглушка",
    "VM interpreter; Cranelift JIT experimental": "интерпретатор VM; JIT на Cranelift экспериментальный",
    "diagnostics, hover, completion, definition, rename, symbols": "диагностика, hover, дополнение, переход, переименование, символы",
    "flux test (fn test_*)": "flux test (fn test_*)",
    "neither": "ни того, ни другого",
    "0.4, months old, one team": "0.4, несколько месяцев, одна команда",
}


def matrix_table():
    cols = "".join(f'<th scope="col"{" class=\"flux\"" if i == 0 else ""}>{e(c)}</th>' for i, c in enumerate(matrix["columns"]))
    rows, last = [], None
    for row in matrix["rows"]:
        area = AREA_RU.get(row["area"], row["area"])
        if area != last:
            rows.append(f'<tr class="area"><th colspan="{len(matrix["columns"]) + 1}">{e(area)}</th></tr>')
            last = area
        cells = []
        for i, (lv, note) in enumerate(row["cells"]):
            word, cls = LEVEL[lv]
            extra = " flux" if i == 0 else ""
            tip = f' data-tip="{e(note)}"' if note else ""
            if i == 0:
                note = FLUX_NOTE_RU.get(note, note)
                tip = f' data-tip="{e(note)}"' if note else ""
            cells.append(f'<td class="{cls}{extra}"{tip}><span class="lv">{word}</span>{f"<small>{e(note)}</small>" if note and i == 0 else ""}</td>')
        rows.append(f'<tr><th scope="row">{e(FEATURE_RU.get(row["feature"], row["feature"]))}</th>{"".join(cells)}</tr>')
    return f'<div class="scroll"><table class="matrix"><thead><tr><th></th>{cols}</tr></thead><tbody>{"".join(rows)}</tbody></table></div>'


# ---------------------------------------------------------------- restrictions

def verdict_bar():
    counts = {"Necessary": 0, "Justified but relaxable": 0, "Arbitrary": 0}
    for r in restrictions:
        counts[r["verdict"]] += 1
    total = sum(counts.values())
    names = {"Necessary": ("необходимо", "v-need"), "Justified but relaxable": ("обосновано, но можно ослабить", "v-relax"),
             "Arbitrary": ("произвольно", "v-arb")}
    segs = "".join(f'<div class="{names[k][1]}" style="flex: {v}" data-tip="{e(names[k][0])}: {v} из {total}"><b>{v}</b></div>' for k, v in counts.items())
    legend = "".join(f'<span><i class="{names[k][1]}"></i>{e(names[k][0])} · {v}</span>' for k, v in counts.items())
    return f'<div class="stack">{segs}</div><div class="legend">{legend}</div>', counts, total


ARB_RU = {
    3: "Запрет циклов в derived (в книге), хотя бинарник их пропускает и вреда нет",
    4: "Запрет управляющих конструкций в функциях, которые вызывает derived",
    21: "Присваивание на верхнем уровне — ошибка разбора, хотя верхнеуровневый `if` может присваивать",
    50: "Член компонента не может деструктурировать",
    51: "Функция-член компонента не может быть обобщённой",
    59: "`emit` требует цепочку имён: экземпляр из списка нужно сначала привязать к имени",
    67: "В теле `live fn` нельзя объявить тип, вложенный компонент, импорт или `const`",
    68: "Нет явных аргументов типа при вызове обобщённой функции",
    70: "Вариант enum с кортежем держит одно значение: `Node(Tree, Tree)` не разбирается",
    71: "`if` в позиции выражения не допускает `else if`",
    72: "В ветках `if`-выражения нельзя писать операторы",
    75: "Нет литерала пустого словаря",
    80: "`return` запрещён в теле замыкания",
    82: "Функции нельзя напечатать или вставить в строку",
    87: "Метод с собственными параметрами типа не может иметь `mut self`",
    88: "У методов нет `mut`-параметров, кроме `mut self`",
    91: "Нет кортежа из одного значения `(x,)`",
    92: "Нет выражения `()`, хотя тип Unit есть",
    99: "Нет составных битовых присваиваний `&=`, `|=`…",
}


def arbitrary_list():
    items = [f'<li><code>#{r["n"]}</code> {e(ARB_RU.get(r["n"], r["title"]))} <span class="score" data-tip="оценка сообщения / книги, 0–3">{r["msg"]}/{r["book"]}</span></li>'
             for r in restrictions if r["verdict"] == "Arbitrary"]
    return '<ul class="arb">' + "".join(items) + "</ul>"


def mean_scores():
    num = lambda s: float(s.split("–")[0])
    m = sum(num(r["msg"]) for r in restrictions) / len(restrictions)
    b = sum(num(r["book"]) for r in restrictions) / len(restrictions)
    return m, b


# ---------------------------------------------------------------- errors

ERR_LANG = [("fl", "Flux"), ("rs", "Rust"), ("ts", "TypeScript"), ("py", "Python"), ("elm", "Elm")]
ERR_RU = {
    "type_mismatch": "Строка вместо числа", "typo": "Опечатка в имени переменной", "nonexhaustive": "match без одного варианта",
    "no_method": "Опечатка в имени метода", "immutable": "Присваивание неизменяемому", "arity": "Не хватает аргумента",
    "optional": "Option используется как число",
}
# Author scores, five cells changed to Codex's after its review. My reading of each message: does it name the problem, point at the place, and give the fix?
# 3 = what + where + correct fix or suggestion; 2 = what + where; 1 = vague; 0 = no error / misleading.
ERR_SCORE = {
    "type_mismatch": {"fl": 2, "rs": 2, "ts": 2, "py": 2, "elm": 3},
    "typo": {"fl": 2, "rs": 3, "ts": 3, "py": 3, "elm": 3},
    "nonexhaustive": {"fl": 2, "rs": 3, "ts": 1, "py": 0, "elm": 3},
    "no_method": {"fl": 0, "rs": 3, "ts": 3, "py": 2, "elm": 3},
    "immutable": {"fl": 3, "rs": 3, "ts": 2, "py": 0, "elm": 2},
    "arity": {"fl": 2, "rs": 3, "ts": 2, "py": 2, "elm": 2},
    "optional": {"fl": 2, "rs": 2, "ts": 2, "py": 2, "elm": 3},
}


def first_line(case, lang):
    m = errors[case][lang]["message"]
    lines = [l.strip() for l in m.splitlines() if l.strip() and not l.strip().startswith(("Verifying", "Dependencies", "Compiling", "(", "...", "Detected", "Traceback", "File \"", "flux::"))]
    if lang == "elm":
        lines = [l for l in lines if not l.startswith("--")]
    if lang == "py" and errors[case][lang]["exit"] == 0:
        return "ошибки нет, напечатано: " + m.strip()
    if lang == "py":
        lines = [l for l in m.splitlines() if "Error" in l] or lines
    return lines[0] if lines else m[:120]


def errors_table():
    head = "".join(f'<th scope="col"{" class=\"flux\"" if k == "fl" else ""}>{n}</th>' for k, n in ERR_LANG)
    rows = []
    totals = {k: 0 for k, _ in ERR_LANG}
    for case in errors:
        cells = []
        for k, _ in ERR_LANG:
            s = ERR_SCORE[case][k]
            totals[k] += s
            stage = {"check": "проверка", "compile": "компиляция", "run": "запуск"}[errors[case][k]["stage"]]
            cells.append(f'<td class="msg s{s}{" flux" if k == "fl" else ""}"><span class="dot">{s}</span><code>{e(first_line(case, k)[:150])}</code><small>{stage}</small></td>')
        rows.append(f'<tr><th scope="row">{e(ERR_RU[case])}</th>{"".join(cells)}</tr>')
    tot = "".join(f'<td class="num{" flux" if k == "fl" else ""}"><b>{totals[k]}</b> / {3 * len(errors)}</td>' for k, _ in ERR_LANG)
    return (f'<div class="scroll"><table class="errors"><thead><tr><th></th>{head}</tr></thead><tbody>{"".join(rows)}'
            f'<tr class="total"><th scope="row">Сумма баллов</th>{tot}</tr></tbody></table></div>'), totals


# ---------------------------------------------------------------- page

def fuzz_numbers():
    mutants = sum(f["mutants"] for f in fuzz)
    ok = sum(f["checked_ok"] for f in fuzz)
    ran = sum(f["ran"] for f in fuzz)
    kinds = {}
    for f in fuzz:
        for x in f["failures"]:
            kinds[x["kind"]] = kinds.get(x["kind"], 0) + x["count"]
    return mutants, ok, ran, kinds


# The same scale as ERR_SCORE, for the 0.5 messages (results/errors.json).
ERR_SCORE_05 = {
    "type_mismatch": 2,
    "typo": 3,
    "nonexhaustive": 3,
    "no_method": 3,
    "immutable": 3,
    "arity": 3,
    "optional": 3,
}


def release_section():
    """0.4 against 0.5 for Flux: the same benchmarks and mistakes."""
    if RES == NOW:
        return ""
    c04, c05 = compute, load_now("compute.json")
    instr = load_now("release_05.json")
    e05 = load_now("errors.json")
    s05 = load_now("startup.json")

    def gm(data, lang):
        return math.exp(sum(math.log(data[b][lang]["wall"] / data[b]["Python"]["wall"]) for b in data) / len(data))

    rows = [
        ("Вычисления, VM: среднее от Python", ratio_text(gm(c04, "Flux VM")), ratio_text(gm(c05, "Flux VM"))),
        ("binary-trees, VM", fmt_s(c04["binary_trees"]["Flux VM"]["wall"]), fmt_s(c05["binary_trees"]["Flux VM"]["wall"])),
        ("fib(30), VM", fmt_s(c04["fib"]["Flux VM"]["wall"]), fmt_s(c05["fib"]["Flux VM"]["wall"])),
        *(
            (f"{SCEN[k][0]}: инструкций, весь запуск", f"{v[0] / 1e9:.2f} млрд", f"{v[1] / 1e9:.2f} млрд")
            for k, v in instr.items()
            if k in SCEN
        ),
        ("Запуск hello world", f'{startup["hello_start_ms"]["Flux VM (run, no cache)"]:.1f} мс', f'{s05["hello_start_ms"]["Flux VM (run, no cache)"]:.1f} мс'),
        ("Сообщения об ошибках (7 случаев)", f'{sum(ERR_SCORE[c]["fl"] for c in ERR_SCORE)} / 21', f"{sum(ERR_SCORE_05.values())} / 21"),
    ]
    body = "".join(f'<tr><th scope="row">{e(a)}</th><td class="num">{e(b)}</td><td class="num"><b>{e(c)}</b></td></tr>' for a, b, c in rows)
    msgs = "".join(
        f'<tr><th scope="row">{e(ERR_RU[c])}</th><td><code>{e(first_line(c, "fl"))[:160]}</code></td>'
        f'<td><code>{e(first_line_now(e05, c))[:160]}</code></td></tr>'
        for c in ERR_RU
    )
    return (
        '<div class="scroll"><table class="plain"><thead><tr><th></th><th>0.4</th><th>0.5</th></tr></thead>'
        f"<tbody>{body}</tbody></table></div>"
        '<details><summary>Сообщения Flux на те же ошибки: 0.4 и 0.5</summary><div class="scroll"><table class="plain msgs">'
        f"<thead><tr><th></th><th>0.4</th><th>0.5</th></tr></thead><tbody>{msgs}</tbody></table></div></details>"
    )


def first_line_now(errs, case):
    m = errs[case]["fl"]["message"]
    keep = [l.strip() for l in m.splitlines() if l.strip().startswith(("x ", "help:"))]
    return " · ".join(keep[:2]) if keep else m[:120]


def build():
    mutants, ok, ran, kinds = fuzz_numbers()
    vbar, vcounts, vtotal = verdict_bar()
    msg_mean, book_mean = mean_scores()
    etable, etotals = errors_table()
    gm_vm, gm_jit = geomean("Flux VM"), geomean("Flux JIT")
    bt = compute["binary_trees"]
    fib = compute["fib"]
    chain = {k: rget(k, "chain") for k in REACT_ORDER}
    todos = {k: rget(k, "todos") for k in REACT_ORDER}
    s_comp = stress["components"]
    page = (HERE / "template.html").read_text()
    subs = {
        "RELEASE_05": release_section(),
        "COMPUTE_TABLE": compute_table(),
        "GEOMEAN_CHART": geomean_chart(),
        "REACTIVE_CHARTS": reactive_charts(),
        "REACTIVE_TABLE": reactive_table(),
        "STARTUP_CHART": startup_chart(),
        "MATRIX": matrix_table(),
        "VERDICT_BAR": vbar,
        "ARBITRARY": arbitrary_list(),
        "ERRORS_TABLE": etable,
        "GM_VM": ratio_text(gm_vm), "GM_JIT": ratio_text(gm_jit),
        "BT_VM": fmt_s(bt["Flux VM"]["wall"]), "BT_PY": fmt_s(bt["Python"]["wall"]),
        "BT_RATIO": f'{bt["Flux VM"]["wall"] / bt["Python"]["wall"]:.0f}',
        "FIB_VM": fmt_s(fib["Flux VM"]["wall"]), "FIB_JIT": fmt_s(fib["Flux JIT"]["wall"]),
        "FIB_GAIN": f'{fib["Flux VM"]["wall"] / fib["Flux JIT"]["wall"]:.0f}',
        "CHAIN_FLUX": fmt_ms(chain["Flux VM"]["update_ms"]), "CHAIN_PLAIN": fmt_ms(chain["Flux VM, no graph"]["update_ms"]),
        "CHAIN_VUE": fmt_ms(chain["Vue reactivity 3.5 (node)"]["update_ms"]), "CHAIN_PREACT": fmt_ms(chain["Preact signals (node)"]["update_ms"]),
        "CHAIN_SOLID": fmt_ms(chain["SolidJS 2 (node)"]["update_ms"]), "CHAIN_MOBX": fmt_ms(chain["MobX 7 (node)"]["update_ms"]),
        "TODOS_FLUX": fmt_ms(todos["Flux VM"]["update_ms"]), "TODOS_PREACT": fmt_ms(todos["Preact signals (node)"]["update_ms"]),
        "TODOS_SVELTE": fmt_ms(todos["Svelte 5 runes (node)"]["update_ms"]),
        "START_FLUX": f'{startup["hello_start_ms"]["Flux VM (run, no cache)"]:.1f}',
        "START_PY": f'{startup["hello_start_ms"]["Python"]:.0f}', "START_NODE": f'{startup["hello_start_ms"]["Node"]:.0f}',
        "CHECK_MS": f'{startup["flux_check_largest_program"]["ms"]:.0f}', "CHECK_LINES": str(startup["flux_check_largest_program"]["lines"]),
        "MUTANTS": f"{mutants:,}".replace(",", " "), "FUZZ_OK": f"{ok:,}".replace(",", " "), "FUZZ_RAN": f"{ran:,}".replace(",", " "),
        "HANGS": str(kinds.get("hang", 0)),
        "STRESS_FIRST": f'{s_comp[0]["rss_mb"]:.1f}', "STRESS_LAST": f'{s_comp[-1]["rss_mb"]:.1f}',
        "V_TOTAL": str(vtotal), "V_NEED": str(vcounts["Necessary"]), "V_RELAX": str(vcounts["Justified but relaxable"]),
        "V_ARB": str(vcounts["Arbitrary"]), "MSG_MEAN": f"{msg_mean:.1f}", "BOOK_MEAN": f"{book_mean:.1f}",
        "ERR_FLUX": str(etotals["fl"]), "ERR_RUST": str(etotals["rs"]), "ERR_ELM": str(etotals["elm"]),
        "ERR_TS": str(etotals["ts"]), "ERR_PY": str(etotals["py"]), "ERR_MAX": str(3 * len(errors)),
    }
    for k, v in subs.items():
        page = page.replace("{{" + k + "}}", v)
    assert "{{" not in page, page[page.index("{{"):page.index("{{") + 40]
    (HERE / "index.html").write_text(page)
    print("wrote", HERE / "index.html", len(page) // 1024, "KB")


if __name__ == "__main__":
    build()
