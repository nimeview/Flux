// node elm/run.mjs <scenario>: runs the compiled Elm worker, prints JSON.
import { readFileSync } from 'node:fs';
const code = readFileSync(new URL('main.js', import.meta.url), 'utf8');
const scope = {};
new Function('scope', code.replace('(this)', '(scope)'))(scope);
const t0 = performance.now();
const app = scope.Elm.Main.init({ flags: process.argv[2] });
app.ports.done.subscribe((seen) => {
  console.log(JSON.stringify({ lib: 'Elm 0.19 (recompute all)', scenario: process.argv[2], checksum: seen, update_ms: performance.now() - t0 }));
});
