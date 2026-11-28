package main

import (
	"fmt"
	"math"
)

type Body struct{ x, y, z, vx, vy, vz, mass float64 }

const pi = 3.141592653589793
const solar = 4 * pi * pi
const days = 365.24

func body(x, y, z, vx, vy, vz, mass float64) Body {
	return Body{x, y, z, vx * days, vy * days, vz * days, mass * solar}
}

func energy(bs []Body) float64 {
	e := 0.0
	for i := range bs {
		b := bs[i]
		e += 0.5 * b.mass * (b.vx*b.vx + b.vy*b.vy + b.vz*b.vz)
		for j := i + 1; j < len(bs); j++ {
			c := bs[j]
			dx, dy, dz := b.x-c.x, b.y-c.y, b.z-c.z
			e -= b.mass * c.mass / math.Sqrt(dx*dx+dy*dy+dz*dz)
		}
	}
	return e
}

func advance(bs []Body, dt float64) {
	for i := range bs {
		bi := &bs[i]
		for j := i + 1; j < len(bs); j++ {
			bj := &bs[j]
			dx, dy, dz := bi.x-bj.x, bi.y-bj.y, bi.z-bj.z
			d2 := dx*dx + dy*dy + dz*dz
			mag := dt / (d2 * math.Sqrt(d2))
			bi.vx -= dx * bj.mass * mag
			bi.vy -= dy * bj.mass * mag
			bi.vz -= dz * bj.mass * mag
			bj.vx += dx * bi.mass * mag
			bj.vy += dy * bi.mass * mag
			bj.vz += dz * bi.mass * mag
		}
	}
	for i := range bs {
		b := &bs[i]
		b.x += dt * b.vx
		b.y += dt * b.vy
		b.z += dt * b.vz
	}
}

func main() {
	bs := []Body{
		{0, 0, 0, 0, 0, 0, solar},
		body(4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694, 0.00769901118419740425, -0.0000690460016972063023, 0.000954791938424326609),
		body(8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411, 0.00499852801234917238, 0.0000230417297573763929, 0.000285885980666130812),
		body(12.894369562139131, -15.111151401698631, -0.22330757889265573, 0.0029646018915851645, 0.0023784717395948095, -0.000029658956854023756, 0.000043662440433515637),
		body(15.379697114850917, -25.919314609987964, 0.17925877295037118, 0.0026806777249038932, 0.001628241700382423, -0.000095159225451971587, 0.000051513890204661145),
	}
	px, py, pz := 0.0, 0.0, 0.0
	for _, b := range bs {
		px += b.vx * b.mass
		py += b.vy * b.mass
		pz += b.vz * b.mass
	}
	bs[0].vx, bs[0].vy, bs[0].vz = -px/solar, -py/solar, -pz/solar
	before := energy(bs)
	for s := 0; s < 100000; s++ {
		advance(bs, 0.01)
	}
	fmt.Println(int64(before*1e9), int64(energy(bs)*1e9))
}
