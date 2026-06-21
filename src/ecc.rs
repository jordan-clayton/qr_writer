// Most of this can probably all be folded into lib.rs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ECCLevel {
    L,
    M,
    Q,
    H,
}

impl ECCLevel {
    pub(crate) fn capability(&self) -> f32 {
        match self {
            Self::L => 0.07,
            Self::M => 0.15,
            Self::Q => 0.25,
            Self::H => 0.30,
        }
    }

    // TODO: this needs a clearer name like ecc_idx
    pub(crate) fn capacity_idx(&self) -> usize {
        match self {
            Self::L => 0,
            Self::M => 1,
            Self::Q => 2,
            Self::H => 3,
        }
    }

    // These bits specify the error correction level used for the format bitstring.
    pub(crate) fn ecc_bits_for_format_string(&self) -> u8 {
        match self {
            Self::L => 1,
            Self::M => 0,
            Self::Q => 3,
            Self::H => 2,
        }
    }
}
