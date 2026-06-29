use crate::galois::{
    EXP_TABLE, GENERATOR_BASE, GaloisPolynomial, gf_poly_divide, gf_poly_mul,
    gf_poly_multiply_by_monomial, gf_poly_one,
};

use crate::errors::{ArithmeticError, QrError, Result};

#[repr(transparent)]
pub(crate) struct ReedSolomon(Vec<GaloisPolynomial>);
impl ReedSolomon {
    pub(crate) fn new() -> Self {
        let polys = vec![gf_poly_one()];
        Self(polys)
    }

    // Per: https://www.thonky.com/qr-code-tutorial/how-create-generator-polynomial
    // Each step is a multiplication of the current polynomial by (a_0 x + a_j),
    // where j is 0, 1, 2, 3, ... for degree: 1, 2, 3, 4...
    // j = (cur_degree - 1) + b = generator_base, for QR = 0
    // eg. For a total degree 2:
    // step/deg 1 => (1)(1x + 2^{1 - 1 + 0 = 0})
    // step/deg 2 => (1x + 2^0)(1x + 2^{2 - 1 + 0 = 1})
    // ... and so on.
    pub(crate) fn build_generator(&mut self, degree: usize) -> Result<&GaloisPolynomial> {
        if degree >= self.0.len() {
            let mut prev = self
                .0
                .last()
                .ok_or(QrError::ArithmeticError(ArithmeticError::EmptyPolynomial))?;

            for deg in self.0.len()..=degree {
                let expt = EXP_TABLE[deg - 1 + GENERATOR_BASE];
                let mult = GaloisPolynomial::new(&[1, expt as u8])?;
                let next_generator = gf_poly_mul(prev, &mult)?;
                self.0.push(next_generator);
                prev = &self.0[deg];
            }
        }

        Ok(&self.0[degree])
    }

    // Per: https://www.thonky.com/qr-codekk-tutorial/error-correction-coding#step-8-generating-error-correction-codewords
    // Args:
    // - data -> the block of (only) codewords
    // - ec_bytes -> the number of ec_bytes in the block.
    //
    // Returns: the EC block as Vec<u8>
    pub(crate) fn encode(&mut self, data: &[u8], ec_bytes: usize) -> Result<Vec<u8>> {
        // Create the generator polynomial
        let generator = self.build_generator(ec_bytes)?;
        // Clone the data into coefficients -> GaloisPolynomial has to clone in its constructor.
        let mut message = GaloisPolynomial::new(data)?;

        // This ensures the leading exponent of the lead term doesn't get too small
        // during the repeated division:
        // m(x) * x^n where n = the number of ec bytes.
        message = gf_poly_multiply_by_monomial(&message, ec_bytes, 1)?;

        // Repeated division: number of divisions = number of terms in m(x) -> handled in galois.rs
        // The remainder is to have exactly ec_bytes worth of room:
        // eg. if ec_bytes = 5, remainder has k <= 5 terms.
        // Internally, leading zeroes are discarded, so they need to be put back in the codeword
        // vector.
        let (_quot, remainder) = gf_poly_divide(&message, generator)?;

        let coefficients = remainder.coefficients();
        let num_zeros = ec_bytes - coefficients.len();

        // Allocate a codeword vector
        let mut code = vec![0; ec_bytes];
        // Copy over the coefficients
        code[num_zeros..].copy_from_slice(coefficients);
        // Return the codeword vector.
        Ok(code)
    }
}
