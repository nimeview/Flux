function fannkuch(n) {
  const perm1 = [...Array(n).keys()], count = new Array(n).fill(0);
  let maxFlips = 0, checksum = 0, sign = 1, r = n;
  for (;;) {
    while (r !== 1) { count[r - 1] = r; r--; }
    const perm = perm1.slice();
    let flips = 0, first = perm[0];
    while (first !== 0) {
      for (let lo = 0, hi = first; lo < hi; lo++, hi--) { const t = perm[lo]; perm[lo] = perm[hi]; perm[hi] = t; }
      flips++; first = perm[0];
    }
    if (flips > maxFlips) maxFlips = flips;
    checksum += sign * flips; sign = -sign;
    for (;;) {
      if (r === n) return [checksum, maxFlips];
      const p0 = perm1[0];
      for (let m = 0; m < r; m++) perm1[m] = perm1[m + 1];
      perm1[r] = p0;
      if (--count[r] > 0) break;
      r++;
    }
  }
}
console.log(...fannkuch(9));
