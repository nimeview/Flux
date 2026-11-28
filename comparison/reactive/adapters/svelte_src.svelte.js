// Svelte 5 runes, compiled with svelte/compiler compileModule by build.mjs.
import { flushSync } from 'svelte';

class Src {
  v = $state.raw(undefined);
  constructor(v) { this.v = v; }
  get() { return this.v; }
  set(v) { this.v = v; }
}

class Comp {
  constructor(fn) { this.fn = fn; }
  v = $derived(this.fn());
  get() { return this.v; }
}

export const svelte = {
  name: 'Svelte 5 runes',
  signal: (v) => new Src(v),
  computed: (fn) => new Comp(fn),
  root: (fn) => {
    let out;
    const dispose = $effect.root(() => { out = fn(); });
    flushSync();
    return { out, dispose };
  },
  effect: (fn) => { $effect(() => { fn(); }); },
  batch: (fn) => { fn(); flushSync(); },
};
