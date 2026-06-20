use itertools::Itertools;
// TODO: Once testing is done, refactor the panicking out of the API.
//
//
// Most of this implementation has been adapted from both, as I have little knowledge in Galois
// Theory:
// - ZXing: https://github.com/zxing/zxing/blob/master/core/src/main/java/com/google/zxing/common/reedsolomon/GenericGF.java
// - RXing: https://github.com/rxing-core/rxing/blob/main/src/common/reedsolomon/generic_gf.rs
//
// References:
// - https://stackoverflow.com/questions/8440654/addition-and-multiplication-in-a-galois-field
// - https://www.thonky.com/qr-code-tutorial/error-correction-coding
//
// NOTE: this implementation is only for GF(2^8).
// The irreducible polynomial: 0b100011101, 0x11D, 0d285
// -> each bit is a power of x => x^8 + x^4 + x^3 + x^2 + 1
// -> The multiplication tables are reduced modulo the irreducible polynomial
pub(crate) const FIELD_SIZE: usize = 256;
pub(crate) const IRR_POLY: usize = 0x11D;
pub(crate) const REM: usize = FIELD_SIZE - 1;

// for the generator polynomial
// 2t = degree of the polynomial
// b = starting integer constant
// t = max number of correctable errors
// "x" is the polynomial variable.
// g(x) = (x + a^b)(x + a^(b + 1))...(x + a^(b + 2t - 1)))
pub(crate) const GENERATOR_BASE: usize = 0;
const fn compute_gf_256_log_tables() -> ([usize; FIELD_SIZE], [usize; FIELD_SIZE]) {
    let mut exp = [0usize; FIELD_SIZE];
    let mut log = [0usize; FIELD_SIZE];

    fill_exp_rec(&mut exp, 1, 0);
    fill_log_rec(&mut log, &exp, 0);

    // Log[0] is undefined. It's pre-initialized to 0 and should never be touched in the actual
    // computation.
    if log[0] != 0 {
        panic!("INITIALIZATION FAILURE: 0-ASSUMPTION IS INCORRECT.");
    }

    (exp, log)
}

// Loops and iterators aren't stable yet in constant evaluation.
// 256 is a small enough size that the recursion should be fine if the limit is bumped.
// See: lib.rs; recursion limit is currently set to 512.
const fn fill_exp_rec(exp: &mut [usize; FIELD_SIZE], mut x: usize, i: usize) {
    if i >= FIELD_SIZE {
        return;
    }
    exp[i] = x;
    x *= 2;

    if x >= 256 {
        x ^= IRR_POLY;
        x &= REM;
    }

    fill_exp_rec(exp, x, i + 1);
}

const fn fill_log_rec(log: &mut [usize; FIELD_SIZE], exp: &[usize; FIELD_SIZE], i: usize) {
    if i >= REM {
        return;
    }

    log[exp[i]] = i;
    fill_log_rec(log, exp, i + 1);
}

// It would be nice if tuple-unpacking could happen at compile time, hopefully one day :<
pub(crate) const LOG_TABLES: ([usize; 256], [usize; 256]) = compute_gf_256_log_tables();
pub(crate) const EXP_TABLE: [usize; 256] = LOG_TABLES.0;
pub(crate) const LOG_TABLE: [usize; 256] = LOG_TABLES.1;

// ----------POLYNOMIAL OPERATIONS-------------
// For the moment it's unnecessary to represent a galois polynomial as more than just a vector of
// its coefficients--at which point it is more sensible to just use rxing.
// The transparent tuple-struct is just to avoid getting mixed up with Vec<u8>
#[derive(Clone, Debug, PartialEq)]
#[repr(transparent)]
pub(crate) struct GaloisPolynomial(Vec<u8>);

// TODO: error correction
impl GaloisPolynomial {
    // Coefficients: a list of u8, where u8 are elements of GF(2^8),
    // Arranged highest order coefficient to lowest order coefficient.
    // i.e. m(x) = a_{n-1} x^{n-1} + a_{n-2} x^{n-2} + ... + a_{1} x + a_{0}
    pub(crate) fn new(coefficients: &[u8]) -> Self {
        if coefficients.is_empty() {
            panic!("A polynomial has to have at least one coefficient.");
        }

        // Coeffs > 1 + leading zero -> need to shrink the polynomial down
        // st the lead coefficient is nonzero.
        let coeffs = if coefficients.len() > 1 && coefficients[0] == 0 {
            let mut pruned = coefficients
                .iter()
                .copied()
                .skip_while(|&x| x == 0)
                .collect::<Vec<_>>();
            if pruned.is_empty() {
                pruned.push(0u8);
            }
            assert!(!pruned.is_empty());
            pruned
        } else {
            coefficients.to_vec()
        };

        Self(coeffs)
    }

    pub(crate) fn monomial(degree: usize, coefficient: usize) -> GaloisPolynomial {
        if coefficient == 0 {
            gf_poly_zero()
        } else {
            // x^degree + x ^{degree - 1} + ... + 0 (x ^ 0) = 1
            let mut coeffs = vec![0u8; degree + 1];
            coeffs[0] = coefficient as u8;
            GaloisPolynomial(coeffs)
        }
    }

    pub(crate) fn coefficients(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn degree(&self) -> usize {
        self.0.len() - 1
    }

    // This is zero if and only if there is one zero coefficient => the scalar term is 0
    pub(crate) fn is_zero(&self) -> bool {
        self.0[0] == 0
    }

    pub(crate) fn is_zero_deg_monomial(&self) -> bool {
        self.0.len() == 1
    }

    // This is O(n) for an n-degree polynomial.
    pub(crate) fn is_monomial(&self) -> bool {
        self.is_zero_deg_monomial() || { self.0.iter().copied().filter(|&x| x > 0).count() == 1 }
    }

    // Gets the value of the coefficient at the nth degree
    pub(crate) fn nth_coefficient(&self, degree: usize) -> u8 {
        let n = self.0.len();
        assert!((0..n).contains(&degree));
        self.0[n - 1 - degree]
    }

    pub(crate) fn is_larger(&self, other: &GaloisPolynomial) -> bool {
        self.0.len() > other.coefficients().len()
    }

    // Evaluates the given polynomial at point a,
    // (i.e.) solves the roots, afaik.
    pub(crate) fn evaluate_at(&self, a: usize) -> u8 {
        match a {
            // @ x = 0, is just the lowest order coefficient.
            0 => self.nth_coefficient(0),
            // @ x = 1 is just a sum of the coefficients.
            1 => {
                let mut res = 0u8;
                for coefficient in self.0.iter().copied() {
                    res = gf_add(res.into(), coefficient.into()) as u8;
                }
                res
            }
            // @ x > 1, substitute x into the polynomial and evaluate.
            // Recall:  x ( a_2 x + a_1) + a_0 => a_2 x^2 + a_1 x + a_0
            _ => {
                let mut res = self.0[0];
                for coefficient in self.0.iter().copied().skip(1) {
                    let mul = gf_multiply(a, res.into());
                    res = gf_add(mul, coefficient.into()) as u8
                }

                res
            }
        }
    }
}

pub(crate) fn gf_poly_zero() -> GaloisPolynomial {
    GaloisPolynomial(vec![0])
}
pub(crate) fn gf_poly_one() -> GaloisPolynomial {
    GaloisPolynomial(vec![1])
}

// Adds or subtracts (same operation in GF(2^8)) two polynomials p1, p2
// This doesn't really need to be a method on GP.
// It -could- be a std::ops::Add on GaloisPolynomial, but I think letting it remain a function
// is clearest.
pub(crate) fn gf_poly_add(p1: &GaloisPolynomial, p2: &GaloisPolynomial) -> GaloisPolynomial {
    if p1.is_zero() {
        return p2.clone();
    }
    if p2.is_zero() {
        return p1.clone();
    }

    // Find the smaller of the two polynomials
    let (smaller, larger) = if p1.is_larger(p2) { (p2, p1) } else { (p1, p2) };

    let diff = larger.coefficients().len() - smaller.coefficients().len();
    // Copy the large polynomial to copy over the larger order terms.
    let mut res = larger.coefficients().to_vec();

    // Zip larger[diff-1..] and smaller together (should be same size)
    // Each tuple are the summands a and b to be added together
    // The map drives the addition.
    //
    // The index arithmetic -might- be a little off here
    // TODO: test this and ensure that it doesn't panic and works as expected.
    let summands = larger
        .coefficients()
        .iter()
        .copied()
        .skip(diff)
        .zip_eq(smaller.0.iter().copied())
        .map(|(a, b)| gf_add(a.into(), b.into()) as u8);

    // The lower coefficients of res (copied from larger) with the sums of the summands.
    res.splice(diff.., summands);
    GaloisPolynomial::new(&res)
}

// Polynomial multiplication:
// (a_1 x + a_0) * (b_1 x + b_0) => a_1 * b_1 x^2 + (a_1 b_0 + b_1 a_0) x + a_0 + b_0
// [is equivalent to a nested for-loop]
pub(crate) fn gf_poly_mul(p1: &GaloisPolynomial, p2: &GaloisPolynomial) -> GaloisPolynomial {
    if p1.is_zero() || p2.is_zero() {
        return gf_poly_zero();
    }

    // 0-degree Monomial multiplication <=> scalar multiplication
    if p1.is_zero_deg_monomial() {
        return gf_poly_scale(p2, p1.nth_coefficient(0).into());
    }
    if p2.is_zero_deg_monomial() {
        return gf_poly_scale(p1, p2.nth_coefficient(0).into());
    }

    // There seems to be a bit of a bug in this multiplication but only for some terms.
    // The alpha table -seems- correct, this needs some more scrutiny.
    let mut product = vec![0; p1.coefficients().len() + p2.coefficients().len() - 1];
    for i in 0..p1.coefficients().len() {
        let a = p1.coefficients()[i];
        for j in 0..p2.coefficients().len() {
            let b = p2.coefficients()[j];
            let mul = gf_multiply(a.into(), b.into());
            product[i + j] = gf_add(product[i + j].into(), mul) as u8;
        }
    }
    GaloisPolynomial::new(&product)
}

pub(crate) fn gf_poly_scale(p: &GaloisPolynomial, scalar: usize) -> GaloisPolynomial {
    match scalar {
        // 0 * p = 0
        0 => gf_poly_zero(),
        // 1 * p = p
        1 => p.clone(),
        // k * p = kp
        _ => {
            let new_coeffs = p
                .coefficients()
                .iter()
                .cloned()
                .map(|a| gf_multiply(a.into(), scalar) as u8)
                .collect::<Vec<_>>();
            GaloisPolynomial::new(&new_coeffs)
        }
    }
}

pub(crate) fn gf_poly_multiply_by_monomial(
    p: &GaloisPolynomial,
    degree: usize,
    coefficient: usize,
) -> GaloisPolynomial {
    if coefficient == 0 {
        return gf_poly_zero();
    }

    // gf_scale could be used here, but that will result in potentially 2 allocations.
    // Doing it manually is only one.
    let mut prod = vec![0; p.coefficients().len() + degree];

    let products = p
        .coefficients()
        .iter()
        .copied()
        .map(|c| gf_multiply(c.into(), coefficient) as u8);

    prod.splice(0..p.coefficients().len(), products);
    GaloisPolynomial::new(&prod)
}

// Polynomial Long Division
// RE: https://www.thonky.com/qr-code-tutorial/error-correction-coding#step-2-understand-polynomial-long-division
// Returns (quotient polynomial, remainder)
// Implementation adapted from: https://github.com/rxing-core/rxing/blob/main/src/common/reedsolomon/generic_gf_poly.rs
pub(crate) fn gf_poly_divide(
    dividend: &GaloisPolynomial,
    divisor: &GaloisPolynomial,
) -> (GaloisPolynomial, GaloisPolynomial) {
    assert!(!divisor.is_zero(), "Cannot divide by zero.");

    let mut quotient = gf_poly_zero();
    let mut remainder = dividend.clone();

    // This is effectively polynomial long division, done iteratively by answering the question:
    // What do I multiply the leading term by to cancel out the (current) dividend's leading term.
    // That then gets subtracted from the remainder (the original divisor)
    // scale/degree_difference = scale * x^{degree_difference} => iteration quotient,
    // in practice: this happens by "shifting" the degree of the divisor and scaling it by the
    // scale coefficient. (i.e, multiplying by a monomial: scale x ^ {degree difference});
    // the rest of the terms in the iteration quotioent get shifted to the proper degree.
    // eg. 2x = scale/degree_dif as monomial, 2x * (x^2 + 1) = 2x^3 + 2x.
    // This gets gets added in to the quotient each iteration.
    //
    // Stop when remainder is 0, or when the remainder's degree < divisor degree.

    let inverse_leading_term = gf_inverse(divisor.nth_coefficient(divisor.degree()).into());

    while remainder.degree() >= divisor.degree() && !remainder.is_zero() {
        let degree_difference = remainder.degree() - divisor.degree();

        let scale = gf_multiply(
            remainder.nth_coefficient(remainder.degree()).into(),
            inverse_leading_term,
        );

        // This gets subtracted from the remainder (starting from the divisor)
        let term = gf_poly_multiply_by_monomial(divisor, degree_difference, scale);
        // Build the iterative quotient as a series of added monomials.
        let iteration_quotient = GaloisPolynomial::monomial(degree_difference, scale);
        // Update q & r
        quotient = gf_poly_add(&quotient, &iteration_quotient);
        remainder = gf_poly_add(&remainder, &term);
    }

    (quotient, remainder)
}

// ----------PRIMITIVE OPERATIONS--------------
// Returns a + b (mod 2) in GF(2^8)
#[inline]
pub(crate) fn gf_add(a: usize, b: usize) -> usize {
    a ^ b
}

// Returns a * b (mod IRR_POLY) in GF(2^8)
#[inline]
pub(crate) fn gf_multiply(a: usize, b: usize) -> usize {
    if a == 0 || b == 0 {
        0
    } else {
        // Remainder is field-size - 1, not field-size;
        EXP_TABLE[(LOG_TABLE[a] + LOG_TABLE[b]).rem_euclid(REM)]
    }
}

// returns 2^a (mod IRR_POLY) in GF(2^8)
#[inline]
pub(crate) fn gf_exp(a: usize) -> usize {
    assert!((0..FIELD_SIZE).contains(&a));
    EXP_TABLE[a]
}

// Returns log_2(a) (mod IRR_POLY) in GF(2^8)
#[inline]
pub(crate) fn gf_log(a: usize) -> usize {
    assert!(
        (1..FIELD_SIZE).contains(&a),
        "OUT OF RANGE OR ZERO ARGUMENT: {a}"
    );
    LOG_TABLE[a]
}
// Returns the multiplicative inverse of a.
// 2^idx * a = 1 mod IRR_POLY
#[inline]
pub(crate) fn gf_inverse(a: usize) -> usize {
    assert!(a > 0, "Cannot take inverse of 0");

    let idx = FIELD_SIZE - LOG_TABLE[a] - 1;
    EXP_TABLE[idx]
}
