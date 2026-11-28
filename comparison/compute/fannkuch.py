def fannkuch(n):
    perm1 = list(range(n))
    count = [0] * n
    max_flips = checksum = 0
    sign = 1
    r = n
    while True:
        while r != 1:
            count[r - 1] = r
            r -= 1
        perm = perm1[:]
        flips = 0
        first = perm[0]
        while first:
            perm[:first + 1] = perm[first::-1]
            flips += 1
            first = perm[0]
        max_flips = max(max_flips, flips)
        checksum += sign * flips
        sign = -sign
        while True:
            if r == n:
                return checksum, max_flips
            p0 = perm1[0]
            perm1[:r] = perm1[1:r + 1]
            perm1[r] = p0
            count[r] -= 1
            if count[r] > 0:
                break
            r += 1

print(*fannkuch(9))
