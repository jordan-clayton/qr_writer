use regex::Regex;
use std::cell::OnceCell;

// This could be an enumeration.
// Not sure yet; decide later.
pub const ENC_NUMERIC: u8 = 1;
pub const ENC_ALPHA: u8 = 2;
pub const ENC_BYTES: u8 = 4;
pub const ENC_KANJI: u8 = 8;

const NUMERIC: OnceCell<Regex> = OnceCell::new();
const ALPHANUMERIC: OnceCell<Regex> = OnceCell::new();
const LATIN1: OnceCell<Regex> = OnceCell::new();
const KANJI: OnceCell<Regex> = OnceCell::new();

// TODO: rethink visibility organization; public is fine until finished.
//
//
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
pub fn get_data_encoding_mode(data: &str) -> u8 {
    if NUMERIC.get_or_init(init_numeric).is_match(data) {
        ENC_NUMERIC
    } else if ALPHANUMERIC.get_or_init(init_alphanumeric).is_match(data) {
        ENC_ALPHA
    } else if LATIN1.get_or_init(init_latin1).is_match(data) {
        ENC_BYTES
    } else if KANJI.get_or_init(init_kanji).is_match(data) {
        ENC_KANJI
    } else {
        // ECI: Extended channel iterpretation
        // TODO: Deal with named constants/enums later.
        0b0111
    }
}

pub struct InvalidVersionError;
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

pub fn get_mode_idx(mode: u8) -> usize {
    // This is equivalent to floor(log2(mode)) and folds ECI to nearest power of 2 (4)
    (7 - mode.leading_zeros()) as usize
}

// TODO: tests module for encoding, make this function private until necessary.
pub fn get_bit_length(mode: u8, version: u8) -> Result<u8, InvalidVersionError> {
    const BIT_LENGTH: [u8; 12] = [
        10, 12, 14, // NUMERIC
        9, 11, 13, // ALPHANUMERIC
        8, 16, 16, // LATIN
        8, 10, 12, // KANJI
    ];

    let ver_idx = match version {
        1..=9 => 0,
        10..=26 => 1,
        27..=40 => 2,
        _ => return Err(InvalidVersionError),
    } as usize;

    let mode_idx = get_mode_idx(mode);
    assert!(
        mode_idx >= 0 && mode_idx <= 3,
        "Invalid mode idx: {mode_idx}, mode: {mode}, leading_zeros: {}",
        mode.leading_zeros()
    );
    Ok(BIT_LENGTH[mode_idx * 3 + ver_idx])
}

// TODO: check regexes
// NUMERIC: 0-9 digits
fn init_numeric() -> Regex {
    Regex::new(r"^\d*$").unwrap()
}

// ALPHANUMERIC: 0-9, A-Z (uppercase only), $, %, *, +, -, ., /, :, and space
fn init_alphanumeric() -> Regex {
    Regex::new(r"^[\dA-Z$%*+\-\ \.\/\:]*$").unwrap()
}

// BYTE MODE: ISO-8859-1 charset. Some QR scanners can detect UTF8 (ECI specified) in byte mode.
fn init_latin1() -> Regex {
    Regex::new(r"^[\x00-\xff]*$").unwrap()
}

// KANJI: Double-byte chars, Shift JIS charset (2 bytes vs utf8's 3-4)
fn init_kanji() -> Regex {
    Regex::new(r"^[\p{Han}\p{Hiragana}\p{Katakana}]*$").unwrap()
}
