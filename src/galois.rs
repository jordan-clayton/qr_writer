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
const LOG_TABLES: ([usize; 256], [usize; 256]) = compute_gf_256_log_tables();
pub(crate) const EXP_TABLE: [usize; 256] = LOG_TABLES.0;
pub(crate) const LOG_TABLE: [usize; 256] = LOG_TABLES.1;

// ----------POLYNOMIAL OPERATIONS-------------
// TODO

// ----------PRIMITIVE OPERATIONS--------------
// Returns a + b in GF(2^8)
#[inline]
fn gf_add(a: usize, b: usize) -> usize {
    a ^ b
}

// Returns a * b in GF(2^8)
#[inline]
fn gf_multiply(a: usize, b: usize) -> usize {
    EXP_TABLE[(LOG_TABLE[a] + LOG_TABLE[b]).rem_euclid(FIELD_SIZE)]
}

// returns 2^a
#[inline]
fn gf_exp(a: usize) -> usize {
    assert!((0..FIELD_SIZE).contains(&a));
    EXP_TABLE[a]
}

// Returns log_2(a) in GF(2^8)
#[inline]
fn gf_log(a: usize) -> usize {
    assert!(
        (1..FIELD_SIZE).contains(&a),
        "OUT OF RANGE OR ZERO ARGUMENT: {a}"
    );
    LOG_TABLE[a]
}
// Returns the multiplicative inverse of a.
#[inline]
fn gf_inverse(a: usize) -> usize {
    assert!(a > 0, "Cannot take inverse of 0");

    let idx = FIELD_SIZE - LOG_TABLE[a] - 1;
    EXP_TABLE[idx]
}
