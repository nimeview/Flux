const counts = new Map();
for (let i = 0; i < 200000; i++) { const k = `k${i % 5000}`; counts.set(k, (counts.get(k) ?? 0) + (i % 7)); }
let total = 0;
for (const v of counts.values()) total += v;
console.log(counts.size, total);
