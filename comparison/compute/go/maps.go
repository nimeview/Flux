package main

import "fmt"

func main() {
	counts := map[string]int{}
	for i := 0; i < 200000; i++ {
		counts[fmt.Sprintf("k%d", i%5000)] += i % 7
	}
	total := 0
	for _, v := range counts {
		total += v
	}
	fmt.Println(len(counts), total)
}
