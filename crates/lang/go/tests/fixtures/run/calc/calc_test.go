package calc

import (
	"fmt"
	"testing"
)

func TestAdd(t *testing.T) {
	if Add(1, 2) != 3 {
		t.Fatal("bad")
	}
}

func TestTable(t *testing.T) {
	for _, name := range []string{"one", "two words"} {
		t.Run(name, func(t *testing.T) {
			_ = name
		})
	}
}

func helper(t *testing.T) {}

func BenchmarkAdd(b *testing.B) {
	for i := 0; i < b.N; i++ {
		Add(i, i)
	}
}

func FuzzAdd(f *testing.F) {
	f.Fuzz(func(t *testing.T, a, b int) { Add(a, b) })
}

func ExampleAdd() {
	fmt.Println(Add(1, 1))
	// Output: 2
}
