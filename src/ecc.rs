// Most of this can probably all be folded into lib.rs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ECCLevel {
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
    pub fn capacity_idx(&self) -> usize {
        match self {
            Self::L => 0,
            Self::M => 1,
            Self::Q => 2,
            Self::H => 3,
        }
    }
}
