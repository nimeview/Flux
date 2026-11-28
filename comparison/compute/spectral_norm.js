const a = (i, j) => 1.0 / (((i + j) * (i + j + 1)) / 2 + i + 1);
function mulAv(v, n) { const out = []; for (let i = 0; i < n; i++) { let s = 0; for (let j = 0; j < n; j++) s += a(i, j) * v[j]; out.push(s); } return out; }
function mulAtv(v, n) { const out = []; for (let i = 0; i < n; i++) { let s = 0; for (let j = 0; j < n; j++) s += a(j, i) * v[j]; out.push(s); } return out; }
const n = 250;
let u = new Array(n).fill(1.0), v = [];
for (let k = 0; k < 10; k++) { v = mulAtv(mulAv(u, n), n); u = mulAtv(mulAv(v, n), n); }
let vbv = 0, vv = 0;
for (let j = 0; j < n; j++) { vbv += u[j] * v[j]; vv += v[j] * v[j]; }
console.log(Math.trunc(Math.sqrt(vbv / vv) * 1e9));
