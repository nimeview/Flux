type Mode = "Heat" | "Cool" | "Idle";
const BAND = 1;

const label = (m: Mode) => ({ Heat: "heating", Cool: "cooling", Idle: "idle" })[m];

const state = { temp: 20, target: 20 };
const mode = (): Mode =>
  state.temp < state.target - BAND ? "Heat" : state.temp > state.target + BAND ? "Cool" : "Idle";
const display = () => `now ${state.temp}C, target ${state.target}C, ${label(mode())}`;

let lastDisplay = "";
let lastMode: Mode | null = null;
function settle() {
  const d = display();
  if (d !== lastDisplay) console.log(`[display] ${d}`);
  lastDisplay = d;
  const m = mode();
  if (lastMode !== null && m !== lastMode) console.log(`[mode] ${label(lastMode)} -> ${label(m)}`);
  lastMode = m;
}

type Ev = { kind: "Reading"; c: number } | { kind: "SetTarget"; c: number } | { kind: "Adjust"; d: number };
function emit(e: Ev) {
  if (e.kind === "Reading") state.temp = e.c;
  else if (e.kind === "SetTarget") state.target = e.c;
  else state.target += e.d;
  settle();
}

settle();
emit({ kind: "Reading", c: 17 });
emit({ kind: "Reading", c: 18 });
emit({ kind: "SetTarget", c: 18 });
emit({ kind: "Reading", c: 23 });
emit({ kind: "Adjust", d: 3 });
emit({ kind: "Adjust", d: 3 });
emit({ kind: "Reading", c: 25 });
emit({ kind: "Reading", c: 25 });
