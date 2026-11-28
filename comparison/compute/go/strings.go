package main

import (
	"fmt"
	"strings"
)

func main() {
	parts := make([]string, 0, 200000)
	for i := 0; i < 200000; i++ {
		parts = append(parts, fmt.Sprintf("item%d", i%1000))
	}
	words := strings.Split(strings.Join(parts, ","), ",")
	total := 0
	for _, w := range words {
		if strings.HasSuffix(w, "7") {
			total += len(w)
		}
	}
	fmt.Println(len(words), total)
}
