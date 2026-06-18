mod ecc;
mod encoding;
mod qr;
mod tables;
mod versioning;

#[cfg(test)]
mod tests {
    use crate::ecc::ECCLevel;
    use crate::encoding::get_data_encoding_mode;
    use crate::qr::encode_data_to_bytes;
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

        let res = encode_data_to_bytes(data, ECCLevel::M);
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
        let res = encode_data_to_bytes(data, ECCLevel::Q);
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

        let res = encode_data_to_bytes(data, ECCLevel::Q);
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

        // Assert version 1.
        let version = get_min_required_version(data.len(), mode, ECCLevel::H);

        // EC level H -> 9 codepoints.
        let res = encode_data_to_bytes(data, ECCLevel::H);
        assert_eq!(res, expect);
    }
}
