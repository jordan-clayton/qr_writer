use crate::ecc::ECCLevel;
use crate::encoding::{ENC_ALPHA, ENC_BYTES, ENC_KANJI, ENC_NUMERIC};
use crate::encoding::{get_bit_length, get_data_encoding_mode};
use crate::matrix::QRCodeMatrix;
use crate::reed_solomon::ReedSolomon;
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

// TODO: type alias for Version: Nonzero<usize>/usize

// TODO: implement better error handling
pub struct QRError;
// TODO: this driver function may best be factored out somewhere else so that it's easier to just
// look things up.
// This module file is going to get large.

// TODO: this should return a matrix.
pub fn encode_qr(data: &str, ecc_level: ECCLevel) -> QRCodeMatrix {
    let (interleaved_with_ecc, version, ecc_level) = prepare_qr_codewords(&data, ecc_level);
    // Convert back to a bitfield and add the remainder bits.
    // Remainder bits are by version only -> version counts from one, so it needs to be
    // decremented.
    let n_remainder_bits = REMAINDER_BITS[version - 1] as usize;

    // TODO: refactor assertions to Result during API cleanup
    let interleaved_bits = interleaved_with_ecc.len() * 8;
    // Cast to a bitfield and add the remainder bits.
    let mut bitfield = BitVec::<u8, Msb0>::from_vec(interleaved_with_ecc);
    bitfield.resize(bitfield.len() + n_remainder_bits, false);

    assert_eq!(bitfield.len(), interleaved_bits + n_remainder_bits);
    // Assert that the remainder bits are placed at the end--in case I might've slightly
    // misunderstood the bitvec api.
    assert!(
        bitfield[bitfield.len() - n_remainder_bits..]
            .iter()
            .all(|b| b == false)
    );

    // The rest of the algorithm is driven by the work in matrix.rs
    // NOTE: this does not render the QR code into a final bitfield.
    // call QRCode::render()
    QRCodeMatrix::new(&bitfield, version, ecc_level)
}

// TODO: seriously consider a series of structs to carry the data over tuple structs.
// Returns the interleaved codeword/ecc vector to be massaged back into a bitfield.
// For now:
// -> propagate version and ecc_level in a tuple-struct: (codewords, version, ecc_level)
pub(crate) fn prepare_qr_codewords(data: &str, ecc_level: ECCLevel) -> (Vec<u8>, usize, ECCLevel) {
    // Encode data codewords
    let (bytes, version, ecc) = encode_data_to_bytes(data, ecc_level);

    // This is overly cautious, but this is just to catch carelessness.
    // The ECC level should not be modified/changed in the data encoding process.
    assert_eq!(ecc, ecc_level);

    // Compute the groups/blocks
    let data_blocks = compute_blocks(bytes.len(), ecc_level, version);
    // Look up the number of ecc_codewords per block

    let idx = (version - 1) * 4 + ecc_level.capacity_idx();
    let ec_bytes = EC_CODEWORDS_PER_BLOCK[idx] as usize;

    // Compute error correction codewords
    let (ecc_bytes, ecc_blocks) = compute_ecc_codewords(&bytes, &data_blocks, ec_bytes);

    // Max number of data bytes = max(group1 data bytes, group2 data bytes)
    let max_data_bytes_per_block = NUM_DATA_CODEWORDS_PER_BLOCK_GROUP_1[idx]
        .max(NUM_DATA_CODEWORDS_PER_BLOCK_GROUP_2[idx]) as usize;

    // Perform the interleaving and return
    let interleaved = interleave_codewords(
        &bytes,
        &data_blocks,
        &ecc_bytes,
        &ecc_blocks,
        max_data_bytes_per_block,
        ec_bytes,
    );
    (interleaved, version, ecc_level)
}

pub(crate) fn encode_data_to_bytes(data: &str, ecc_level: ECCLevel) -> (Vec<u8>, usize, ECCLevel) {
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
        Ok(n_bits) => n_bits as usize,
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
            // The encoding is already checked to be able to actually return ENC_KANJI
            let (cow, _enc, has_errors) = SHIFT_JIS.encode(data);
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
    let num_codewords = TOTAL_NUM_CODEWORDS_BY_VERSION_AND_EC_LEVEL
        [((version - 1) * 4) as usize + ecc_level.capacity_idx()];
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
    // Append the version and the ecc level together for unpacking
    (bits.into_vec(), version as usize, ecc_level)
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

// ------GROUPING AND BLOCK SEGMENTATION-------
// TODO: decide whether it's worth it to try and do it by collections of indices
// It -should- be faster to just index the data/codeword block and flatten the error codewords.
//
// It is much easier to reason about as a multidimensional array though, so if this is painful,
// Massage the data into an n-d array of codewords and accept the penalty.
// Both ZXing and rxing use a Struct pair of Block/ECC as vectors.
//
// This could be a transparent tuple block
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub(crate) enum GroupNo {
    Group1,
    Group2,
}

impl TryFrom<usize> for GroupNo {
    type Error = ();
    fn try_from(n: usize) -> Result<Self, Self::Error> {
        match n {
            0 => Ok(Self::Group1),
            1 => Ok(Self::Group2),
            _ => Err(()),
        }
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub(crate) struct Block(
    (
        // start_idx
        usize,
        // end_idx -> index not number of elements.
        // num_elements is end_idx - start_idx + 1
        usize,
    ),
);

impl Block {
    // The table index should be passed in and precomputed.
    // (version -1) * 4 + ecc_codeword_idx() -> when version is 1-idxd
    pub(crate) fn new_data_block(start_idx: usize, group_no: GroupNo, table_idx: usize) -> Self {
        let num_codewords_per_block = match group_no {
            GroupNo::Group1 => NUM_DATA_CODEWORDS_PER_BLOCK_GROUP_1[table_idx],
            GroupNo::Group2 => {
                let g2_block_codewords = NUM_DATA_CODEWORDS_PER_BLOCK_GROUP_2[table_idx];
                assert!(
                    g2_block_codewords > 0,
                    "table_idx is wrong: {table_idx}. Invalid version or ecc level."
                );
                g2_block_codewords
            }
        };

        // Start is just the start_idx.
        // End index is start_idx + num_codewords_per_block - 1
        let end_idx = start_idx + (num_codewords_per_block as usize) - 1;
        Self((start_idx, end_idx))
    }

    // NOTE: the interface for this function doesn't match the data block
    // This function's only use is in the codeword computation, where the index arithmetic is
    // handled within the loop (rather than having to reverse-engineer it)
    // If this poses genuine friction, then swap so that the interfaces and implementations are
    // similar and correct in the ECC computation function.
    pub(crate) fn new_ecc_block(start_idx: usize, end_idx: usize) -> Self {
        Self((start_idx, end_idx))
    }

    pub(crate) fn start_idx(&self) -> usize {
        self.0.0
    }
    pub(crate) fn end_idx(&self) -> usize {
        self.0.1
    }
}

#[repr(transparent)]
pub(crate) struct Group(Vec<Block>);

impl Group {
    // The table index should be passed in and precomputed.
    // (version -1) * 4 + ecc_codeword_idx() -> when version is 1-idxd
    pub(crate) fn new(start_idx: usize, group_no: GroupNo, table_idx: usize) -> Self {
        let mut idx = start_idx;

        // Number of blocks per group is going to depend on the group number.
        let num_blocks_per_group = match group_no {
            GroupNo::Group1 => NUM_BLOCKS_GROUP_1[table_idx],
            GroupNo::Group2 => {
                let g2_blocks = NUM_BLOCKS_GROUP_2[table_idx];
                // TODO: refactor to result once done.
                assert!(
                    g2_blocks > 0,
                    "table_idx is wrong: {table_idx}. Invalid version or ecc level."
                );
                g2_blocks
            }
        };

        let mut res = Vec::with_capacity(num_blocks_per_group as usize);

        for _ in 0..num_blocks_per_group {
            let block = Block::new_data_block(idx, group_no, table_idx);
            // Update the indices, the idx is now end_idx + 1
            idx = block.end_idx() + 1;
            res.push(block);
        }

        Self(res)
    }

    pub(crate) fn blocks(&self) -> &[Block] {
        &self.0
    }
}

// POSSIBLY CONVERT THIS TO AN ITERATOR?
#[repr(transparent)]
pub(crate) struct QrSegmentation(Vec<Group>);

impl QrSegmentation {
    // SUPPLY VERSION 1-indexed
    pub(crate) fn new(total_codewords: usize, ecc_level: ECCLevel, version: usize) -> Self {
        // Table indexing:
        // (version -1) * 4 + ecc_codeword_idx()

        let table_idx = (version - 1) * 4 + ecc_level.capacity_idx();
        let mut idx = 0;

        // At the moment, there can be at most 2 groups.
        // Check first before initializing groups.
        // A table-lookup is extremely cheap and saves on pointless allocations.
        let num_groups = if NUM_BLOCKS_GROUP_2[table_idx] > 0 {
            2
        } else {
            1
        };

        let mut res = Vec::with_capacity(num_groups);
        for i in 0..num_groups {
            let group_no = i
                .try_into()
                .expect("There are only two groups, this should map 1:1 with a C enum");
            let group = Group::new(idx, group_no, table_idx);
            // Update the index, is going to be last block's end_idx + 1
            let end_idx = group
                .blocks()
                .last()
                .expect("There should be at least one block in the group.")
                .end_idx();

            // New starting index.
            idx = end_idx + 1;

            // Push the group to the groups vector
            res.push(group);
        }

        // (This should really be a result/error condition)
        // TODO: refactor out the panics/assertions once the code is tested.
        // Assert that the last index + 1 = total_codewords.
        //
        let end_idx = res
            .last()
            .expect("There should be at least one group.")
            .blocks()
            .last()
            .expect("There should be at least one block in the group.")
            .end_idx();

        assert_eq!(
            end_idx + 1,
            total_codewords,
            "INDEX ARITHMETIC IS INCORRECT."
        );

        // Wrap the group vector and return.
        Self(res)
    }
    pub(crate) fn groups(&self) -> &[Group] {
        &self.0
    }

    // Grab the block, return an inclusive slice [start_idx..=end_idx]
    pub(crate) fn get_block<'a>(&self, data: &'a [u8], group: usize, block: usize) -> &'a [u8] {
        // TODO: refactor to get/result once bugs teased out.
        let block = &self.groups()[group].blocks()[block];
        &data[block.start_idx()..=block.end_idx()]
    }

    // This gives a constant stream of blocks
    // It's easiest to do the interleaving as a nested loop, eg:
    // for byte in 0..data.len(){
    //      for block in 0..blocks.len() {
    //          write(data[block.start_idx + byte]
    //      }
    // }
    pub(crate) fn flatten_to_blocks(self) -> Vec<Block> {
        self.0.into_iter().flat_map(|group| group.0).collect()
    }
}

//  --------- STRUCTURING THE FINAL MESSAGE (for writing) ----------
//  - Segment the data codewords into blocks
//      - Flatten the segmentation struct into a list of blocks.
//  - Compute the ECC codewords per block
//
//  Note: there is a different number of ECC codewords than data codewords per block,
//  but all ECC codeword blocks have the same number of codewords per version/ecc level
//  (table lookup)

// SUPPLY VERSION NUMBER AS 1-INDEXED
#[inline]
pub(crate) fn compute_blocks(
    num_data_codewords: usize,
    ecc_level: ECCLevel,
    version: usize,
) -> Vec<Block> {
    // Segment the data
    let segmentation = QrSegmentation::new(num_data_codewords, ecc_level, version);
    // Flatten it.
    segmentation.flatten_to_blocks()
}

// Parent should pre-look up this information.
// let idx = ((version - 1) * 4) + ecc_level.capacity_idx();
// let codewords_per_block = EC_CODEWORDS_PER_BLOCK[idx] as usize;

// SUPPLY VERSION NUMBER AS 1-INDEXED
pub(crate) fn compute_ecc_codewords(
    data: &[u8],
    blocks: &[Block],
    ec_codewords_per_block: usize,
) -> (Vec<u8>, Vec<Block>) {
    let mut reed_solomon = ReedSolomon::new();

    let num_blocks = blocks.len();
    // Preallocate a vector of sufficient size for the codewords.
    let ecc_vec_size = num_blocks * ec_codewords_per_block;
    let mut res = Vec::with_capacity(ecc_vec_size);
    let mut ecc_block_data = Vec::with_capacity(blocks.len());

    let mut ecc_block_idx = 0;
    for block in blocks {
        let start_idx = block.start_idx();
        let end_idx = block.end_idx();
        let mut next_ecc_bytes =
            reed_solomon.encode(&data[start_idx..=end_idx], ec_codewords_per_block);

        assert_eq!(next_ecc_bytes.len(), ec_codewords_per_block);

        let next_ecc_block =
            Block::new_ecc_block(ecc_block_idx, ecc_block_idx + next_ecc_bytes.len() - 1);

        ecc_block_idx += next_ecc_bytes.len();
        res.append(&mut next_ecc_bytes);
        ecc_block_data.push(next_ecc_block);
    }

    (res, ecc_block_data)
}

// This function only does the interleaving.
// The remainder bits are not handled until later.
fn interleave_codewords(
    data: &[u8],
    data_blocks: &[Block],
    ecc_bytes: &[u8],
    ecc_blocks: &[Block],
    // These can be looked up.
    max_data_bytes_per_block: usize,
    ec_bytes_per_block: usize,
) -> Vec<u8> {
    let mut interleave = Vec::with_capacity(data.len() + ecc_blocks.len());
    // Codes with multiple groups have different numbers of bytes per block.
    for byte_offset in 0..max_data_bytes_per_block {
        for block in data_blocks {
            let data_idx = block.start_idx() + byte_offset;
            // If it's -in- the range of the block, we write it.
            if data_idx <= block.end_idx() {
                interleave.push(data[data_idx]);
            }
        }
    }

    for byte_offset in 0..ec_bytes_per_block {
        for block in ecc_blocks {
            let data_idx = block.start_idx() + byte_offset;
            if data_idx <= block.end_idx() {
                interleave.push(ecc_bytes[data_idx]);
            }
        }
    }

    interleave
}
