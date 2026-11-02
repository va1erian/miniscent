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
    /// A recipe file could not be read. Line 0 is the file as a whole.
    Parse { line: usize, why: String },
    /// A page's blocks do not connect.
    Graph(String),
    /// A page would hold more images at once than a run may.
    TooLarge,
    /// The caller stopped the run.
    Cancelled,
}

impl fmt::Display for TexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSize => write!(f, "sides must be powers of two up to 1024"),
            Self::UnknownParam(p) => write!(f, "unknown parameter {p}"),
            Self::BadParam(p) => write!(f, "bad value for parameter {p}"),
            Self::NotTiling(why) => write!(f, "does not tile: {why}"),
            Self::BadInputs(op) => write!(f, "{op}: wrong inputs"),
            Self::Parse { line, why } => write!(f, "line {line}: {why}"),
            Self::Graph(why) => write!(f, "{why}"),
            Self::TooLarge => write!(f, "the page holds too many images at once"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for TexError {}
