package main

import "fmt"

func fannkuch(n int) (int, int) {
	perm1 := make([]int, n)
	count := make([]int, n)
	perm := make([]int, n)
	for i := range perm1 {
		perm1[i] = i
	}
	maxFlips, checksum, sign, r := 0, 0, 1, n
	for {
		for r != 1 {
			count[r-1] = r
			r--
		}
		copy(perm, perm1)
		flips, first := 0, perm[0]
		for first != 0 {
			for lo, hi := 0, first; lo < hi; lo, hi = lo+1, hi-1 {
				perm[lo], perm[hi] = perm[hi], perm[lo]
			}
			flips++
			first = perm[0]
		}
		if flips > maxFlips {
			maxFlips = flips
		}
		checksum += sign * flips
		sign = -sign
		for {
			if r == n {
				return checksum, maxFlips
			}
			p0 := perm1[0]
			for m := 0; m < r; m++ {
				perm1[m] = perm1[m+1]
			}
			perm1[r] = p0
			count[r]--
			if count[r] > 0 {
				break
			}
			r++
		}
	}
}

func main() {
	c, f := fannkuch(9)
	fmt.Println(c, f)
}
