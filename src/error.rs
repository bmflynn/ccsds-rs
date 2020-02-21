use std::convert::From;
use std::error::Error;
use std::fmt;
use std::io::Error as IoError;

use packed_struct::PackingError;

use std::io;

/* All of this is just to make sure we can make the error handling
 * code more brief by using the ? operator that auto-converts the
 * errors to a common type represented below by the enum. The
 * actual conversion bits are below:  From<src type>.
 */


#[derive(Debug)]
pub enum DecodeError {
    Io(io::Error),
    Packing(PackingError),
    Convert(std::convert::Infallible),
    Other(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            DecodeError::Io(ref cause) => write!(f, "I/O Error {}", cause),
            DecodeError::Packing(ref cause) => write!(f, "packing error {}", cause),
            DecodeError::Convert(ref cause) => write!(f, "conversion error {}", cause),
            DecodeError::Other(ref cause) => write!(f, "Other error {}", cause),
        }
    }
}

impl Error for DecodeError {
    fn description(&self) -> &str {
        match *self {
            DecodeError::Io(ref cause) => cause.description(),
            DecodeError::Packing(ref cause) => cause.description(),
            DecodeError::Convert(ref cause) => cause.description(),
            DecodeError::Other(ref s) => s,
        }
    }

    fn cause(&self) -> Option<&Error> {
        match *self {
            DecodeError::Io(ref cause) => Some(cause),
            DecodeError::Packing(ref cause) => Some(cause),
            DecodeError::Convert(ref cause) => Some(cause),
            DecodeError::Other(_) => None,
        }
    }
}

impl From<IoError> for DecodeError {
    fn from(cause: IoError) -> DecodeError {
        DecodeError::Io(cause)
    }
}

impl From<PackingError> for DecodeError {
    fn from(cause: PackingError) -> DecodeError {
        DecodeError::Packing(cause)
    }
}

impl From<std::convert::Infallible> for DecodeError {
    fn from(cause: std::convert::Infallible) -> DecodeError {
        DecodeError::Convert(cause)
    }
}
