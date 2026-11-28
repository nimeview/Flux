// One small adapter per library: signal, computed, effect, a root that owns
// effects, and batch(fn) = apply all writes in fn, then run effects once.
import * as solid from 'solid-js';
import * as vue from '@vue/reactivity';
import * as mobx from 'mobx';
import * as preact from '@preact/signals-core';
import { svelte } from './svelte.compiled.js';

const solidA = {
  name: 'SolidJS 2',
  signal: (v) => { const [g, s] = solid.createSignal(v); return { get: g, set: (x) => s(() => x) }; },
  computed: (fn) => { const g = solid.createMemo(fn); return { get: g }; },
  root: (fn) => { let out; const dispose = solid.createRoot((d) => { out = fn(); return d; }); solid.flush(); return { out, dispose }; },
  effect: (fn) => { solid.createEffect(fn, () => {}); },
  batch: (fn) => { fn(); solid.flush(); },
};

// Vue's own runtime queues effects through a scheduler and flushes them;
// do the same here.
const vueQueue = new Set();
const vueA = {
  name: 'Vue reactivity 3.5',
  signal: (v) => { const r = vue.shallowRef(v); return { get: () => r.value, set: (x) => { r.value = x; } }; },
  computed: (fn) => { const c = vue.computed(fn); return { get: () => c.value }; },
  root: (fn) => { const scope = vue.effectScope(); const out = scope.run(fn); return { out, dispose: () => scope.stop() }; },
  effect: (fn) => { const runner = vue.effect(fn, { scheduler: () => vueQueue.add(runner) }); },
  batch: (fn) => { fn(); const jobs = [...vueQueue]; vueQueue.clear(); for (const j of jobs) if (j.effect.dirty) j(); },
};

mobx.configure({ enforceActions: 'never' });
const mobxA = {
  name: 'MobX 7',
  signal: (v) => { const b = mobx.observable.box(v, { deep: false }); return { get: () => b.get(), set: (x) => b.set(x) }; },
  computed: (fn) => { const c = mobx.computed(fn); return { get: () => c.get() }; },
  root: (fn) => { const disposers = []; mobxA._d = disposers; const out = fn(); return { out, dispose: () => disposers.forEach((d) => d()) }; },
  effect: (fn) => { mobxA._d.push(mobx.autorun(fn)); },
  batch: (fn) => mobx.runInAction(fn),
};

const preactA = {
  name: 'Preact signals',
  signal: (v) => { const s = preact.signal(v); return { get: () => s.value, set: (x) => { s.value = x; } }; },
  computed: (fn) => { const c = preact.computed(fn); return { get: () => c.value }; },
  root: (fn) => { const disposers = []; preactA._d = disposers; const out = fn(); return { out, dispose: () => disposers.forEach((d) => d()) }; },
  effect: (fn) => { preactA._d.push(preact.effect(fn)); },
  batch: (fn) => preact.batch(fn),
};

export const libs = [solidA, vueA, mobxA, preactA, svelte];
