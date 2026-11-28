const parts = [];
for (let i = 0; i < 200000; i++) parts.push(`item${i % 1000}`);
const words = parts.join(",").split(",");
let total = 0;
for (const w of words) if (w.endsWith("7")) total += w.length;
console.log(words.length, total);
