// Galois compile-time tables are computed recursively, so the stack limit needs to be doubled.
#![recursion_limit = "512"]
mod ecc;
mod encoding;
mod galois;
mod qr;
mod reed_solomon;
mod tables;
mod versioning;

// TODO: consider moving the galois tests/other module tests to their respective modules (if/where
// possible) and reduce visibility where sensible.
//
// TODO: more test cases can (and should) be generated using:
// https://www.nayuki.io/page/creating-a-qr-code-step-by-step
#[cfg(test)]
mod tests {
    use crate::ecc::ECCLevel;
    use crate::encoding::get_data_encoding_mode;
    use crate::galois::{
        EXP_TABLE, FIELD_SIZE, GaloisPolynomial, IRR_POLY, LOG_TABLE, REM, gf_exp, gf_inverse,
        gf_multiply, gf_poly_add, gf_poly_divide, gf_poly_mul, gf_poly_multiply_by_monomial,
        gf_poly_zero,
    };
    use crate::qr::{
        QrSegmentation, compute_ecc_codewords, encode_data_to_bytes, prepare_qr_codewords,
    };
    use crate::reed_solomon::ReedSolomon;
    use crate::tables::EC_CODEWORDS_PER_BLOCK;
    use crate::versioning::get_min_required_version;

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

        let (res, _, _) = encode_data_to_bytes(data, ECCLevel::M);
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

        // This should be a V1-Q code
        let (res, _, _) = encode_data_to_bytes(data, ECCLevel::Q);
        assert_eq!(res, expect);
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

        let (res, _, _) = encode_data_to_bytes(data, ECCLevel::Q);
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

        // EC level H -> 9 codepoints.
        let (res, _, _) = encode_data_to_bytes(data, ECCLevel::H);
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
        let (data_codewords, _, _) = encode_data_to_bytes(data, ECCLevel::M);
        assert_eq!(data_codewords, expect_data_codewords);

        // Encode the ec bytes.
        let idx = ((version - 1) * 4) as usize + ECCLevel::M.capacity_idx() * 4;
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

        let processed = prepare_qr_codewords(data, ECCLevel::Q);

        let expected: [u8; 26] = [
            0x20, 0x5B, 0x0B, 0x78, 0xD1, 0x72, 0xDC, 0x4D, 0x43, 0x40, 0xEC, 0x11, 0xEC, 0xA8,
            0x48, 0x16, 0x52, 0xD9, 0x36, 0x9C, 0x00, 0x2E, 0x0F, 0xB4, 0x7A, 0x10,
        ];

        assert_eq!(processed.len(), 26, "Wrong size returned.");

        assert_eq!(
            &processed, &expected,
            "Error is likely within the interleaving."
        );
    }
}
