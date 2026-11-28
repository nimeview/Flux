package main

import (
	"fmt"
	"math"
)

func a(i, j int) float64 { return 1.0 / float64((i+j)*(i+j+1)/2+i+1) }

func mulAv(v []float64, n int) []float64 {
	out := make([]float64, 0, n)
	for i := 0; i < n; i++ {
		s := 0.0
		for j := 0; j < n; j++ {
			s += a(i, j) * v[j]
		}
		out = append(out, s)
	}
	return out
}

func mulAtv(v []float64, n int) []float64 {
	out := make([]float64, 0, n)
	for i := 0; i < n; i++ {
		s := 0.0
		for j := 0; j < n; j++ {
			s += a(j, i) * v[j]
		}
		out = append(out, s)
	}
	return out
}

func main() {
	n := 250
	u := make([]float64, n)
	for i := range u {
		u[i] = 1
	}
	var v []float64
	for k := 0; k < 10; k++ {
		v = mulAtv(mulAv(u, n), n)
		u = mulAtv(mulAv(v, n), n)
	}
	vbv, vv := 0.0, 0.0
	for j := 0; j < n; j++ {
		vbv += u[j] * v[j]
		vv += v[j] * v[j]
	}
	fmt.Println(int64(math.Sqrt(vbv/vv) * 1e9))
}
