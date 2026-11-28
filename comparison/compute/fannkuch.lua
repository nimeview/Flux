local function fannkuch(n)
  local perm1, count, perm = {}, {}, {}
  for i = 0, n - 1 do perm1[i] = i; count[i] = 0 end
  local maxFlips, checksum, sign, r = 0, 0, 1, n
  while true do
    while r ~= 1 do count[r - 1] = r; r = r - 1 end
    for k = 0, n - 1 do perm[k] = perm1[k] end
    local flips, first = 0, perm[0]
    while first ~= 0 do
      local lo, hi = 0, first
      while lo < hi do perm[lo], perm[hi] = perm[hi], perm[lo]; lo = lo + 1; hi = hi - 1 end
      flips = flips + 1; first = perm[0]
    end
    if flips > maxFlips then maxFlips = flips end
    checksum = checksum + sign * flips; sign = -sign
    while true do
      if r == n then return checksum, maxFlips end
      local p0 = perm1[0]
      for m = 0, r - 1 do perm1[m] = perm1[m + 1] end
      perm1[r] = p0
      count[r] = count[r] - 1
      if count[r] > 0 then break end
      r = r + 1
    end
  end
end
print(fannkuch(9))
