use std::io;
use std::array;
use std::error;
use std::fmt;

macro_rules! stingray_error {
    () => (crate::StingrayError::with_location("", file!(), line!()));
    ($($arg:expr),+) => (crate::StingrayError::with_location(&format!($($arg,)+), file!(), line!()));
}

pub type StingrayResult<T> = std::result::Result<T, StingrayError>;

#[derive(Debug)]
pub enum StingrayError {
    Io(io::Error),
    Array(array::TryFromSliceError),

    Stingray {
        error: String,
    }
}

impl StingrayError {
    pub fn new(msg: &str) -> Self {
        StingrayError::Stingray {
            error: msg.to_string(),
        }
    }

    pub fn with_location(msg: &str, file: &str, line: u32) -> Self {
        StingrayError::Stingray {
            error: format!("{} at {}:{}", msg, file, line),
        }
    }
}

impl fmt::Display for StingrayError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            StingrayError::Io(ref err) => err.fmt(f),
            StingrayError::Array(ref err) => err.fmt(f),
            StingrayError::Stingray { ref error } => error.fmt(f),
        }
    }
}

impl error::Error for StingrayError {
    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            StingrayError::Io(ref err) => Some(err),
            StingrayError::Array(ref err) => Some(err),
            StingrayError::Stingray {..} => Some(self),
        }
    }
}

impl From<io::Error> for StingrayError {
    fn from(err: io::Error) -> StingrayError {
        StingrayError::Io(err)
    }
}

impl From<array::TryFromSliceError> for StingrayError {
    fn from(err: array::TryFromSliceError) -> StingrayError {
        StingrayError::Array(err)
    }
}








