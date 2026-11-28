// Compiles the Svelte adapter to plain JS once.
import { compileModule } from 'svelte/compiler';
import { readFileSync, writeFileSync } from 'node:fs';
const here = new URL('.', import.meta.url);
const src = readFileSync(new URL('svelte_src.svelte.js', here), 'utf8');
const out = compileModule(src, { generate: 'client', filename: 'svelte_src.svelte.js' });
writeFileSync(new URL('svelte.compiled.js', here), out.js.code);
console.log('built adapters/svelte.compiled.js');
