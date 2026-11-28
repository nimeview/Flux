def fannkuch(n)
  perm1 = (0...n).to_a; count = [0] * n
  max_flips = checksum = 0; sign = 1; r = n
  loop do
    while r != 1; count[r - 1] = r; r -= 1; end
    perm = perm1.dup
    flips = 0; first = perm[0]
    while first != 0
      perm[0..first] = perm[0..first].reverse
      flips += 1; first = perm[0]
    end
    max_flips = flips if flips > max_flips
    checksum += sign * flips; sign = -sign
    loop do
      return [checksum, max_flips] if r == n
      p0 = perm1[0]
      r.times { |m| perm1[m] = perm1[m + 1] }
      perm1[r] = p0
      count[r] -= 1
      break if count[r] > 0
      r += 1
    end
  end
end
puts fannkuch(9).join(" ")
