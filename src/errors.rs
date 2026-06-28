use std::error::Error;
use std::fmt::{Display, Formatter};

// TODO: collect error types here and flesh out implementation.
//  -> File Ops/File Writes
//  -> Data stream too large for QR.
//  -> Encoding errors.
//  -> etc.
//  -> collect what's been left around in modules and see what actually makes sense.
#[derive(Clone, Debug)]
pub enum QrError {}

pub type Result<T> = Result<T, QrError>;

impl Error for QrError {}

impl From<std::io::Error> for QrError {
    fn from(_io_err: std::io::Error) -> Self {
        todo!("Implement From<std::io::Error>");
    }
}

impl Display for QrError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        todo!("Implement Display for QrError");
    }
}
