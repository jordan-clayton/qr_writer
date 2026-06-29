use crate::ECCLevel;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub enum TextEncoding {
    ShiftJIS,
    ISO88591,
}

#[derive(Copy, Clone, Debug)]
pub enum ArithmeticError {
    ZeroDivision,
    ZeroLog,
    MemberNotInField { member: usize, field_size: usize },
    EmptyPolynomial,
    InvalidDegree,
}

// TODO: collect error types here and flesh out implementation.
//  -> File Ops/File Writes
//  -> Data stream too large for QR.
//  -> Encoding errors.
//  -> etc.
//  -> collect what's been left around in modules and see what actually makes sense.
#[derive(Debug)]
pub enum QrError {
    InvalidMode(u8),
    InvalidGroup(usize),
    // At the moment, this API doesn't support optimizing/multi-qr.
    InvalidVersion,
    InvalidCorners,
    InvalidMask(usize),
    VersionResolution {
        data_len: usize,
        ecc_level: ECCLevel,
    },
    UtfEncodeError(TextEncoding),
    DataEncodeError {
        reason: String,
    },
    // TODO: If this doesn't need to be a string, make it a static slice.
    WriteError {
        reason: String,
    },
    // Swap to a string if format strings are required.
    SampleError {
        reason: String,
    },
    // Errors during rendering the matrix -> I'm not quite sure about this just yet.
    RenderError {
        reason: String,
    },
    ArithmeticError(ArithmeticError),
    #[cfg(feature = "image")]
    // TODO: from Impl for coercing errors from image crate
    // Just wrap the error.
    ImageError(image::ImageError),
    #[cfg(feature = "svg")]
    SvgWriteError(std::io::Error),
}

pub type Result<T> = std::result::Result<T, QrError>;

impl Error for QrError {}

// Svg just uses std::io for its file-write operations.
// This api doesn't do any other I/O, so this should be unambiguous.
#[cfg(feature = "svg")]
impl From<std::io::Error> for QrError {
    fn from(e: std::io::Error) -> Self {
        Self::SvgWriteError(e)
    }
}

#[cfg(feature = "png")]
impl From<image::ImageError> for QrError {
    fn from(p: image::ImageError) -> Self {
        Self::ImageError(p)
    }
}

impl Display for QrError {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Self::VersionResolution {
                data_len,
                ecc_level,
            } => write!(
                f,
                "Data stream is too large for maximum QR version.\nBytes: {data_len}, ECC level: {:?}",
                ecc_level
            ),
            _ => todo!("Implement display for rest of error enum."),
        }
    }
}
