//! The part of the language written in itself. It loads first, as the source
//! named `<prelude>`, so every other source can use it and none can redefine
//! it.

pub(crate) const NAME: &str = "<prelude>";

pub(crate) const SOURCE: &str = "
define arr do: blk [ i := 1. [i <= arr size] whileTrue: [blk value: (arr at: i). i := i + 1] ]
define n timesRepeat: blk [ i := 0. [i < n] whileTrue: [blk value. i := i + 1] ]
define waitUntil: cond [ [cond value] whileFalse: [wait: 0.1] ]
define every: secs do: blk [ fork: [ [true] whileTrue: [wait: secs. blk value] ] ]
define after: secs do: blk [ fork: [ wait: secs. blk value ] ]
";
