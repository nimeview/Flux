// The reactive scenarios for the JS libraries; same sizes and checksums as
// gen_flux.py. Usage: node --conditions=browser bench.mjs <lib-index> <scenario>
// Prints JSON: checksum, update ms (and create ms), computations, effect runs.
import { libs } from './adapters/libs.mjs';

const N = 1000, U = 1000, T = 100;
const L = libs[Number(process.argv[2])];
const scenario = process.argv[3];
let computations = 0, effects = 0, seen = 0;
const comp = (fn) => L.computed(() => { computations++; return fn(); });
const watch = (read) => L.effect(() => { effects++; seen += read(); });

const scenarios = {
  chain() {
    const { out: s } = L.root(() => {
      const s = L.signal(0);
      let prev = comp(() => s.get() + 1);
      for (let i = 1; i < N; i++) { const p = prev; prev = comp(() => p.get() + 1); }
      const last = prev;
      watch(() => last.get());
      return s;
    });
    const t0 = performance.now();
    for (let i = 0; i < U; i++) L.batch(() => s.set(i + 1));
    return { update_ms: performance.now() - t0 };
  },
  fan() {
    const { out: s } = L.root(() => {
      const s = L.signal(0);
      const a = [];
      for (let i = 0; i < N; i++) a.push(comp(() => s.get() + i));
      const groups = [];
      for (let g = 0; g < N; g += 50) {
        const part = a.slice(g, g + 50);
        groups.push(comp(() => { let t = 0; for (const x of part) t += x.get(); return t; }));
      }
      const total = comp(() => { let t = 0; for (const g of groups) t += g.get(); return t; });
      watch(() => total.get());
      return s;
    });
    const t0 = performance.now();
    for (let i = 0; i < U; i++) L.batch(() => s.set(i + 1));
    return { update_ms: performance.now() - t0 };
  },
  counters() {
    let all;
    const { out: make } = L.root(() => {
      all = L.signal([]);
      const total = comp(() => all.get().map((c) => c.double.get()).reduce((x, y) => x + y, 0));
      watch(() => total.get());
      return (n) => {
        const xs = [...all.get()];
        for (let i = 0; i < n; i++) {
          const c = { n: L.signal(i) };
          c.double = comp(() => c.n.get() * 2);
          xs.push(c);
        }
        all.set(xs);
      };
    });
    const t0 = performance.now();
    L.batch(() => make(N));
    const t1 = performance.now();
    for (let k = 0; k < T; k++) L.batch(() => { for (const c of all.get()) c.n.set(c.n.get() + 1); });
    return { create_ms: t1 - t0, update_ms: performance.now() - t1 };
  },
  todos() {
    let todos;
    L.root(() => {
      todos = L.signal([]);
      const remaining = comp(() => todos.get().filter((t) => !t.done).length);
      watch(() => remaining.get());
    });
    const t0 = performance.now();
    for (let i = 0; i < N; i++) L.batch(() => todos.set([...todos.get(), { id: i, done: false }]));
    for (let i = 0; i < N; i += 2) L.batch(() => todos.set(todos.get().map((t) => (t.id === i ? { ...t, done: !t.done } : t))));
    for (let i = 0; i < N / 2; i++) L.batch(() => todos.set(todos.get().filter((t) => t.id !== i)));
    return { update_ms: performance.now() - t0 };
  },
};

globalThis.gc?.();
const heap0 = process.memoryUsage().heapUsed;
const r = scenarios[scenario]();
const heap = process.memoryUsage().heapUsed - heap0;
console.log(JSON.stringify({ lib: L.name, scenario, checksum: seen, ...r, computations, effects, heap_mb: heap / 1e6 }));
