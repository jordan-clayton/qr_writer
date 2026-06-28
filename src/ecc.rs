// TODO: fold this enum into another module -> likely encoding

#[derive(Default, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub enum ECCLevel {
    #[default]
    L,
    M,
    Q,
    H,
}

impl ECCLevel {
    pub fn capability(&self) -> f32 {
        match self {
            Self::L => 0.07,
            Self::M => 0.15,
            Self::Q => 0.25,
            Self::H => 0.30,
        }
    }

    // TODO: this needs a clearer name like ecc_idx
    pub fn capacity_idx(&self) -> usize {
        match self {
            Self::L => 0,
            Self::M => 1,
            Self::Q => 2,
            Self::H => 3,
        }
    }

    // These bits specify the error correction level used for the format bitstring.
    pub fn ecc_bits_for_format_string(&self) -> u8 {
        match self {
            Self::L => 1,
            Self::M => 0,
            Self::Q => 3,
            Self::H => 2,
        }
    }
}
