module example.com/broken

go banana
frobnicate now
require example.com/missing
require (
	example.com/ok v1.0.0
	example.com/bad 1.0.0
)
replace example.com/x => example.com/y
replace example.com/z v1.0.0
retract [v1.0.0 v1.1.0]
godebug nokey
module example.com/again
require "example.com/open v1.0.0
require (
	example.com/last v1.0.0
