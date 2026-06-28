mod ecc;
mod encoding;
#[cfg(any(feature = "png", feature = "image", feature = "svg"))]
mod export;
mod galois;
mod mask;
mod matrix;
mod qr;
mod reed_solomon;
mod tables;
mod versioning;

// These are basically the extent of what's required to gain value from this api.
// (This will change once mask/version hints are implemented)
pub use ecc::ECCLevel;
#[cfg(any(feature = "image", feature = "png"))]
pub use export::render_image;
#[cfg(feature = "png")]
pub use export::save_png;
#[cfg(any(feature = "png", feature = "image", feature = "svg"))]
pub use export::{IntegerInverse, nearest_integer_multiple};
#[cfg(feature = "svg")]
pub use export::{Stroke, SvgHints, SvgRectHints, save_svg};
pub use matrix::QRCodeMatrix;
pub use qr::encode_qr;

// TODO: cleanup -> Format strings should either all have ending punctuation or none.
//      + additional cleaning to make these tests easier to read.
//
#[cfg(test)]
mod tests {
    use crate::ecc::ECCLevel;
    use crate::encoding::{EncodingHints, get_data_encoding_mode};
    use crate::galois::{
        EXP_TABLE, FIELD_SIZE, GaloisPolynomial, IRR_POLY, LOG_TABLE, REM, gf_exp, gf_inverse,
        gf_multiply, gf_poly_add, gf_poly_divide, gf_poly_mul, gf_poly_multiply_by_monomial,
        gf_poly_zero,
    };
    use crate::matrix::{
        QUIET_ZONE_SIZE, SquareMatrix, emplace_alignment_squares,
        emplace_finder_patterns_into_blank_matrix, emplace_timing_patterns, print_matrix_and_crash,
    };
    use crate::qr::{
        QrSegmentation, compute_ecc_codewords, encode_data_to_bytes, encode_qr,
        prepare_qr_codewords,
    };
    use crate::reed_solomon::ReedSolomon;
    use crate::tables::*;
    use crate::versioning::get_min_required_version;
    use std::num::NonZero;

    #[cfg(any(feature = "png", feature = "svg"))]
    use crate::export::{IntegerInverse, nearest_integer_multiple};
    #[cfg(feature = "png")]
    use crate::export::{resize_and_render_image_exact, save_png, write_png};
    #[cfg(any(feature = "png", feature = "svg"))]
    use std::path::{Path, PathBuf};
    // save_svg currently calls the render_svg_without_resampling internally
    // Expect this to be changed--render_svg_with_resampling may be removed if
    // the svg scaling is superior than a texture buffer resample (it most likely will be).
    // Testing is currently being used to become familiar with svg format and how it handles
    // scaling.
    #[cfg(feature = "svg")]
    use crate::export::{
        Stroke, SvgHints, SvgRectHints, render_svg_with_resampling, save_svg, write_svg,
    };

    // Using rxing to generate test cases for comparison against the encoder in this library.
    use rxing::qrcode::QRCodeWriter;

    // To access the output data and access bits
    // NOTE: these are still complemented internally (0 = false = white)
    // so these will need to be complemented on comparison.
    use rxing::common::BitMatrix;
    #[cfg(feature = "svg")]
    use rxing::qrcode::common::ErrorCorrectionLevel::L;
    use rxing::{BarcodeFormat, EncodeHintValue, EncodeHints, Writer};

    // This will only be used if the image/svg crates are pulled in.
    #[cfg(any(feature = "png", feature = "svg"))]
    const IMG_DIRECTORY_SLUG: &str = "test_images";

    // TESTS TO STILL BE IMPLEMENTED:
    // - Unit tests for the penalty functions
    // - Unit tests for resize/resampling

    #[test]
    fn test_regexes() {
        let numeric = "86784";
        let alnum = "XA956-B";
        let byte = "https://www.example.com";
        // Leave kanji for now.
        assert_eq!(get_data_encoding_mode(numeric), 1);
        assert_eq!(get_data_encoding_mode(alnum), 2);
        assert_eq!(get_data_encoding_mode(byte), 4);
    }

    #[test]
    fn test_byte_encoding() {
        // Taken from: https://dev.to/maxart2501/let-s-develop-a-qr-code-generator-part-ii-sequencing-data-4ae example.
        let data = "https://www.qrcode.com/";
        let expect: [u8; 28] = [
            65, 118, 135, 71, 71, 7, 51, 162, 242, 247, 119, 119, 114, 231, 23, 38, 54, 246, 70,
            82, 230, 54, 246, 210, 240, 236, 17, 236,
        ];

        // Ensure it's byte mode
        let mode = get_data_encoding_mode(data);
        assert_eq!(mode, 4);

        // Ensure it's version 2
        let version = get_min_required_version(data.len(), mode, ECCLevel::M);
        assert_eq!(version, 2);
        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::M);
        let (res, _, _) = encode_data_to_bytes(data, hints);
        assert_eq!(res, expect);
    }
    #[test]
    fn test_alphanumeric_encoding() {
        // Taken from: https://www.thonky.com/qr-code-tutorial/data-encoding#add-pad-bytes-if-the-string-is-still-too-short
        let data = "HELLO WORLD";
        let expect: [u8; 13] = [
            0b00100000, 0b01011011, 0b00001011, 0b01111000, 0b11010001, 0b01110010, 0b11011100,
            0b01001101, 0b01000011, 0b01000000, 0b11101100, 0b00010001, 0b11101100,
        ];

        // Ensure it's alphanumeric
        let mode = get_data_encoding_mode(data);
        assert_eq!(mode, 2);

        // Ensure it's version 1
        let version = get_min_required_version(data.len(), mode, ECCLevel::Q);
        assert_eq!(version, 1);

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        // This should be a V1-Q code
        let (res, _, _) = encode_data_to_bytes(data, hints);
        assert_eq!(res, expect);

        // TEST CASE FOR V7
        // Alphanumeric, version 1, ecc level Q
        // String is: HELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLD

        let v7_test = data.repeat(11);
        let v7_version = get_min_required_version(v7_test.len(), 2, ECCLevel::Q);

        assert_eq!(v7_version, 7);

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);

        let (processed, _, _) = encode_data_to_bytes(&v7_test, hints);

        let expected_v7: [u8; 88] = [
            0x23, 0xCB, 0x0B, 0x78, 0xD1, 0x72, 0xDC, 0x4D, 0x44, 0xB4, 0xA2, 0xDE, 0x4E, 0x74,
            0x8A, 0x6E, 0xF9, 0x85, 0xBC, 0x68, 0xB9, 0x6E, 0x26, 0xA2, 0x5A, 0x51, 0x6F, 0x27,
            0x3A, 0x45, 0x37, 0x7C, 0xC2, 0xDE, 0x34, 0x5C, 0xB7, 0x13, 0x51, 0x2D, 0x28, 0xB7,
            0x93, 0x9D, 0x22, 0x9B, 0xBE, 0x61, 0x6F, 0x1A, 0x2E, 0x5B, 0x89, 0xA8, 0x96, 0x94,
            0x5B, 0xC9, 0xCE, 0x91, 0x4D, 0xDF, 0x30, 0xB7, 0x8D, 0x17, 0x2D, 0xC4, 0xD4, 0x4B,
            0x4A, 0x2D, 0xE4, 0xE7, 0x48, 0xA6, 0xEF, 0x98, 0x5B, 0xC6, 0x8B, 0x96, 0xE2, 0x6A,
            0x1A, 0x00, 0xEC, 0x11,
        ];

        assert_eq!(processed, expected_v7, "Error with v7 data encoding.");
    }

    #[test]
    fn test_numeric_encoding() {
        // Taken from: https://www.thonky.com/qr-code-tutorial/numeric-mode-encoding
        let data = "8675309";

        // Binary string:
        // 110110001110000100101001
        // EC level Q -> 13 codepoints.
        let expect: [u8; 13] = [
            0b00010000, 0b00011111, 0b01100011, 0b10000100,
            0b10100100, // 2 bits of the terminator in last byte.
            // Add 2 zero-bits + 6 for byte alignment = one zero-byte
            0b00000000, // Padding starts after this byte
            236, 17, 236, 17, 236, 17, 236,
        ];

        let mode = get_data_encoding_mode(data);

        // Ensure it's numeric mode
        assert_eq!(mode, 1);

        // Ensure it's version 1.
        let version = get_min_required_version(data.len(), mode, ECCLevel::Q);
        assert_eq!(version, 1);
        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        let (res, _, _) = encode_data_to_bytes(data, hints);
        assert_eq!(res, expect);
    }

    #[cfg(feature = "kanji")]
    #[test]
    fn test_kanji() {
        // Taken from thonky: https://www.thonky.com/qr-code-tutorial/kanji-mode-encoding
        let data = "茗荷";
        // Kanji size in version 1 = 8 bits
        let expect: [u8; 9] = [
            0b10000000, 0b00101101, 0b01010101, 0b00011010,
            0b01011100, // 2 bits of extra padding
            // + 2 more bits (4-bit total terminator) + 6 bits for alignment.
            0b00000000, 236, 17, 236,
        ];

        let mode = get_data_encoding_mode(data);
        // Assert kanji mode
        assert_eq!(mode, 8);

        // Data needs to be corrected. 3-byte kanji in utf-8 will be oversized.
        assert_eq!(data.len().rem_euclid(3), 0);
        let char_count = data.len() / 3;

        // Assert version 1.
        let version = get_min_required_version(char_count, mode, ECCLevel::H);
        assert_eq!(version, 1);

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::H);
        // EC level H -> 9 codepoints.
        let (res, _, _) = encode_data_to_bytes(data, hints);
        assert_eq!(res, expect);
    }

    // This is a quick sanity check to ensure the recursive constant compile-time procedure matches
    // its iterative equivalent.
    // The tables need to be correct for GF(256) for reed-solomon.
    #[test]
    fn test_galois_tables() {
        let mut exp = [0usize; FIELD_SIZE];
        let mut log = [0usize; FIELD_SIZE];

        let mut x = 1usize;
        for e in exp.iter_mut() {
            *e = x;
            x *= 2;

            if x >= 256 {
                x ^= IRR_POLY;
                // This just masks out the high bits from the XOR.
                x &= REM;
            }
        }

        // log_2(0) is undefined.
        for (i, e) in exp.iter().enumerate().take(REM) {
            log[*e] = i;
        }

        assert_eq!(log[0], 0);

        assert_eq!(exp, EXP_TABLE);
        assert_eq!(log, LOG_TABLE);
    }

    // There's a very-weird case that's happening with 54 * 29
    // It -should- be 4 (?) and it's returning 2
    #[test]
    fn test_gf_multiply() {
        let a = 54usize;
        let b = 29usize;

        // Integer 54: a^249 <=> a^249 = 54
        // Integer: 29: a^8 <=> a^8 = 29

        // Check table values.
        assert_eq!(LOG_TABLE[a], 249);
        assert_eq!(EXP_TABLE[249], a);

        assert_eq!(LOG_TABLE[b], 8);
        assert_eq!(EXP_TABLE[8], b);

        let expected_exp = (249usize + 8).rem_euclid(255);
        assert_eq!(expected_exp, 2);

        let res = gf_multiply(a, b);
        assert_eq!(res, 4);
    }

    #[test]
    fn test_gf_inverse() {
        let a = gf_exp(4);
        let inv = gf_inverse(a);

        let testval = gf_multiply(a, inv);
        let rem = testval.rem_euclid(FIELD_SIZE);

        assert!(testval > 0);
        assert_eq!(rem, 1, "a: {a}, inv: {inv}, testval: {testval}");
    }

    // TODO: test polynomial operations.

    #[test]
    fn test_polynomial_add() {
        // 83x^2 + 202
        let a = GaloisPolynomial::new(&[83, 0, 202]);
        // 31x + 153
        let b = GaloisPolynomial::new(&[0, 31, 153]);
        // 83 XOR 0 = 83, 0 XOR 31 = 31
        // 202 = 0xCA = 0b11001010
        // 153 = 0x99 = 0b10011001
        // XOR => 0b01010011 = 0x53 = 83
        let expected = GaloisPolynomial::new(&[83, 31, 83]);

        let c = gf_poly_add(&a, &b);
        assert_eq!(c, expected);
    }

    #[test]
    fn test_polynomial_multiply() {
        // x + 2
        let a = GaloisPolynomial::new(&[1, 2]);
        // x + 3
        let b = GaloisPolynomial::new(&[1, 3]);
        // (x + 2)(x + 3) = x^2 + x + 6
        let expected = GaloisPolynomial::new(&[1, 1, 6]);
        let c = gf_poly_mul(&a, &b);
        assert_eq!(c, expected);
    }

    // TODO: more division tests worked out by hand
    // This is just the inverse of the multiplication, but it should tease out immediate errors.
    #[test]
    fn test_polynomial_divide() {
        let dividend = GaloisPolynomial::new(&[1, 1, 6]);
        let divisor = GaloisPolynomial::new(&[1, 3]);

        let expect_quotient = GaloisPolynomial::new(&[1, 2]);
        let expect_remainder = gf_poly_zero();

        let (quotient, remainder) = gf_poly_divide(&dividend, &divisor);
        assert_eq!(quotient, expect_quotient, "Quotient failure");
        assert_eq!(remainder, expect_remainder, "Remainder failure");
    }

    #[test]
    fn test_multiply_by_monomial() {
        // x + 2
        let a = GaloisPolynomial::new(&[1, 2]);
        // * (x ^ 2)
        // (x + 2)(x^2) = (x^3 + 2x^2) = [1, 2, 0, 0]
        let expect = GaloisPolynomial::new(&[1, 2, 0, 0]);
        let res = gf_poly_multiply_by_monomial(&a, 2, 1);
        assert_eq!(res, expect);
    }

    // TODO:
    // - Test to confirm that poly_multiplication with 0-degree monomials still properly multiply.

    // This is to tease out discrepancies between the EXP table and the
    // examples on Thonky.com
    #[test]
    fn test_alphas() {
        let coeffs = [
            EXP_TABLE[84] as u8,  // a^84
            EXP_TABLE[80] as u8,  // a^80
            EXP_TABLE[151] as u8, // a^151
            EXP_TABLE[130] as u8, // .. and so on
            EXP_TABLE[145] as u8,
            EXP_TABLE[202] as u8,
            EXP_TABLE[154] as u8,
            EXP_TABLE[148] as u8,
            EXP_TABLE[178] as u8,
            EXP_TABLE[116] as u8,
            EXP_TABLE[129] as u8,
        ];

        let expect_coeffs = [107, 253, 170, 46, 77, 112, 57, 82, 171, 248, 23];

        assert_eq!(coeffs, expect_coeffs);
    }

    #[test]
    fn test_log_table() {
        let coeffs = [
            LOG_TABLE[107] as u8, // Inverse of the EXP test
            LOG_TABLE[253] as u8, //
            LOG_TABLE[170] as u8, //
            LOG_TABLE[46] as u8,  //
            LOG_TABLE[77] as u8,
            LOG_TABLE[112] as u8,
            LOG_TABLE[57] as u8,
            LOG_TABLE[82] as u8,
            LOG_TABLE[171] as u8,
            LOG_TABLE[248] as u8,
            LOG_TABLE[23] as u8,
        ];

        let expect_coeffs = [84, 80, 151, 130, 145, 202, 154, 148, 178, 116, 129];

        assert_eq!(coeffs, expect_coeffs);
    }

    #[test]
    fn test_build_generator() {
        // Version 1-M
        let version = 1;
        let idx = (version - 1) * 4 + ECCLevel::M.capacity_idx();
        let ec_bytes = EC_CODEWORDS_PER_BLOCK[idx] as usize;
        assert_eq!(ec_bytes, 10);
        // Using degree 10, equivalent to the reed_solomon test case.

        // Expected polynomial generated by: https://www.thonky.com/qr-code-tutorial/generator-polynomial-tool?degree=10
        // ɑ0x10 + ɑ251x9 + ɑ67x8 + ɑ46x7 + ɑ61x6 + ɑ118x5 + ɑ70x4 + ɑ64x3 + ɑ94x2 + ɑ32x + ɑ45
        // a = 2 (in GF256)

        let expect_coeffs = [
            EXP_TABLE[0] as u8,   // a0
            EXP_TABLE[251] as u8, // a251
            EXP_TABLE[67] as u8,  // a67
            EXP_TABLE[46] as u8,  // .. and so on
            EXP_TABLE[61] as u8,
            EXP_TABLE[118] as u8,
            EXP_TABLE[70] as u8,
            EXP_TABLE[64] as u8,
            EXP_TABLE[94] as u8,
            EXP_TABLE[32] as u8,
            EXP_TABLE[45] as u8,
        ];

        // Compute the generator polynomial.
        let mut rs_encoder = ReedSolomon::new();
        let generator = rs_encoder.build_generator(ec_bytes);
        let generator_coefficients = generator.coefficients();

        // Double check the exponents are correct first.

        let expected_expts = [0, 251, 67, 46, 61, 118, 70, 64, 94, 32, 45];

        let coeff_expts = generator_coefficients
            .iter()
            .copied()
            .map(|c| LOG_TABLE[c as usize])
            .collect::<Vec<_>>();

        assert_eq!(coeff_expts, expected_expts);

        assert_eq!(generator_coefficients.len(), expect_coeffs.len());
        assert_eq!(generator_coefficients, expect_coeffs);
    }

    // TODO: TESTS FOR VERSION AND FORMAT STRINGS - COMPARE WITH KNOWN STRINGS FROM REFERENCE TABLE.
    #[test]
    fn test_sample_format_strings() {
        // Indexing: ecc.capacity_idx * 8 + mask idx

        // L: 0, 4, 7
        let masks = [0usize, 4, 7];

        let l_strings = masks
            .iter()
            .map(|&m| FORMAT_INFO_STRINGS[m])
            .collect::<Vec<_>>();
        let expected_l_strings = [0b111011111000100, 0b110011000101111, 0b110100101110110];
        assert_eq!(l_strings, expected_l_strings, "L-String failure.");

        // M: 0, 4, 7
        let m_strings = masks
            .iter()
            .map(|&m| FORMAT_INFO_STRINGS[1 * 8 + m])
            .collect::<Vec<_>>();

        let expected_m_strings = [0b101010000010010, 0b100010111111001, 0b100101010100000];
        assert_eq!(m_strings, expected_m_strings, "M-String failure.");
        // Q: 0, 4, 7

        let q_strings = masks
            .iter()
            .map(|&m| FORMAT_INFO_STRINGS[2 * 8 + m])
            .collect::<Vec<_>>();

        let expected_q_strings = [0b011010101011111, 0b010010010110100, 0b010101111101101];

        assert_eq!(q_strings, expected_q_strings, "Q-String Failure.");
        // H: 0, 4, 7
        let h_strings = masks
            .iter()
            .map(|&m| FORMAT_INFO_STRINGS[3 * 8 + m])
            .collect::<Vec<_>>();

        let expected_h_strings = [0b001011010001001, 0b000011101100010, 0b000100000111011];
        assert_eq!(h_strings, expected_h_strings, "H-String Failure.");
    }

    #[test]
    fn test_sample_version_strings() {
        // Like above, do a 12-sample of the version information strings
        // Indexing: version (counting from 1) - 7;

        // every 3rd string.
        let versions = [
            7, 10, 13, 16, //
            19, 22, 25, 28, //
            31, 34, 37, 40, //
        ];

        let version_strings = versions
            .iter()
            .map(|&v| VERSION_INFO_STRINGS[v - 7])
            .collect::<Vec<_>>();

        let expected_version_strings = [
            0b000111110010010100, // 7
            0b001010010011010011, // 10
            0b001101100001000111, // 13
            0b010000101101111000, // 16
            0b010011010100110010, // 19
            0b010110100011001001, // 22
            0b011001000111100001, // 25
            0b011100110000011010, // 28
            0b011111001001010000, // 31
            0b100010100010111010, // 34
            0b100101010000101110, // 37
            0b101000110001101001, // 40
        ];

        assert_eq!(version_strings, expected_version_strings);
    }

    #[test]
    fn test_reed_solomon() {
        // Taken from: https://www.thonky.com/qr-code-tutorial/error-correction-coding#step-8-generating-error-correction-codewords
        let data = "HELLO WORLD";
        let expect_data_codewords: [u8; 16] = [
            32, 91, 11, 120, 209, 114, 220, 77, 67, 64, 236, 17, 236, 17, 236, 17,
        ];
        let expect_ec_codewords: [u8; 10] = [196, 35, 39, 119, 235, 215, 231, 226, 93, 23];

        // Ensure it's alphanumeric
        let mode = get_data_encoding_mode(data);
        assert_eq!(mode, 2);

        // Ensure it's version 1
        let version = get_min_required_version(data.len(), mode, ECCLevel::M);
        assert_eq!(version, 1);

        // This should be a V1-M code
        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::M);
        let (data_codewords, _, _) = encode_data_to_bytes(data, hints);
        assert_eq!(data_codewords, expect_data_codewords);

        // Encode the ec bytes.
        let idx = ((version - 1) * 4) as usize + ECCLevel::M.capacity_idx();
        assert_eq!(idx, 1, "POINTER ARITHMETIC IS WRONG: 1-M is idx 1");
        let ec_bytes = EC_CODEWORDS_PER_BLOCK[idx] as usize;
        assert_eq!(ec_bytes, 10);

        let mut rs_encoder = ReedSolomon::new();
        let ec_codewords = rs_encoder.encode(&data_codewords, ec_bytes);

        // Confirm the EC codewords
        assert_eq!(ec_codewords, expect_ec_codewords);
    }

    // Test the data segmenting routine.
    #[test]
    fn test_data_codeword_segmentation() {
        // From: https://www.thonky.com/qr-code-tutorial/structure-final-message example
        let data: [u8; 62] = [
            67, 85, 70, 134, 87, 38, 85, 194, 119, 50, 6, 18, 6, 103, 38, // Group 1, block 1
            246, 246, 66, 7, 118, 134, 242, 7, 38, 86, 22, 198, 199, 146,
            6, // Group 1, block 2
            182, 230, 247, 119, 50, 7, 118, 134, 87, 38, 82, 6, 134, 151, 50,
            7, // Group 2, block 1
            70, 247, 118, 86, 194, 6, 151, 50, 224, 236, 17, 236, 17, 236, 17,
            236, // Group 2, block 2
        ];

        // Version 5, ECC level Q

        let segmentation = QrSegmentation::new(
            data.len(),
            ECCLevel::Q,
            // Version 5
            5,
        );

        // The group/block data is known, so this will just be hardcoded
        // -> it can be looked up by table, OR
        // -> derived from accessing the inner groups/
        // blocks in each group.
        //
        // Group: 1 Block: 1
        assert_eq!(
            segmentation.get_block(&data, 0, 0),
            &[
                67, 85, 70, 134, 87, 38, 85, 194, 119, 50, 6, 18, 6, 103,
                38, // Group 1, block 1
            ]
        );

        // Group: 1 Block: 2
        assert_eq!(
            segmentation.get_block(&data, 0, 1),
            &[
                246, 246, 66, 7, 118, 134, 242, 7, 38, 86, 22, 198, 199, 146,
                6, // Group 1, block 2
            ]
        );

        assert_eq!(
            segmentation.get_block(&data, 1, 0),
            &[
                182, 230, 247, 119, 50, 7, 118, 134, 87, 38, 82, 6, 134, 151, 50,
                7, // Group 2, block 1
            ]
        );

        assert_eq!(
            segmentation.get_block(&data, 1, 1),
            &[
                70, 247, 118, 86, 194, 6, 151, 50, 224, 236, 17, 236, 17, 236, 17,
                236, // Group 2, block 2
            ]
        );
    }

    #[test]
    fn test_ecc_on_blocks() {
        // This is known to segment properly per the above test.

        // From: https://www.thonky.com/qr-code-tutorial/structure-final-message example
        let data: [u8; 62] = [
            67, 85, 70, 134, 87, 38, 85, 194, 119, 50, 6, 18, 6, 103, 38, // Group 1, block 1
            246, 246, 66, 7, 118, 134, 242, 7, 38, 86, 22, 198, 199, 146,
            6, // Group 1, block 2
            182, 230, 247, 119, 50, 7, 118, 134, 87, 38, 82, 6, 134, 151, 50,
            7, // Group 2, block 1
            70, 247, 118, 86, 194, 6, 151, 50, 224, 236, 17, 236, 17, 236, 17,
            236, // Group 2, block 2
        ];

        // Version 5, ECC version Q
        // These are known and do not have to be worked out.
        let version = 5;
        let ecc_level = ECCLevel::Q;
        let table_idx = (version - 1) * 4 + ecc_level.capacity_idx();
        let ec_bytes = EC_CODEWORDS_PER_BLOCK[table_idx] as usize;
        assert_eq!(ec_bytes, 18, "EC byte discrepancy.");

        // Segment the data into blocks
        let segmentation = QrSegmentation::new(
            data.len(),
            ECCLevel::Q,
            // Version 5
            5,
        );
        // Flatten into a block vector.
        let blocks = segmentation.flatten_to_blocks();

        // Pass to the ECC computation
        let (ecc_bytes, ecc_blocks) = compute_ecc_codewords(&data, &blocks, ec_bytes);
        assert_eq!(ecc_blocks.len(), 4, "Invalid block computation.");

        // Examine the blocks.
        let expected_ecc_blocks: [[u8; 18]; 4] = [
            [
                213, 199, 11, 45, 115, 247, 241, 223, 229, 248, 154, 117, 154, 111, 86, 161, 111,
                39,
            ], // Block 1
            [
                87, 204, 96, 60, 202, 182, 124, 157, 200, 134, 27, 129, 209, 17, 163, 163, 120, 133,
            ], // Block 2
            [
                148, 116, 177, 212, 76, 133, 75, 242, 238, 76, 195, 230, 189, 10, 108, 240, 192,
                141,
            ], // Block 3
            [
                140, 100, 250, 247, 108, 131, 37, 104, 253, 113, 111, 235, 197, 83, 6, 205, 89, 74,
            ], // Block 4
        ];

        // Compare each of the blocks and make sure that they're equal
        for i in 0..ecc_blocks.len() {
            let ecc_block_data = ecc_blocks[i];
            let ecc_block = &ecc_bytes[ecc_block_data.start_idx()..=ecc_block_data.end_idx()];
            let expected_block = &expected_ecc_blocks[i];
            assert_eq!(ecc_block, expected_block, "Block {} doesn't align.", i + 1);
        }
    }

    // This is the full data-processing pipeline up to interleaving
    // Remainder bits will be tested later.
    #[test]
    fn test_prepare_qr_codewords() {
        // Alphanumeric, version 1, ecc level Q
        let data = "HELLO WORLD";
        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        let (processed, _, _) = prepare_qr_codewords(data, hints);

        let expected: [u8; 26] = [
            0x20, 0x5B, 0x0B, 0x78, 0xD1, 0x72, 0xDC, 0x4D, 0x43, 0x40, 0xEC, 0x11, 0xEC, 0xA8,
            0x48, 0x16, 0x52, 0xD9, 0x36, 0x9C, 0x00, 0x2E, 0x0F, 0xB4, 0x7A, 0x10,
        ];

        assert_eq!(processed.len(), 26, "Wrong size returned.");

        assert_eq!(
            &processed, &expected,
            "Error is likely within the interleaving."
        );

        // Alphanumeric, version 1, ecc level Q
        // String is: HELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLD

        let v7_test = data.repeat(11);
        let v7_version = get_min_required_version(v7_test.len(), 2, ECCLevel::Q);

        assert_eq!(v7_version, 7);
        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);

        let (processed, _, _) = prepare_qr_codewords(&v7_test, hints);

        let expected_v7: [u8; 196] = [
            0x23, 0x8A, 0x3A, 0x9D, 0xCE, 0xE7, 0xCB, 0x6E, 0x45, 0x22, 0x91, 0x48, 0x0B, 0xF9,
            0x37, 0x9B, 0x4D, 0xA6, 0x78, 0x85, 0x7C, 0xBE, 0xDF, 0xEF, 0xD1, 0xBC, 0xC2, 0x61,
            0x30, 0x98, 0x72, 0x68, 0xDE, 0x6F, 0xB7, 0x5B, 0xDC, 0xB9, 0x34, 0x1A, 0x8D, 0xC6,
            0x4D, 0x6E, 0x5C, 0x2E, 0x17, 0x8B, 0x44, 0x26, 0xB7, 0x5B, 0x2D, 0x96, 0xB4, 0xA2,
            0x13, 0x89, 0xC4, 0xE2, 0xA2, 0x5A, 0x51, 0xA8, 0xD4, 0x6A, 0xDE, 0x51, 0x2D, 0x96,
            0x4B, 0x1A, 0x4E, 0x6F, 0x28, 0x94, 0x4A, 0x00, 0x74, 0x27, 0xB7, 0x5B, 0x2D, 0xEC,
            0x93, 0xC9, 0xE4, 0x11, 0xE1, 0x57, 0x6D, 0x88, 0x18, 0x78, 0xEE, 0xF8, 0x55, 0x4C,
            0x6D, 0x09, 0xDF, 0xF9, 0xF3, 0x0B, 0x02, 0x32, 0xBD, 0xFA, 0x93, 0x44, 0x7D, 0xD8,
            0x1D, 0xED, 0x7C, 0x05, 0xC0, 0x30, 0x73, 0x35, 0x2B, 0x64, 0x0B, 0x2B, 0x07, 0xCC,
            0x1C, 0x0D, 0x57, 0x68, 0x65, 0x01, 0xEC, 0xCC, 0xA5, 0xAA, 0x26, 0x1F, 0x17, 0x88,
            0x3F, 0xBB, 0xE8, 0xA0, 0x1C, 0x6B, 0xBF, 0x45, 0x62, 0xCA, 0x1F, 0xC8, 0x46, 0xB8,
            0x3B, 0xFD, 0xDE, 0x09, 0x75, 0x5B, 0xB0, 0xFA, 0xCC, 0xA0, 0xDA, 0x63, 0x70, 0x5C,
            0x17, 0x02, 0x06, 0xD3, 0xBE, 0x0C, 0x39, 0x18, 0x07, 0x82, 0xDE, 0xA7, 0xE3, 0xA1,
            0xA3, 0x2B, 0x91, 0x90, 0x9C, 0x87, 0x32, 0xD4, 0xD8, 0xF0, 0x3C, 0xDF, 0xEF, 0x03,
        ];

        assert_eq!(processed, expected_v7, "Error with v7 interleaving");
    }

    // TODO: expect bugs to debug wrt rxing; I don't know the API, so more investigation is needed
    // to ensure a proper equality test.
    //
    // TODO TWICE: implement tests for each additional encoding mode implemented: byte, numeric,
    // kanji
    //
    // TODO THRICE: move these encode/rendering tests below the bit-emplacement unit tests.
    #[test]
    fn test_encode_qr() {
        // Alphanumeric, version 1, ecc level Q
        let data = "HELLO WORLD";

        // Note -> this doesn't emplace spaces between each "HELLO WORLD"
        // The v7_string should be:
        // HELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLD
        // -> test this with existing online encoders and compare by inspection until the full
        // implementation is complete and can be automated.
        let v7_test = data.repeat(11);
        let v7_version = get_min_required_version(v7_test.len(), 2, ECCLevel::Q);

        assert_eq!(v7_version, 7);

        // NOTE: THESE HAVE NOT BEEN RENDERED, SO THEY DO NOT HAVE A QUIET ZONE THAT NEEDS TO BE
        // CORRECTED FOR FOR THE COMPARISON.
        // THEY ARE ALSO NOT COMPLEMENTED, SO THIS CAN BE A 1:1 COMPARISON with RXING.

        // HOWEVER, RXING's IMPLEMENTATION -DOES- ADD A 4 MODULE QUIET ZONE AND THIS NEEDS TO BE
        // ACCOUNTED FOR IN THE COMPARISON LOOP.

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        // Per the selection algorithm, this is returning mask 6
        let encoded_v1 = encode_qr(data, Some(hints));
        let enc_v1_mat = encoded_v1.matrix();
        // Per the selection algorithm, this is returning mask 0
        let encoded_v7 = encode_qr(&v7_test, Some(hints));
        let enc_v7_mat = encoded_v7.matrix();

        // This is rxing's EncodeHints
        let qr_hints =
            EncodeHints::default().with(EncodeHintValue::ErrorCorrection("Q".to_string()));
        let n_v1 = enc_v1_mat.side_length();
        let n_v7 = enc_v7_mat.side_length();
        let qr_encoder = QRCodeWriter::default();
        let format = BarcodeFormat::QR_CODE;
        let expect_v1 = qr_encoder
            .encode_with_hints(data, &format, n_v1 as i32, n_v1 as i32, &qr_hints)
            .expect("V1 should encode and render correctly");

        let expect_v7 = qr_encoder
            .encode_with_hints(&v7_test, &format, n_v7 as i32, n_v7 as i32, &qr_hints)
            .expect("V7 should encode and render correctly");

        assert_eq!(
            expect_v1.width() as usize,
            n_v1 + 2 * QUIET_ZONE_SIZE,
            "V1 Size discrepancy width"
        );
        assert_eq!(
            expect_v1.height() as usize,
            n_v1 + 2 * QUIET_ZONE_SIZE,
            "V1 Size discrepancy height"
        );

        for i in 0..n_v1 {
            for j in 0..n_v1 {
                let (y, x) = ((i + QUIET_ZONE_SIZE) as u32, (j + QUIET_ZONE_SIZE) as u32);
                // 1 = true = black for both encodings.
                let lhs = enc_v1_mat.get(i, j).inner();
                // GET IS REVERSED IN RXING -> it's using x = horizontal = column
                let rhs = expect_v1.get(x as u32, y as u32);
                assert_eq!(
                    lhs, rhs,
                    "V1 Bit discrepancy at i: {i}, j: {j}, x: {x}, y: {y}"
                );
            }
        }

        // This will crash - > the semantics are different.
        assert_eq!(
            expect_v7.width() as usize,
            n_v7 + 2 * QUIET_ZONE_SIZE,
            "V7 Size discrepancy width"
        );
        assert_eq!(
            expect_v7.height() as usize,
            n_v7 + 2 * QUIET_ZONE_SIZE,
            "V7 Size discrepancy height"
        );

        for i in 0..n_v7 {
            for j in 0..n_v7 {
                let (y, x) = ((i + QUIET_ZONE_SIZE) as u32, (j + QUIET_ZONE_SIZE) as u32);
                // 1 = true = black for both encodings.
                let lhs = enc_v7_mat.get(i, j).inner();
                let rhs = expect_v7.get(x, y);
                assert_eq!(
                    lhs, rhs,
                    "V7 Bit discrepancy at i: {i}, j: {j} x: {x}, y: {y}"
                );
            }
        }
    }

    #[test]
    fn test_qr_render_to_bytes() {
        // Alphanumeric, version 1, ecc level Q
        let data = "HELLO WORLD";

        // Note -> this doesn't emplace spaces between each "HELLO WORLD"
        // The v7_string should be:
        // HELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLDHELLO WORLD
        // -> test this with existing online encoders and compare by inspection until the full
        // implementation is complete and can be automated.

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        let v7_test = data.repeat(11);
        let v7_version = get_min_required_version(v7_test.len(), 2, ECCLevel::Q);

        assert_eq!(v7_version, 7);

        // Per the selection algorithm, this is returning mask 6
        let encoded_v1 = encode_qr(data, Some(hints));
        // Per the selection algorithm, this is returning mask 0
        let encoded_v7 = encode_qr(&v7_test, Some(hints));

        // Grab the side lengths before render (and adding the quiet zone)
        // RXing's encoder appends the quiet zone to the final matrix, so this needs to be
        // corrected for.
        let n_v1 = encoded_v1.matrix().side_length();
        let n_v7 = encoded_v7.matrix().side_length();

        // These are in -pixel- values, so they're complement QR values.
        // They also add the quiet-zone size in their rendered size (like the rxing bitmatrices).
        let v1_bytes = encoded_v1.render();
        let v7_bytes = encoded_v7.render();

        // The default ECC level is L (I believe), so this needs to be passed in to the rxing encoding.
        let qr_hints =
            EncodeHints::default().with(EncodeHintValue::ErrorCorrection("Q".to_string()));
        let qr_encoder = QRCodeWriter::default();
        let format = BarcodeFormat::QR_CODE;
        let expect_v1 = qr_encoder
            .encode_with_hints(data, &format, n_v1 as i32, n_v1 as i32, &qr_hints)
            .expect("V1 should encode and render correctly");

        let expect_v7 = qr_encoder
            .encode_with_hints(&v7_test, &format, n_v7 as i32, n_v7 as i32, &qr_hints)
            .expect("V7 should encode and render correctly");

        let v1_bytes_side_len = v1_bytes.side_length() as u32;
        assert_eq!(v1_bytes_side_len, expect_v1.width(), "V1 Width discrepancy");
        assert_eq!(
            v1_bytes_side_len,
            expect_v1.height(),
            "V1 Height discrepancy"
        );

        // V1 BYTE CHECK -> my render performs the complement, so these need to be
        // complemented.
        // Rxing returns boolean, so one of the two halves need to be cast.
        // Both have a quiet zone, so no offset arithmetic is needed for the data comparison.

        for i in 0..v1_bytes_side_len as usize {
            for j in 0..v1_bytes_side_len as usize {
                let lhs = *v1_bytes.get(i, j);
                // Ensure it's only 1 bit.
                assert!((0u8..=1).contains(&lhs));
                let (x, y) = (j as u32, i as u32);
                // Complement and then cast from boolean to u8
                // RXING uses 1 = black = true until export
                let rhs = !expect_v1.get(x, y) as u8;
                assert_eq!(
                    lhs, rhs,
                    "V1 Bit discrepancy at i: {i}, j: {j}), x: {x}, y: {y}"
                );
            }
        }

        let v7_bytes_side_len = v7_bytes.side_length() as u32;

        assert_eq!(v7_bytes_side_len, expect_v7.width(), "V1 Width discrepancy");
        assert_eq!(
            v7_bytes_side_len,
            expect_v7.height(),
            "V1 Height discrepancy"
        );

        // V7 BYTE CHECK.
        for i in 0..v7_bytes_side_len as usize {
            for j in 0..v7_bytes_side_len as usize {
                let lhs = *v7_bytes.get(i, j);
                // Ensure it's only 1 bit.
                assert!((0u8..=1).contains(&lhs));
                let (x, y) = (j as u32, i as u32);
                // Complement and then cast from boolean to u8
                // RXING uses 1 = black = true until export
                let rhs = !expect_v7.get(x, y) as u8;
                assert_eq!(
                    lhs, rhs,
                    "V7 Bit discrepancy at i: {i}, j: {j}), x: {x}, y: {y}"
                );
            }
        }
    }

    // --- IMAGE EXPORTING ----

    // This tests the image resampling for export to png
    // If it completes, consider it "mostly correct."
    // The resampling is difficult to automate; determine pixel discrepancies by examining
    // the output in CARGO_MANIFEST_DIR/test_images/
    #[cfg(feature = "png")]
    #[test]
    fn test_qr_render_png_with_resample() {
        // Alphanumeric, version 1, ecc level Q
        let data = "HELLO WORLD";

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        // Per the selection algorithm, this is returning mask 6
        let encoded_v1 = encode_qr(data, Some(hints));

        let rendered = encoded_v1.render();

        // Make the file path
        let crate_dir = env!("CARGO_MANIFEST_DIR");

        let img_dir = Path::new(crate_dir).join(IMG_DIRECTORY_SLUG);

        let img_1x = img_dir.join("hello_world_1x.png");
        let side_len = rendered.side_length();
        // Test 1x (image export)
        let res_1 = save_png(&img_1x, &rendered, None);

        assert!(
            res_1.is_ok(),
            "Failed to write image correctly at 1x scale. Error: {:?}.",
            res_1.err()
        );

        // Test 10x (resize) -> Expect this to be 210 x 210 px in length.
        let img_10x = img_dir.join("hello_world_10x.png");
        let res_2 = save_png(&img_10x, &rendered, Some(side_len * 10));

        assert!(
            res_2.is_ok(),
            "Failed to write image correctly at 2x scale. Error: {:?}.",
            res_2.err()
        );

        // Test 10.5x (resize with fractional scaling) -> expect this to be 220 px in length.
        // -- not encouraged, but may still produce a working QR code
        // -- ideally scaling should be done with svg
        // -- and better done with proper interpolation from the image crate.
        // This resampling only does a basic linear interpolation using normalized coordinates.
        let img_10p5x = img_dir.join("hello_world_10p5x.png");
        let fract_side_length = (side_len as f32 * 10.5).floor() as usize;

        let res_3 = {
            let (png, _is_fract) =
                resize_and_render_image_exact(&rendered, Some(fract_side_length));
            write_png(&img_10p5x, &png)
        };

        assert!(
            res_3.is_ok(),
            "Failed to write image correctly at 1.5x scale. Error: {:?}.",
            res_3.err()
        );

        // Test 20.75 with ratio preservation (integer scaling)
        let img_20p75x = img_dir.join("hello_world_21x.png");
        // This should get rounded up to 11.
        // This will be (33 * 21 = 693)px * 693 px in size
        let fract_side_length = (side_len as f32 * 20.75).floor() as usize;
        let res_4 = save_png(&img_20p75x, &rendered, Some(fract_side_length));
        assert!(
            res_4.is_ok(),
            "Failed to write image correctly at 20.75x -> 21x scale. Error: {:?}",
            res_4.err()
        );
    }

    #[cfg(any(feature = "svg", feature = "png"))]
    fn test_nearest_integer() {
        let old_len = 21 as usize;
        let new_len_greater = (21f32 * 10.50).floor() as usize;

        let nearest_pos = nearest_integer_multiple(old_len, new_len_greater);

        if let IntegerInverse::Multiply(nearest_pos) = nearest_pos {
            assert_eq!(nearest_pos, 11, "Calculation is off in nearest positive.");
        } else {
            panic!("nearest integer returning divide instead of multiply.");
        }

        let old_len = 210;
        let new_len = 21;
        let nearest_neg = nearest_integer_multiple(old_len, new_len);

        if let IntegerInverse::Divide(nearest_neg) = nearest_neg {
            assert_eq!(nearest_neg, 10, "Calculation is off in nearest, divide.");
        } else {
            panic!("nearest integer returning multiply instead of divide.");
        }

        let old_len = 21;
        let new_len = (old_len as f32 * 10.5).floor() as usize;

        let nearest_fract_pos = nearest_integer_multiple(old_len, new_len);

        if let IntegerInverse::Multiply(nearest_fract_pos) = nearest_fract_pos {
            assert_eq!(
                nearest_fract_pos, 11,
                "Nearest integer calculation is off in nearest fractional multiply."
            );
        } else {
            panic!("nearest integer fract returning divide instead of multiply.");
        }

        let old_len = 210;
        let new_len = (0.75 * old_len as f32).floor() as usize;
        let expect_len = 105;

        let nearest_fract_neg = nearest_integer_multiple(old_len, new_len);

        if let IntegerInverse::Divide(nearest_fract_neg) = nearest_fract_neg {
            assert_eq!(
                nearest_fract_neg, 2,
                "Nearest integer calculation is off in nearest fractional divide."
            );

            let test_len = old_len / nearest_fract_neg;
            assert_eq!(
                test_len, expect_len,
                "Nearest integer returning wrong magnitude"
            );
        } else {
            panic!("nearest integer returning multiply instead of divide.");
        }
    }

    // This tests svg export + exporting svg after running with basic linear resampling.
    #[cfg(feature = "svg")]
    #[test]
    fn test_qr_render_svg_with_resample() {
        // Alphanumeric, version 1, ecc level Q
        let data = "HELLO WORLD";

        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        // Per the selection algorithm, this is returning mask 6
        let encoded_v1 = encode_qr(data, Some(hints));

        let rendered = encoded_v1.render();
        let side_length = rendered.side_length();

        // Make the file path
        let crate_dir = env!("CARGO_MANIFEST_DIR");

        let img_dir = Path::new(crate_dir).join(IMG_DIRECTORY_SLUG);

        // Run an svg export with zero resampling
        let img_1x = img_dir.join("hello_word_1x.svg");
        let res_1 = save_svg(&img_1x, &rendered, None, None);
        assert!(
            res_1.is_ok(),
            "Failed to write image correctly at 1x scale. Error: {:?}",
            res_1.err()
        );

        // Run an svg export with 2x scaling without resampling
        let img_10x_no_resample = img_dir.join("hello_world_10x_no_resample.svg");
        let res_2 = save_svg(
            &img_10x_no_resample,
            &rendered,
            Some(side_length * 10),
            None,
        );
        assert!(
            res_2.is_ok(),
            "Failed to write image correctly at 1x scale. Error: {:?}",
            res_2.err()
        );

        // Run an svg export with 10x scaling with resampling
        let img_10x_with_resample = img_dir.join("hello_world_10x_with_resample.svg");
        let svg = render_svg_with_resampling(&rendered, Some(side_length * 10), None);
        let res_3 = write_svg(&img_10x_with_resample, &svg);

        assert!(
            res_3.is_ok(),
            "Failed to write image correctly at 1x scale. Error: {:?}",
            res_3.err()
        );
    }

    #[cfg(feature = "svg")]
    #[test]
    fn test_qr_render_svg_with_hints() {
        let data = "HELLO WORLD";
        let mut hints = EncodingHints::default();
        hints.ecc_level = Some(ECCLevel::Q);
        let encoded = encode_qr(data, Some(hints));

        let render = encoded.render();

        // Make the file path
        let crate_dir = env!("CARGO_MANIFEST_DIR");

        let img_dir = Path::new(crate_dir).join(IMG_DIRECTORY_SLUG);

        // Run an svg export with zero resampling
        let img_crisp = img_dir.join("hello_word_crisp_edges.svg");

        let img_hints = img_dir.join("hello_world_squircle.svg");

        // Test turning off subpixel rendering/antialiasing
        let mut hints = SvgHints::default();
        hints.shape_rendering = Some("crispEdges");

        let side_length = Some(210);
        let res_1 = save_svg(&img_crisp, &render, side_length, Some(hints));
        assert!(
            res_1.is_ok(),
            "Failed to write image with no antialiasing. Error: {:?}",
            res_1.err()
        );

        // Test corner radii for rounded-corner QR pixels.

        let mut hints = SvgHints::default();
        hints.shape_rendering = None;

        let mut pixel_hints = SvgRectHints::default();
        let corner_radius = "2";
        pixel_hints.rx = Some(corner_radius);
        pixel_hints.ry = Some(corner_radius);
        hints.pixel_hints = Some(pixel_hints);

        let res_2 = save_svg(&img_hints, &render, side_length, Some(hints));

        assert!(
            res_2.is_ok(),
            "Failed to write image with hints. Error: {:?}",
            res_2.err()
        );
    }

    #[test]
    fn test_emplace_finder_patterns() {
        // Version 1 has a 21 x 21 square matrix
        let version = 1;
        let side_length = (version - 1) * 4 + 21;
        assert_eq!(side_length, 21);
        // Construct a square matrix
        let mut sq_matrix = SquareMatrix::new(side_length);
        assert_eq!(sq_matrix.side_length(), 21);

        emplace_finder_patterns_into_blank_matrix(&mut sq_matrix, version);
        let (mat, _side_length) = sq_matrix.destructure_into_bytes();
        let expect: [u8; 441] = [
            //                   //                   //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, //
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, //
            // --
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            //--
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
        ];

        for i in 0..side_length {
            for j in 0..side_length {
                let idx = i * side_length + j;
                assert_eq!(mat[idx], expect[idx], "Mismatch at i: {i}, j: {j}");
            }
        }
    }

    #[test]
    fn test_emplace_timing_and_finder_patterns() {
        // Version 1 has a 21 x 21 square matrix
        let version = 1;
        let side_length = (version - 1) * 4 + 21;
        assert_eq!(side_length, 21);
        // Construct a square matrix
        let mut sq_matrix = SquareMatrix::new(side_length);
        assert_eq!(sq_matrix.side_length(), 21);

        emplace_timing_patterns(&mut sq_matrix);
        emplace_finder_patterns_into_blank_matrix(&mut sq_matrix, version);
        let (mat, _side_length) = sq_matrix.destructure_into_bytes();
        let expect: [u8; 441] = [
            // x = timing-pattern column/row.
            //                x //                   //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, // x
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, //
            // --
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            //--
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, //
        ];

        for i in 0..side_length {
            for j in 0..side_length {
                let idx = i * side_length + j;

                assert_eq!(mat[idx], expect[idx], "Mismatch at i: {i}, j: {j}");
            }
        }
    }

    #[test]
    fn test_alignment_centers_single_pointer() {
        // Version 2
        let t1 = ALIGNMENT_BLOCK_CENTERS[1];
        let t2 = ALIGNMENT_BLOCK_CENTERS[1];

        let i1 = t1.inner();
        let i2 = t2.inner();
        assert!(std::ptr::eq(i1, i2));
    }

    #[test]
    fn test_emplace_timing_finder_alignment_patterns() {
        // Version 2 has a 21 x 21 square matrix
        let version = 2;
        let side_length = (version - 1) * 4 + 21;
        assert_eq!(side_length, 25);
        // Construct a square matrix
        let mut sq_matrix = SquareMatrix::new(side_length);
        assert_eq!(sq_matrix.side_length(), 25);

        emplace_timing_patterns(&mut sq_matrix);
        emplace_finder_patterns_into_blank_matrix(&mut sq_matrix, version);
        emplace_alignment_squares(&mut sq_matrix, version);
        let (mat, _side_length) = sq_matrix.destructure_into_bytes();
        let expect: [u8; 25 * 25] = [
            // x = timing-pattern column/row.

            // Finder ------- x --                             ----------------------
            // 1  2  3  4  5 |6| 7  8  9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, // 0
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, // 1
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, // 2
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, // 3
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, // 4
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1, // 5
            //               | | Timing Column
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, // 6 x
            //               | |
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, // 7
            // Finder ------------                      Finder ----------------------
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 8
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 9
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 10
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 11
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 12
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 13
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 14
            1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 15
            //                          // -- Alignment    |-  -  -  -  -|
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 16
            // Finder -------------|
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, // 17
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, // 18
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, // 19
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 20
            //                          // -- Alignment    |-  -  -  -  -|
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 21
            1, 0, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 22
            1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 23
            1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 24
        ];
        //  // Finder ------------|

        for i in 0..side_length {
            for j in 0..side_length {
                let idx = i * side_length + j;

                assert_eq!(mat[idx], expect[idx], "Mismatch at i: {i}, j: {j}");
            }
        }
    }

    // -------------------------FURTHER ENCODING TESTS.
    // For each of these, encode QR using rxing without any hints and let it select the "optimal"
    // to tease out any issues with my selection implementations.
    #[test]
    fn test_qr_render_to_bytes_byte_mode() {
        // ZXing/rxing's default error correction level is L
        let test_string = "https://www.github.com/jordan-clayton/qr_writer";
        run_render_test_ecc_l(test_string);
    }

    #[test]
    fn test_qr_render_to_bytes_numeric_mode() {
        // ZXing/rxing's default error correction level is L
        // Taken from: https://www.thonky.com/qr-code-tutorial/numeric-mode-encoding
        let test_string = "8675309";
        run_render_test_ecc_l(test_string);
    }

    // TODO: this needs to provide kanji as an encoding hint.
    #[cfg(feature = "kanji")]
    #[test]
    fn test_qr_render_to_bytes_kanji_mode() {
        use encoding_rs::SHIFT_JIS;
        // "Lol"
        let data = "草";
        // "Lolololololololololol"
        let test_string = data.repeat(9);

        // Make sure the code -does- encode properly using Shift JIS
        let (_encode_string, enc, errors) = SHIFT_JIS.encode(&test_string);
        assert!(!errors, "String expected to encode without issue.");
        assert!(
            enc == SHIFT_JIS,
            "String expected to encode properly to JIS."
        );

        let encode_hints =
            EncodeHints::default().with(EncodeHintValue::CharacterSet("Shift_JIS".to_string()));

        run_render_test_ecc_l_with_encode_hints(&test_string, &encode_hints);
    }

    #[test]
    fn test_encoding_hints() {
        let hints = EncodingHints {
            mask: Some(1),
            version: Some(NonZero::new(40).unwrap()),
            ecc_level: Some(ECCLevel::Q),
        };

        let data = "HELLO WORLD";

        let encoded = encode_qr(&data, Some(hints));
        let side_length = encoded.side_length();
        let rendered = encoded.render();

        let rxing_hints = EncodeHints::default()
            .with(EncodeHintValue::ErrorCorrection("Q".to_string()))
            .with(EncodeHintValue::QrMaskPattern("1".to_string()))
            .with(EncodeHintValue::QrVersion("40".to_string()));
        let rxing_format = BarcodeFormat::QR_CODE;
        let rxing_encoder = QRCodeWriter::default();
        let qr_encoder = QRCodeWriter::default();

        let expect = qr_encoder
            .encode_with_hints(
                &data,
                &rxing_format,
                side_length as i32,
                side_length as i32,
                &rxing_hints,
            )
            .expect("RXING Matrix should encode without issue.");

        let test_side_len = rendered.side_length() as u32;

        assert_eq!(
            test_side_len,
            expect.width(),
            "QR with explicit hints width discrepancy with rxing"
        );
        assert_eq!(
            test_side_len,
            expect.height(),
            "QR with explicit hints height discrepancy with rxing"
        );

        // Byte check
        for i in 0..test_side_len as usize {
            for j in 0..test_side_len as usize {
                let lhs = *rendered.get(i, j);
                assert!((0u8..=1).contains(&lhs));
                let (x, y) = (j as u32, i as u32);
                // RXING stores the complement until image export
                let rhs = !expect.get(x, y) as u8;
                assert_eq!(
                    lhs, rhs,
                    "v40 (hints test) Bit discrepancy at i: {i}, j:{j}, x: {x}, y: {y}"
                );
            }
        }
    }

    // TODO: mild refactor -> also allow provided ECC level (my enum).
    // The ecc levels should match before sending the hints to rxing.
    // This function should be modified once hints have been implemented in -this- api.
    fn run_render_test_ecc_l_with_encode_hints(test_string: &str, encode_hints: &EncodeHints) {
        let test_qr = encode_qr(test_string, None);
        let test_n = test_qr.matrix().side_length();
        let test_render = test_qr.render();

        let rx_encoder = QRCodeWriter;
        let format = BarcodeFormat::QR_CODE;
        let expect_render = rx_encoder
            .encode_with_hints(
                test_string,
                &format,
                test_n as i32,
                test_n as i32,
                encode_hints,
            )
            .expect("Rxing encoder should encode without issue.");

        // This includes the quiet zone
        let render_side_length = test_render.side_length() as u32;
        assert_eq!(
            render_side_length,
            expect_render.height(),
            "Height mismatch."
        );
        assert_eq!(render_side_length, expect_render.width(), "Width mismatch.");

        // Byte comparison -> the rx matrix will be complement mine.
        for i in 0..render_side_length as usize {
            for j in 0..render_side_length as usize {
                let lhs = *test_render.get(i, j);
                let rhs = !expect_render.get(j as u32, i as u32) as u8;
                assert_eq!(lhs, rhs, "Bit mismatch at i: {i}, j: {j}");
            }
        }
    }

    fn run_render_test_ecc_l(test_string: &str) {
        let test_qr = encode_qr(test_string, None);
        let test_n = test_qr.matrix().side_length();
        let test_render = test_qr.render();

        let rx_encoder = QRCodeWriter;
        let format = BarcodeFormat::QR_CODE;
        let expect_render = rx_encoder
            .encode(test_string, &format, test_n as i32, test_n as i32)
            .expect("Rxing encoder should encode without issue.");

        // This includes the quiet zone
        let render_side_length = test_render.side_length() as u32;
        assert_eq!(
            render_side_length,
            expect_render.height(),
            "Height mismatch."
        );
        assert_eq!(render_side_length, expect_render.width(), "Width mismatch.");

        // Byte comparison -> the rx matrix will be complement mine.
        for i in 0..render_side_length as usize {
            for j in 0..render_side_length as usize {
                let lhs = *test_render.get(i, j);
                let rhs = !expect_render.get(j as u32, i as u32) as u8;
                assert_eq!(lhs, rhs, "Bit mismatch at i: {i}, j: {j}");
            }
        }
    }

    // Pick an arbitrarily large (but within the maximum limit) string of bytes for byte-mode
    // encoding.
    // The maximum byte-mode ECC L string is 2953 characters long.
    #[test]
    fn fuzz_test_qr() {
        // Generic lipsum text.
        // This is a kung-fu copy-paste job, and the string should be truncated into the maximum
        // number of characters.
        let lipsum = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor\n\n\
            incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis\n\n\
            nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\n\
            Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu\n\n\
            fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in\n\n\
            culpa qui officia deserunt mollit anim id est laborum.\n\n\
            Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium\n\n\
            doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore\n\n\
            veritatis et quasi architecto beatae vitae dicta sunt explicabo. Nemo enim\n\n\
            ipsam voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia\n\n\
            consequuntur magni dolores eos qui ratione voluptatem sequi nesciunt. Neque\n\n\
            porro quisquam est, qui dolorem ipsum quia dolor sit amet, consectetur,\n\n\
            adipisci velit, sed quia non numquam eius modi tempora incidunt ut labore et\n\n\
            dolore magnam aliquam quaerat voluptatem.\n\n\
            Ut enim ad minima veniam, quis nostrum exercitationem ullam corporis suscipit\n\n\
            laboriosam, nisi ut aliquid ex ea commodi consequatur. Quis autem vel eum iure\n\n\
            reprehenderit qui in ea voluptate velit esse quam nihil molestiae consequatur,\n\n\
            vel illum qui dolorem eum fugiat quo voluptas nulla pariatur. At vero eos et\n\n\
            accusamus et iusto odio dignissimos ducimus qui blanditiis praesentium\n\n\
            voluptatum deleniti atque corrupti quos dolores et quas molestias excepturi\n\n\
            sint occaecati cupiditate non provident.\n\n\
            Similique sunt in culpa qui officia deserunt mollitia animi, id est laborum et\n\n\
            dolorum fuga. Et harum quidem rerum facilis est et expedita distinctio. Nam\n\n\
            libero tempore, cum soluta nobis est eligendi optio cumque nihil impedit quo\n\n\
            minus id quod maxime placeat facere possimus, omnis voluptas assumenda est,\n\n\
            omnis dolor repellendus. Temporibus autem quibusdam et aut officiis debitis aut\n\n\
            rerum necessitatibus saepe eveniet ut et voluptates repudiandae sint et\n\n\
            molestiae non recusandae.\n\n\
            Itaque earum rerum hic tenetur a sapiente delectus, ut aut reiciendis\n\n\
            voluptatibus maiores alias consequatur aut perferendis doloribus asperiores\n\n\
            repellat. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do\n\n\
            eiusmod tempor incididunt ut labore et dolore magna aliqua. Proin nibh nisl\n\n\
            condimentum id venenatis a condimentum vitae. Amet justo donec enim diam\n\n\
            vulputate ut pharetra sit. Nunc id cursus metus aliquam eleifend mi in nulla.\n\n\
            Tincidunt id aliquet risus feugiat in ante metus dictum at. Felis eget nunc lobortis\n\n\
            mattis aliquam faucibus purus in massa. Pretium lectus quam id leo in vitae turpis massa sed.\n\n\
            Tincidunt id aliquet risus feugiat in ante metus dictum at. Felis eget nunc lobortis\n\n\
            mattis aliquam faucibus purus in massa. Pretium lectus quam id leo in vitae turpis massa sed.\n\n\
            Tincidunt id aliquet risus feugiat in ante metus dictum at. Felis eget nunc lobortis\n\n\
            mattis aliquam faucibus purus in massa. Pretium lectus quam id leo in vitae turpis massa sed.
            ";

        let diff = (2953 as i32 - lipsum.len() as i32).abs() as usize;
        let lipref = &lipsum[0..lipsum.len() - diff];
        assert_eq!(lipref.len(), 2953);

        // This string falls within the boundaries and -should not panic-
        run_render_test_ecc_l(lipref);

        // This string falls outside boundaries (too long) and should panic RIGHT NOW
        // until cleanup.

        let res = std::panic::catch_unwind(|| {
            run_render_test_ecc_l(lipsum);
        });

        assert!(
            res.is_err(),
            "Encoding succeded with invalid version level."
        );
    }
}
