package main

import "fmt"

type Node struct{ l, r *Node }

func make_(d int) *Node {
	if d == 0 {
		return &Node{}
	}
	return &Node{make_(d - 1), make_(d - 1)}
}

func check(t *Node) int {
	if t == nil {
		return 0
	}
	return 1 + check(t.l) + check(t.r)
}

func main() {
	maxDepth := 16
	total := check(make_(maxDepth + 1))
	longLived := make_(maxDepth)
	for depth := 4; depth <= maxDepth; depth += 2 {
		iterations := 1 << (maxDepth - depth + 4)
		for i := 0; i < iterations; i++ {
			total += check(make_(depth))
		}
	}
	total += check(longLived)
	fmt.Println(total)
}
