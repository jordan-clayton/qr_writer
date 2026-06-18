use crate::ecc::ECCLevel;
use crate::encoding::{ENC_ALPHA, ENC_BYTES, ENC_KANJI, ENC_NUMERIC};
use crate::encoding::{get_bit_length, get_data_encoding_mode};
use crate::tables::*;
use crate::versioning::get_min_required_version;
use bitvec::prelude::*;
use itertools::Itertools;

#[cfg(feature = "kanji")]
use encoding_rs::*;

// Cleanup TODO: magic constants.
const PADDING_HI: u8 = 236;
const PADDING_LOW: u8 = 17;

const ONE_DIGIT_BITLEN: usize = 4;
const TWO_DIGIT_BITLEN: usize = 7;
const THREE_DIGIT_BITLEN: usize = 10;

// Move the sign bit to bit 12 (sign extend), complement and then cast to u16
// This masks off the bottom 11 bits per the Alphanumeric encoding scheme
// The top bits will all be zeroed out.
const ALPHA_TWO_CHAR: u16 = !((1u16 << 15) as i16 >> 4) as u16;
// Move the sign bit to bit 7 (sign extend), complement and then cast to u16
// Odd Alphanumeric strings use a 6-bit integer as the final bit-string
const ALPHA_ONE_CHAR: u16 = !((1u16 << 15) as i16 >> 9) as u16;
const ALPHA_LENGTH: usize = 11;
const ALPHA_HALF_LENGTH: usize = 6;

const BYTE_LENGTH: usize = 8;
const KANJI_BIT_LEN: usize = 13;

// I'm not interested in getting very clever with masking/shifts to operate at byte-level
// For now, bitvec is fine.

// TODO: implement better error handling
pub struct QRError;
pub fn encode_qr(data: &str, ecc_level: ECCLevel) -> Vec<u8> {
    let bytes = encode_data_to_bytes(data, ecc_level);
    // Error correction.
    todo!("Finish encode qr.");
}

// TODO: make private once project organized, if possible
// For now, public is fine and required for testing.
pub fn encode_data_to_bytes(data: &str, ecc_level: ECCLevel) -> Vec<u8> {
    #[cfg(feature = "kanji")]
    let mut char_count = data.len();
    #[cfg(not(feature = "kanji"))]
    let char_count = data.len();

    let mode = get_data_encoding_mode(data);

    // If ENC_KANJI, the char count needs to be corrected.
    // SHIFT_JIS 2-byte encoding are 3-byte in utf-8.
    #[cfg(feature = "kanji")]
    {
        if mode == ENC_KANJI {
            assert_eq!(char_count.rem_euclid(3), 0);
            char_count /= 3;
        }
    }

    let version = get_min_required_version(char_count, mode, ecc_level);
    // This is only for encoding the number of characters.
    let bit_length = match get_bit_length(mode, version) {
        Ok(version) => version as usize,
        Err(_) => {
            panic!("Invalid QR Version supplied!")
        }
    };

    let prealloc_size = match mode {
        ENC_NUMERIC => THREE_DIGIT_BITLEN,
        ENC_ALPHA => ALPHA_LENGTH,
        ENC_BYTES => BYTE_LENGTH,
        #[cfg(feature = "kanji")]
        ENC_KANJI => KANJI_BIT_LEN,
        #[cfg(not(feature = "kanji"))]
        ENC_KANJI => unreachable!("Non feature-supported kanji should be treated as bytes."),
        _ => panic!("INVALID MODE: {mode}"),
    };

    // Preallocate a bitarray.
    let array_len = (char_count + 1) * prealloc_size as usize + 4;
    // Lsb0 has better codegen.
    // For now, and per the information I can find, I think this has to be MSB.
    // I'm not quite sure if/whether it's possible/worth the headache to try and swap.
    // To do this LE, would have to be Lsb0 traversal, le stores, and the insertion writing
    // needs to be backward (end of bitvec -> front of bitvec).
    // After which, the entire bitstring would need to be reversed for writing to an image (I
    // believe).
    // Only look into this if speed becomes an issue.
    let mut bits = bitvec![u8, Msb0; 0; array_len];
    let idx = 4;
    bits[0..idx].store_be(mode);
    bits[idx..idx + bit_length].store_be(char_count);

    // idx += bit_length;
    // Finish processing the data.
    let mut end_idx = match mode {
        ENC_NUMERIC => encode_numeric(data, &mut bits, idx + bit_length),
        ENC_ALPHA => encode_alpha(data, &mut bits, idx + bit_length),
        ENC_BYTES => encode_bytes(data, &mut bits, idx + bit_length),
        #[cfg(feature = "kanji")]
        ENC_KANJI => {
            // The data needs to be re-encoded from unicode over to ShiftJIS
            let (cow, enc, has_errors) = SHIFT_JIS.encode(data);
            assert!(!has_errors);
            // Take ownership (copy if needed) of the data and pass the bytes to encode_kanji.
            let data = cow.into_owned();
            encode_kanji(&data, &mut bits, idx + bit_length)
        }
        #[cfg(not(feature = "kanji"))]
        ENC_KANJI => unreachable!("Non feature-supported kanji should be treated as bytes."),
        _ => panic!("INVALID MODE: {mode}"),
    };

    // Get the number of codewords
    let num_codewords =
        CODEWORDS_BY_VERSION_EC_LEVEL[((version - 1) * 4) as usize + ecc_level.capacity_idx()];
    // Compute the total number of bits for the QR.
    let total_bits = num_codewords * 8;

    // Extend the bitfield up to total_bits in-case we're not the right size.
    bits.resize(total_bits as usize, false);

    // Add terminator bits.
    // end_idx is an index, so it's already - 1 => end_idx is the number of bits written so far.
    let num_terminators = (total_bits as i32 - end_idx as i32).abs().clamp(0, 4) as usize;

    end_idx += num_terminators;

    // Have to pad 0's until next multiple of 8
    let rem = (8 - end_idx.rem_euclid(8)).rem_euclid(8);
    end_idx += rem;

    // Add padding until end_idx = total_bit - 1;
    // For now, just use a boolean, rather than trying to play with parity
    let mut hi = true;

    // end_idx -is- the total number of bits written so far, so break once it hits total_bits
    // previous range-write will be end_idx.. end of bitvector.
    while end_idx < total_bits as usize {
        let byte = if hi { PADDING_HI } else { PADDING_LOW };
        bits[end_idx..end_idx + 8].store_be(byte);
        end_idx += 8;
        hi = !hi;
    }

    // At this point, we can convert into a vector of bytes and return it.
    bits.into_vec()
}

// TODO: make into result types.
// TODO: Refactor into result types after debugging. This function should never be called on
// non-numeric data.
fn encode_numeric(data: &str, bitfield: &mut BitVec<u8, Msb0>, start_idx: usize) -> usize {
    // The iterator will panic on 0-characters.
    if data.is_empty() {
        return start_idx;
    }

    let mut idx = start_idx;

    // Closures for triplet-processing
    let one_digit = |c1: char, bitfield: &mut BitVec<u8, Msb0>, idx: usize| {
        // Assert ascii char
        assert!(c1.is_ascii_digit());
        let num = (c1 as u8) - b'0';
        bitfield[idx..idx + ONE_DIGIT_BITLEN].store_be(num);
        ONE_DIGIT_BITLEN
    };
    let two_digit = |c1: char, c2: char, bitfield: &mut BitVec<u8, Msb0>, idx: usize| {
        assert!(c1.is_ascii_digit() && c2.is_ascii_digit());
        let num1 = (c1 as u16) - b'0' as u16;
        let num2 = (c2 as u16) - b'0' as u16;
        let num = num1 * 10 + num2;
        bitfield[idx..idx + TWO_DIGIT_BITLEN].store_be(num);
        TWO_DIGIT_BITLEN
    };
    let three_digit =
        |c1: char, c2: char, c3: char, bitfield: &mut BitVec<u8, Msb0>, idx: usize| {
            assert!(c1.is_ascii_digit() && c2.is_ascii_digit() && c3.is_ascii_digit());
            let num1 = (c1 as u16) - b'0' as u16;
            let num2 = (c2 as u16) - b'0' as u16;
            let num3 = (c3 as u16) - b'0' as u16;
            let num = num1 * 100 + num2 * 10 + num3;
            bitfield[idx..idx + THREE_DIGIT_BITLEN].store_be(num);
            THREE_DIGIT_BITLEN
        };

    // NOTE: Thonky reference diverges from ZXing/most QR implementations with its leading-zero
    // treatment. Leading zeros don't seem to be relevant here.
    for mut triplet in data.chars().chunks(3).into_iter() {
        let c1 = triplet.next();
        let c2 = triplet.next();
        let c3 = triplet.next();
        let inc = match (c1, c2, c3) {
            (Some(c1), Some(c2), Some(c3)) => three_digit(c1, c2, c3, bitfield, idx),
            (Some(c1), Some(c2), None) => two_digit(c1, c2, bitfield, idx),
            (Some(c1), None, None) => one_digit(c1, bitfield, idx),
            _ => unreachable!("The iterator cannot produce leading Nones"),
        };
        idx += inc;
    }

    idx
}

fn encode_alpha(data: &str, bitfield: &mut BitVec<u8, Msb0>, start_idx: usize) -> usize {
    // The iterator will panic on 0-characters.
    if data.is_empty() {
        return start_idx;
    }
    let mut idx = start_idx;

    // Iterate over pairs of characters in the string.
    for mut pair in data.chars().chunks(2).into_iter() {
        let c1 = pair.next();
        let c2 = pair.next();

        match (c1, c2) {
            (Some(c1), Some(c2)) => {
                let c1_val = get_alphanumeric_value(c1);
                let c2_val = get_alphanumeric_value(c2);
                let rval = 45 * c1_val + c2_val;
                let masked = rval & ALPHA_TWO_CHAR;
                bitfield[idx..idx + ALPHA_LENGTH].store_be(masked);
                idx += ALPHA_LENGTH;
            }
            // Perhaps this is firing multiple times.
            (Some(c1), None) => {
                let c1_val = get_alphanumeric_value(c1);
                let masked = c1_val & ALPHA_ONE_CHAR;
                bitfield[idx..idx + ALPHA_HALF_LENGTH].store_be(masked);
                idx += ALPHA_HALF_LENGTH;
            }
            _ => unreachable!("It's not possible to iterate more than 2 characters at once."),
        }
    }

    idx
}

// RETURNS THE IDX of the next insertion point.
fn encode_bytes(data: &str, bitfield: &mut BitVec<u8, Msb0>, start_idx: usize) -> usize {
    let mut idx = start_idx;
    // Try and re-encode to ISO 8859-1
    let bytes = match try_encode_iso_8859_1(data) {
        // ISO-8859-1
        Ok(bytes) => bytes,
        // UTF-8
        Err(_) => data.as_bytes().to_owned(),
    };

    // Fill the bitfield, padding taken care of by remaining bits
    // Last 4 bits should be terminator (if needed)
    for byte in bytes {
        bitfield[idx..idx + BYTE_LENGTH].store_be(byte);
        idx += BYTE_LENGTH;
    }
    idx
}

#[cfg(feature = "kanji")]
fn encode_kanji(data: &[u8], bitfield: &mut BitVec<u8, Msb0>, start_idx: usize) -> usize {
    let mut idx = start_idx;
    // This function can only be called if data's length is even.
    assert!(
        data.len() & 1 == 0,
        "Kanji non-even byte size: {}",
        data.len()
    );

    // 2 bytes = 1 kanji
    for chunk in data.chunks(2) {
        let (hi, lo) = match chunk {
            &[hi, lo] => (hi as u16, lo as u16),
            _ => unreachable!("Non-even kanji byte size."),
        };
        let double_byte: u16 = hi << 8 | lo;

        let subtraction = match double_byte {
            0x8140..=0x9FFC => double_byte - 0x8140,
            0xE040..=0xEBBF => double_byte - 0xC140,
            _ => unreachable!("Kanji should be in valid byte range."),
        };

        // Split hi and lo and do the following:
        // (hi * 0xC0) + lo
        let res = (subtraction >> 8) * 0xC0 + (subtraction & 0x00FF);
        // Encode as a 13-bit number.
        bitfield[idx..idx + KANJI_BIT_LEN].store_be(res);
        idx += KANJI_BIT_LEN;
    }

    idx
}

// TODO: better errors
pub struct ISOError;
fn try_encode_iso_8859_1(data: &str) -> Result<Vec<u8>, ISOError> {
    let mut bytes = Vec::with_capacity(data.len());
    for c in data.chars() {
        // Use 32-bits width to avoid overflow panic, cast on push.
        let code = c as u32;
        if code <= 255 {
            bytes.push(code as u8);
        } else {
            return Err(ISOError);
        }
    }
    Ok(bytes)
}

// TODO: Result type/better error handling.
// For now, just panic.
#[inline]
fn get_alphanumeric_value(c: char) -> u16 {
    match c {
        '0' => 0,
        '1' => 1,
        '2' => 2,
        '3' => 3,
        '4' => 4,
        '5' => 5,
        '6' => 6,
        '7' => 7,
        '8' => 8,
        '9' => 9,
        'A' => 10,
        'B' => 11,
        'C' => 12,
        'D' => 13,
        'E' => 14,
        'F' => 15,
        'G' => 16,
        'H' => 17,
        'I' => 18,
        'J' => 19,
        'K' => 20,
        'L' => 21,
        'M' => 22,
        'N' => 23,
        'O' => 24,
        'P' => 25,
        'Q' => 26,
        'R' => 27,
        'S' => 28,
        'T' => 29,
        'U' => 30,
        'V' => 31,
        'W' => 32,
        'X' => 33,
        'Y' => 34,
        'Z' => 35,
        ' ' => 36,
        '$' => 37,
        '%' => 38,
        '*' => 39,
        '+' => 40,
        '-' => 41,
        '.' => 42,
        '/' => 43,
        ':' => 44,
        _ => panic!("Invalid character: {c}"),
    }
}
