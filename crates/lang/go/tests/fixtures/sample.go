package garden

import (
	"fmt"
	"io"
)

const Max = 10

const (
	Small = iota
	Medium
	Large
)

var (
	count int
	name, title = "a", "b"
	handler = func() {
		fmt.Println("inline")
	}
)

var Default *Plant

// Plant is something that grows.
type Plant struct {
	Name, Kind string `json:"name"`
	height     float64
	io.Reader
	*Soil
	Leaves map[string]struct{ Size int }
}

type Grower interface {
	Grow(days int) error
	io.Closer
	Height() float64
}

type (
	ID    int
	Alias = string
	List[T any] []T
)

type Grid [5]int

func (p *Plant) Grow(days int) error {
	if days < 0 {
		return fmt.Errorf("negative: %d", days)
	}
	return nil
}

func New(name string) *Plant {
	return &Plant{Name: name}
}

func (l List[T]) Len() int { return len(l) }

func (s *Soil) Wet() bool { return true }

func (p Plant) Height() float64 {
	return p.height
}
