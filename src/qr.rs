use crate::ecc::ECCLevel;
use crate::encoding::{ENC_ALPHA, ENC_BYTES, ENC_KANJI, ENC_NUMERIC};
use crate::encoding::{get_bit_length, get_data_encoding_mode};
use crate::versioning::get_min_required_version;
use bitvec::prelude::*;

const PADDING_HI: u8 = 236;
const PADDING_LOW: u8 = 17;

// I'm not interested in getting very clever with masking/shifts to operate at byte-level
// For now, bitvec is fine.

// TODO: grab the ECC codeblock table and place somewhere sensible
const CODEWORDS: [u8; 8] = [
    19, // 1-L
    16, // 1-M
    13, // 1-Q
    9,  // 1-H
    34, // 2-L
    28, // 2-M
    16, // 2-Q
    55, // 2-H
];

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
    let char_count = data.len();
    let mode = get_data_encoding_mode(data);
    let version = get_min_required_version(char_count, mode, ecc_level);
    let bit_length = match get_bit_length(mode, version) {
        Ok(version) => version as usize,
        Err(_) => {
            panic!("Invalid QR Version supplied!")
        }
    };

    // Preallocate a bitarray, no need to use vectors, just use an insertion pointer.
    let array_len = (char_count + 1) * bit_length as usize + 4;
    // Lsb0 has better codegen.
    // For now, and per the information I can find, I think this has to be MSB.
    // I'm not quite sure if/whether it's possible/worth the headache to try and swap.
    // To do this LE, would have to be Lsb0 traversal, le stores, and the insertion writing
    // needs to be backward (end of bitvec -> front of bitvec).
    // I -believe- this can be achieved by just calling bitvec.reverse()
    // then swap the endianness of Vec<u8>
    let mut bits = bitvec![u8, Msb0; 0; array_len];
    let idx = 4;
    bits[0..idx].store_be(mode);
    bits[idx..idx + bit_length].store_be(char_count);

    // idx += bit_length;
    // Finish processing the data.
    let mut end_idx = match mode {
        ENC_NUMERIC => encode_numeric(data, &mut bits, idx + bit_length, bit_length),
        ENC_ALPHA => encode_alpha(data, &mut bits, idx + bit_length, bit_length),
        ENC_BYTES => encode_bytes(data, &mut bits, idx + bit_length, bit_length),
        ENC_KANJI => encode_kanji(data, &mut bits, idx + bit_length, bit_length),
        _ => panic!("INVALID MODE: {}", mode),
    };

    // Get the number of codewords
    let num_codewords = CODEWORDS[(version * 4) as usize + ecc_level.capacity_idx()];
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
// TODO: return the INDEX
fn encode_numeric(
    data: &str,
    bitfield: &mut BitVec<u8, Msb0>,
    start_idx: usize,
    bit_length: usize,
) -> usize {
    let mut idx = start_idx;

    todo!("Implement encode numeric");
}

fn encode_alpha(
    data: &str,
    bitfield: &mut BitVec<u8, Msb0>,
    start_idx: usize,
    bit_length: usize,
) -> usize {
    todo!("Implement encode alphanumeric");
}

// RETURNS THE IDX of the next insertion point.
fn encode_bytes(
    data: &str,
    bitfield: &mut BitVec<u8, Msb0>,
    start_idx: usize,
    bit_length: usize,
) -> usize {
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
        bitfield[idx..idx + bit_length].store_be(byte);
        idx += bit_length;
    }
    idx
}

fn encode_kanji(
    data: &str,
    bitfield: &mut BitVec<u8, Msb0>,
    start_idx: usize,
    bit_length: usize,
) -> usize {
    todo!("Implement encode kanji");
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
