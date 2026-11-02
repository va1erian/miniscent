//! Why a texture could not be made.

use core::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum TexError {
    /// A side is zero, not a power of two, or above [`crate::image::MAX_SIDE`].
    BadSize,
    /// An operator has a parameter that does not exist.
    UnknownParam(String),
    /// A parameter is outside its range or of the wrong kind.
    BadParam(String),
    /// A pattern's counts cannot tile (say, alternating rows with an odd count).
    NotTiling(&'static str),
    /// An operator received the wrong number of inputs, or inputs of another size.
    BadInputs(&'static str),
}

impl fmt::Display for TexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSize => write!(f, "sides must be powers of two up to 1024"),
            Self::UnknownParam(p) => write!(f, "unknown parameter {p}"),
            Self::BadParam(p) => write!(f, "bad value for parameter {p}"),
            Self::NotTiling(why) => write!(f, "does not tile: {why}"),
            Self::BadInputs(op) => write!(f, "{op}: wrong inputs"),
        }
    }
}

impl std::error::Error for TexError {}
