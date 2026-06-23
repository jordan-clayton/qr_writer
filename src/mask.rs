use crate::matrix::{Module, SquareMatrix};
use itertools::FoldWhile::{Continue, Done};
use itertools::Itertools;
// Mask utility abstractions and functions for determining the optimal mask patterns.

pub(crate) const MAX_NUM_MASK_PATTERNS: usize = 8;
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub(crate) enum MaskPattern {
    Zero,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
}

impl MaskPattern {
    // This should be called on a writable/data area
    // Applies the masking rule to suggest whether to flip the bit at (row, column).
    pub(crate) fn should_mask(&self, row: usize, column: usize) -> bool {
        match self {
            // (row + column) mod 2 == 0
            Self::Zero => (row + column) & 1 != 1,
            // (row) mod 2 == 0
            Self::One => row & 1 != 1,
            // (column) mod 3 == 0
            Self::Two => column.rem_euclid(3) == 0,
            // (row + column) mod 3 == 0
            Self::Three => (row + column).rem_euclid(3) == 0,
            // (floor(row / 2) + floor(column / 3) ) mod 2 == 0
            Self::Four => ((row >> 1) + (column / 3)) & 1 != 1,
            // (row * column) mod 2 + (row * column) mod 3 == 0
            Self::Five => (row * column).rem_euclid(2) + (row * column).rem_euclid(3) == 0,
            // ((row * column) mod 2 + (row * column) mod 3) mod 2 == 0
            Self::Six => ((row * column).rem_euclid(2) + (row * column).rem_euclid(3)) & 1 != 1,
            // ((row + PLUS column) mod 2 + (row * column) mod 3) mod 2 == 0
            Self::Seven => ((row + column).rem_euclid(2) + (row * column).rem_euclid(3)) & 1 != 1,
        }
    }
}

impl TryFrom<u8> for MaskPattern {
    type Error = ();
    fn try_from(n: u8) -> Result<Self, Self::Error> {
        match n {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            4 => Ok(Self::Four),
            5 => Ok(Self::Five),
            6 => Ok(Self::Six),
            7 => Ok(Self::Seven),
            _ => Err(()),
        }
    }
}

impl From<MaskPattern> for u8 {
    fn from(m: MaskPattern) -> Self {
        match m {
            MaskPattern::Zero => 0,
            MaskPattern::One => 1,
            MaskPattern::Two => 2,
            MaskPattern::Three => 3,
            MaskPattern::Four => 4,
            MaskPattern::Five => 5,
            MaskPattern::Six => 6,
            MaskPattern::Seven => 7,
        }
    }
}

impl TryFrom<usize> for MaskPattern {
    type Error = ();
    fn try_from(n: usize) -> Result<Self, Self::Error> {
        match n {
            0 => Ok(Self::Zero),
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            4 => Ok(Self::Four),
            5 => Ok(Self::Five),
            6 => Ok(Self::Six),
            7 => Ok(Self::Seven),
            _ => Err(()),
        }
    }
}

// Apply the base penalty for every run of at least 5 of the same colour
// Runs greater than 5 add 1 additional point for each additional pixel.
// (Runs over both rows and columns).
const BASE_PENALTY_RULE_1: usize = 3;
// +3 everytime there's a 2 x 2 submatrix of the same colour
// This can overlap.
const PENALTY_RULE_2: usize = 3;
// Finder-like patterns
const PENALTY_RULE_3: usize = 40;
// Every 5% deviation from 50/50 (black/white) is +10
const PENALTY_RULE_4: usize = 10;

// This has been -somewhat- verified by inspecting the scores on a test case
// I cannot seem to find a set of resources that agree on scores.
// Both Nayuki and Thonky contradict each other and neither apply the palindrome rule
// -- SO, what I can confirm thus far is that:
// - Scores for rules 1, 2, 4 all agree with Nayuki's implementation and reference material
// - Scores for rule 3 are -close- to the Thonky example:
//  - my hunch is that there might be some disagreements about the palindrome rule leading to
//    over/under counting
//  - When verifying the output with: https://www.nayuki.io/page/creating-a-qr-code-step-by-step
//      - by inspection, the values get closer if one discards the false-positives detected on the
//        finder patterns.
//
// I'm pretty sure the implementation is at least close enough to the reference comments on the
// ZXing repository that I'll consider them to be "correct."
//
// The plan is to allow for a mask hint/override to allow users to override the mask decision.
pub(crate) fn compute_best_mask(matrices: &[SquareMatrix<Module>]) -> usize {
    let mut best_score = usize::MAX;
    let mut best_mask = 0;
    for (i, matrix) in matrices.iter().enumerate() {
        let p1_score = penalty_rule_1(matrix);
        let p2_score = penalty_rule_2(matrix);
        let p3_score = penalty_rule_3(matrix);
        let p4_score = penalty_rule_4(matrix);

        let score = p1_score + p2_score + p3_score + p4_score;
        if score < best_score {
            best_score = score;
            best_mask = i;
        }
    }
    return best_mask;
}

// NOTE: This is -just- for testing to ensure the scores are correct.
// The above imperative code achieves the same thing with fewer allocations.
// NOTE TWICE: if these scores don't agree with the expected cases, implement tests on each run.
// (They should also probably be tested anyway, so look at implementing them).
pub(crate) fn compute_penalties(matrices: &[SquareMatrix<Module>]) -> Vec<usize> {
    let mut scores = vec![0; matrices.len()];

    // Compute the penalty scores for each
    scores.iter_mut().zip(matrices).for_each(|(score, matrix)| {
        *score = penalty_rule_1(matrix)
            + penalty_rule_2(matrix)
            + penalty_rule_3(matrix)
            + penalty_rule_4(matrix);
    });
    scores
}

pub(crate) fn penalty_rule_1(matrix: &SquareMatrix<Module>) -> usize {
    compute_rule_1_run(matrix, RunDirection::Row) + compute_rule_1_run(matrix, RunDirection::Column)
}

// This could just be a boolean.
#[repr(C)]
enum RunDirection {
    Row,
    Column,
}

// RUNS CANNOT OVERLAP -> this runs a sliding window over the run.
pub(crate) fn compute_rule_1_run(
    matrix: &SquareMatrix<Module>,
    run_direction: RunDirection,
) -> usize {
    // 0 = false = white
    // 1 = true = black
    // -1 = sentinel
    let mut current_color = -1;
    let mut num_similar = 0;
    let mut penalty_accumulator = 0;

    let n = matrix.side_length();

    for i in 0..n {
        for j in 0..n {
            let (i_0, j_0) = match run_direction {
                RunDirection::Row => (i, j),
                RunDirection::Column => (j, i),
            };

            let module_value = matrix.get(i_0, j_0).inner() as i32;

            if current_color == -1 {
                current_color = module_value;
                num_similar = 1;
                continue;
            }

            if current_color != module_value {
                current_color = module_value;
                num_similar = 1;
                continue;
            }

            // Sanity check => TODO: remove in API cleanup refactor
            assert_eq!(module_value, current_color);
            num_similar += 1;

            if num_similar == 5 {
                penalty_accumulator += BASE_PENALTY_RULE_1;
            }

            if num_similar > 5 {
                penalty_accumulator += 1;
            }
        }

        current_color = -1;
        num_similar = 0;
    }

    penalty_accumulator
}

pub(crate) fn penalty_rule_2(matrix: &SquareMatrix<Module>) -> usize {
    // Check each 2 x 2 box of the same colour.
    let n = matrix.side_length() - 1;
    let mut penalty_accumulator = 0;

    for i in 0..n {
        for j in 0..n {
            // Check each of the 4 modules.
            let a = matrix.get(i, j).inner();
            let b = matrix.get(i, j + 1).inner();
            let c = matrix.get(i + 1, j).inner();
            let d = matrix.get(i + 1, j + 1).inner();

            // a == b
            let e1 = !(a ^ b);
            // b == c
            let e2 = !(b ^ c);
            // c == d
            let e3 = !(c ^ d);
            if e1 && e2 && e3 {
                penalty_accumulator += 1;
            }
        }
    }

    penalty_accumulator * PENALTY_RULE_2
}

// Run over rows/columns of finder-like patterns (11 bits):
// P1: 1 0 1 1 1 0 1 0 0 0 0
// P2: 0 0 0 0 1 0 1 1 1 0 1
// These patterns can overlap and contribute to the penalty.
const NUM_FINDER_PATTERN_BITS: usize = 11;
// This isn't a great name, TODO: fix this.
const NUM_END_BITS: usize = 4;
const NUM_FINDER_PATTERN_PALINDROMIC_BITS: usize = NUM_FINDER_PATTERN_BITS - NUM_END_BITS;

const PATTERN: [bool; NUM_FINDER_PATTERN_PALINDROMIC_BITS] =
    [true, false, true, true, true, false, true];

const PATTERN_END: [bool; NUM_END_BITS] = [false; NUM_END_BITS];
// I might be -slighly- misunderstanding things here.
// Apparently the horzontal and vertical finder patterns can go into the quiet zone?
// I'm not quite sure whether that's correct -> available information seems contradictory.
// Thonky ref does not suggest considering the quiet zone.
//
// Additionally, patterns apparently can overlap per: https://www.nayuki.io/page/creating-a-qr-code-step-by-step: step 9
// notes in ZXing say not to apply a penalty to palindromic sequences: 000010111010000
// (i.e, P1 right after P2);
//  -- I'm genuinely unsure whether the quiet zone needs to be considered as part of the penalty,
//     But by the looks of the ZXing implementation, the quiet zone doesn't get considered in
//     the penalty computation.
pub(crate) fn penalty_rule_3(matrix: &SquareMatrix<Module>) -> usize {
    let mut penalty_accumulator = 0;
    let n = matrix.side_length();
    for i in 0..n {
        for j in 0..n {
            // Row check
            if j + NUM_FINDER_PATTERN_PALINDROMIC_BITS <= n {
                // The row accesses can grab slices.
                // get_row_ranges is non-inclusive in its range
                let row = matrix.get_row_range(i, j, NUM_FINDER_PATTERN_PALINDROMIC_BITS);

                // Sanity check.
                assert_eq!(row.len(), PATTERN.len());

                // Check P1
                let pattern_match = row
                    .iter()
                    .zip(PATTERN)
                    .map(|(m, b)| m.inner() == b)
                    .fold_while(
                        true,
                        |acc, e| {
                            if !acc { Done(acc) } else { Continue(e && acc) }
                        },
                    )
                    .into_inner();

                if pattern_match {
                    // Check for 4 falses on either side of the pattern.
                    //
                    // i is constant for all of these since we're iterating by row.
                    // The LHS is  j - NUM_END_BITS.
                    let l_match = horizontal_4_false(i, j as i32 - NUM_END_BITS as i32, matrix);
                    // The rhs is j + NUM_FINDER_PATTERN_PALINDROMIC_BITS
                    let r_match = horizontal_4_false(
                        i,
                        j as i32 + NUM_FINDER_PATTERN_PALINDROMIC_BITS as i32,
                        matrix,
                    );

                    if l_match || r_match {
                        penalty_accumulator += 1;
                    }
                }
            }
            // Column check
            if i + NUM_FINDER_PATTERN_PALINDROMIC_BITS <= n {
                // The column accesses will just kinda stink.
                let pattern_match = (i..i + NUM_FINDER_PATTERN_PALINDROMIC_BITS)
                    .zip_eq(PATTERN)
                    .map(|(k, b)| matrix.get(k, j).inner() == b)
                    .fold_while(
                        true,
                        |acc, e| {
                            if !acc { Done(acc) } else { Continue(e && acc) }
                        },
                    )
                    .into_inner();

                if pattern_match {
                    // Check for 4 falses on the top of the bottom of the pattern.
                    //
                    // j is constant for all of these since we're iterating by column.
                    // The top is i - NUM_END_BITS;
                    let t_match = vertical_4_false(j, i as i32 - NUM_END_BITS as i32, matrix);
                    // The bottom is i + NUM_FINDER_PATTERN_PALINDROMIC_BITS
                    let b_match = vertical_4_false(
                        j,
                        i as i32 + NUM_FINDER_PATTERN_PALINDROMIC_BITS as i32,
                        matrix,
                    );

                    if t_match || b_match {
                        penalty_accumulator += 1;
                    }
                }
            }
        }
    }

    penalty_accumulator * PENALTY_RULE_3
}

// NOTE: i has to be constant since we're iterating over a row.
fn horizontal_4_false(i: usize, from: i32, matrix: &SquareMatrix<Module>) -> bool {
    let n = matrix.side_length();

    // If we're out of bounds, return false
    // If from is negative, it will be extremely greater than n, so this will still return false.
    if from < 0 || from as usize + NUM_END_BITS > n {
        return false;
    }

    let range = matrix.get_row_range(i, from as usize, NUM_END_BITS);

    // Quick sanity check in-case get_row_range has a bug.
    assert_eq!(range.len(), PATTERN_END.len());

    range
        .iter()
        .zip(PATTERN_END)
        .map(|(m, b)| m.inner() == b)
        .fold_while(
            true,
            |acc, e| {
                if !acc { Done(acc) } else { Continue(e && acc) }
            },
        )
        .into_inner()
}

// NOTE: j has to be constant, since this iterates over a column.
fn vertical_4_false(j: usize, from: i32, matrix: &SquareMatrix<Module>) -> bool {
    let n = matrix.side_length();

    // If we're out of bounds, return false
    // If from is negative, it will be extremely greater than n, so this will still return false.
    if from < 0 || from as usize + NUM_END_BITS > n {
        return false;
    }

    let i = from as usize;
    // Iterate over the columns and compare with the pattern.
    (i..i + NUM_END_BITS)
        .zip_eq(PATTERN_END)
        .map(|(k, b)| matrix.get(k, j).inner() == b)
        .fold_while(
            true,
            |acc, e| {
                if !acc { Done(acc) } else { Continue(e && acc) }
            },
        )
        .into_inner()
}

// Measure the amount of 5% variances from the mean (50%)
// The total penalty is The number of (variance - 1) * PENALTY_RULE_4
pub(crate) fn penalty_rule_4(matrix: &SquareMatrix<Module>) -> usize {
    const DARK_PROPORTION: f32 = 0.5;
    const VARIANCE: f32 = 0.05;
    let mut num_dark = 0;
    let n = matrix.side_length();
    let total_size = (n * n) as f32;
    for i in 0..n {
        for j in 0..n {
            // If we get a black module, increment the accumulator.
            if matrix.get(i, j).inner() {
                num_dark += 1;
            }
        }
    }

    // Compute the total proportion
    let p_dark = num_dark as f32 / total_size;

    // The penalty is applied every 5% distance away from the mean.
    // p = k * 0.05 +- 0.50. Num penalties = floor(k)
    let num_penalties = ((p_dark - DARK_PROPORTION).abs() / VARIANCE).floor() as usize;

    num_penalties * PENALTY_RULE_4
}
