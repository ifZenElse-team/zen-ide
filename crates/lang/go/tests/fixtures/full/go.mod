// Deprecated: use example.com/garden/v2 instead.
// It will not get fixes.
module example.com/garden

go 1.23.0

toolchain go1.23.4

require example.com/single v1.0.0

require (
	example.com/alpha v1.2.3
	example.com/beta v0.4.0 // indirect
	"example.com/quoted" v2.0.0+incompatible
	`example.com/raw` v0.0.0-20240101000000-abcdefabcdef // indirect; for tests
)

replace example.com/alpha => ../alpha

replace (
	example.com/beta v0.4.0 => example.com/beta-fork v0.4.1
	example.com/gamma => "./third party/gamma"
)

exclude example.com/alpha v1.2.0

exclude (
	example.com/beta v0.3.9
)

// The first release had a broken API.
retract v0.1.0

retract (
	[v0.2.0, v0.2.5] // Published too early.
	v0.3.0
)

godebug default=go1.21

godebug (
	panicnil=1
	asynctimerchan=0
)

tool example.com/tools/cmd/gen

tool (
	example.com/tools/cmd/lint
)

ignore ./node_modules

ignore (
	./build
	"./my docs"
)
