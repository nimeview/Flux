const make = (d) => (d === 0 ? [null, null] : [make(d - 1), make(d - 1)]);
const check = (t) => (t === null ? 0 : 1 + check(t[0]) + check(t[1]));
const maxDepth = 16;
let total = check(make(maxDepth + 1));
const longLived = make(maxDepth);
for (let depth = 4; depth <= maxDepth; depth += 2) {
  const iterations = 1 << (maxDepth - depth + 4);
  for (let i = 0; i < iterations; i++) total += check(make(depth));
}
total += check(longLived);
console.log(total);
