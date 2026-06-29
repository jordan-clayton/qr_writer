use crate::tables::*;
#[cfg(feature = "kanji")]
use encoding_rs::*;
use regex::Regex;
use std::num::NonZero;
use std::sync::OnceLock;

use crate::errors::{QrError, Result};

// TODO: remove this once ecc moved here.
use crate::ecc::ECCLevel;

#[derive(Default, Copy, Clone, Debug)]
pub struct EncodingHints {
    // version is 1-counted for -most- of everyting in this library
    // The index conversion (0-indexing) is handled internally.
    pub version: Option<NonZero<u8>>,
    pub mask: Option<u8>,
    pub ecc_level: Option<ECCLevel>,
}

// TODO: bring versioning over here to reduce modules.
// TODO: bring ecc over here to reduce number of modules.

// TODO: refactor this into an enumeration
pub const ENC_NUMERIC: u8 = 1;
pub const ENC_ALPHA: u8 = 2;
pub const ENC_BYTES: u8 = 4;
pub const ENC_KANJI: u8 = 8;

static NUMERIC: OnceLock<Regex> = OnceLock::new();
static ALPHANUMERIC: OnceLock<Regex> = OnceLock::new();
static LATIN1: OnceLock<Regex> = OnceLock::new();
#[cfg(feature = "kanji")]
static KANJI: OnceLock<Regex> = OnceLock::new();

// https://en.wikipedia.org/wiki/QR_code
//
// NOTE: Other encoding modes; implement when/where necessary
// 0000 -> Terminator (finishes the stream)
// 0101 -> FNC1 in 1st position (barcode)
// 1001 -> FNC1 in 2nd position (barcode)
// 0011 -> structured append (>1 QR symbol)

// NOTE: these can be mixed:
// [ MODE ] [ Bitstream ] -> [MODE] [Bitstream] -> ... -> [0000 (Terminator)]
// Implement mixing when necessary
pub(crate) fn get_data_encoding_mode(data: &str) -> u8 {
    #[cfg(feature = "kanji")]
    {
        if NUMERIC.get_or_init(init_numeric).is_match(data) {
            ENC_NUMERIC
        } else if ALPHANUMERIC.get_or_init(init_alphanumeric).is_match(data) {
            ENC_ALPHA
        } else if LATIN1.get_or_init(init_latin1).is_match(data) {
            ENC_BYTES
        } else if KANJI.get_or_init(init_kanji).is_match(data) {
            // Test whether Shift_JIS charset and double-byte kanji.
            // Otherwise return BYTE/Unicode.

            if is_double_byte_kanji(data) {
                ENC_KANJI
            } else {
                ENC_BYTES
            }
        } else {
            // ECI: Extended channel iterpretation
            // TODO: Deal with named constants/enums later.
            // This gets folded into byte mode anyway.
            0b0111
        }
    }

    #[cfg(not(feature = "kanji"))]
    if NUMERIC.get_or_init(init_numeric).is_match(data) {
        ENC_NUMERIC
    } else if ALPHANUMERIC.get_or_init(init_alphanumeric).is_match(data) {
        ENC_ALPHA
    } else if LATIN1.get_or_init(init_latin1).is_match(data) {
        ENC_BYTES
    } else {
        // ECI: Extended channel iterpretation
        // TODO: Deal with named constants/enums later.
        0b0111
    }
}

// NUM BITS IN LENGTH FIELD (character counter)
// --------------------------------------------------------
// | ENCODING   (mode) | VER: 1-9 | VER: 10-26 | VER 27-40|
// --------------------------------------------------------
// | NUMERIC    (0001) | 10       | 12         | 14       |
// --------------------------------------------------------
// | ALPHANUM   (0010) | 9        | 11         | 13       |
// --------------------------------------------------------
// | BYTE/LAT   (0100) | 8        | 16         | 16       |
// --------------------------------------------------------
// | KANJI      (1000) | 8        | 10         | 12       |
// --------------------------------------------------------
// ECI gets folded into BYTE/Latin

// TODO: this could probably be a method on mode if mode becomes an enumeration.
pub(crate) fn get_mode_idx(mode: u8) -> usize {
    // This is equivalent to floor(log2(mode)) and folds ECI to nearest power of 2 (4)
    (7 - mode.leading_zeros()) as usize
}

pub(crate) fn get_bit_length(mode: u8, version: u8) -> Result<u8> {
    let ver_idx = match version {
        1..=9 => 0,
        10..=26 => 1,
        27..=40 => 2,
        _ => return Err(QrError::InvalidVersion),
    } as usize;

    let mode_idx = get_mode_idx(mode);
    if mode_idx > 3 {
        Err(QrError::InvalidMode(mode))
    } else {
        Ok(BIT_LENGTH[mode_idx * 3 + ver_idx])
    }
}

// TODO: this needs testing.
#[cfg(feature = "kanji")]
fn is_double_byte_kanji(data: &str) -> bool {
    let (cow, encoding, errors) = SHIFT_JIS.encode(data);

    if errors {
        return false;
    }

    // I'm not sure whether encoding would be different here.
    // TODO: look at encoding_rs and remove if redundant.
    if encoding != SHIFT_JIS {
        return false;
    }

    let len = cow.len();

    // If it's not even length, it can't be double byte.
    if len & 1 == 1 {
        return false;
    }

    // Double check that it's double-byte kanji

    for i in (0..len).step_by(2) {
        let byte = cow[i];

        // Taken from ZXing. I do not know how to read/write kanji, but
        // this range check is important for something.
        if !(0x81..=0x9F).contains(&byte) && !(0xE0..=0xEB).contains(&byte) {
            return false;
        }
    }

    true
}

// NUMERIC: 0-9 digits
fn init_numeric() -> Regex {
    Regex::new(r"^\d*$").expect("Numeric regex expected to compile without issue.")
}

// ALPHANUMERIC: 0-9, A-Z (uppercase only), $, %, *, +, -, ., /, :, and space
fn init_alphanumeric() -> Regex {
    Regex::new(r"^[\dA-Z$%*+\-\ \.\/\:]*$").expect("Alnum regex expected to compile without issue.")
}

// BYTE MODE: ISO-8859-1 charset. Some QR scanners can detect UTF8 (ECI specified) in byte mode.
fn init_latin1() -> Regex {
    Regex::new(r"^[\x00-\xff]*$").expect("ISO-8859-1 regex expected to compile without issue.")
}

#[cfg(feature = "kanji")]
// KANJI: Double-byte chars, Shift JIS charset (2 bytes vs utf8's 3-4)
fn init_kanji() -> Regex {
    Regex::new(r"^[\p{Han}\p{Hiragana}\p{Katakana}]*$")
        .expect("Kanji regex expected to compile without issue.")
}
