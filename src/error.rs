use std::convert::From;
use std::error::Error;
use std::fmt;
use std::io::Error as IoError;

use std::io;

/* All of this is just to make sure we can make the error handling
 * code more brief by using the ? operator that auto-converts the
 * errors to a common type represented below by the enum. The
 * actual conversion bits are below:  From<src type>.
 */

#[derive(Debug)]
pub enum DecodeError {
    IO(io::Error),
    TooFewBytes,
    Other(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            DecodeError::IO(ref cause) => write!(f, "I/O Error {}", cause),
            DecodeError::TooFewBytes => write!(f, "too few bytes"),
            DecodeError::Other(ref cause) => write!(f, "Other error {}", cause),
        }
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            DecodeError::IO(ref source) => Some(source),
            DecodeError::TooFewBytes => None,
            DecodeError::Other(_) => None,
        }
    }
}

impl From<IoError> for DecodeError {
    fn from(cause: IoError) -> DecodeError {
        DecodeError::IO(cause)
    }
}
