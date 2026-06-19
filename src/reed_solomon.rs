use crate::galois::{GaloisPolynomial, gf_poly_one};

#[repr(transparent)]
pub(crate) struct ReedSolomon(Vec<GaloisPolynomial>);
impl ReedSolomon {
    pub(crate) fn new() -> Self {
        let polys = vec![gf_poly_one()];
        Self(polys)
    }

    pub(crate) fn build_generator(&mut self) -> GaloisPolynomial {
        todo!("Implement building the generator polynomial.");
    }

    // Args:
    // - data -> the block of codewords
    // - ec_bytes -> the number of ec_bytes in the block.
    //
    // Returns: the EC block as Vec<u8>
    pub(crate) fn encode(&self, data: &[u8], ec_bytes: usize) -> Vec<u8> {
        todo!("Implement RS encoding.");
    }
}
