use crate::ecc::ECCLevel;
use crate::errors::{QrError, Result};
use crate::mask::compute_best_mask;
use crate::mask::*;
use crate::tables::*;
use bitvec::prelude::*;

// The quiet zone needs to be at least 4 modules around the matrix
pub(crate) const QUIET_ZONE_SIZE: usize = 4;

// NOTE TO SELF: matrices work in units of "modules" (using m to denote).
// These are similar to the concept of a "texel" or a matrix cell.

// Module side length, s: (((v-1) * 4) + 21), where v is the version (1-indexed)
// e.g Version 32: (((32 - 1) * 4) + 21) = 145m

// Future TODO: Version, Mask hints to allow a user to try picking a mask/version pattern.

// Drawing order
// 1. Timing
// 2. Finder
//  2_i. Separator
//  2_ii. Dark bit
// 3. Alignment
// ----
// 4. Version (if applicable)
//  4_i. Version/Format can be drawn in either order, but masking makes things very complicated.
//  4_ii. This is another reed_solomon, but it can be done on a much smaller scale.
// ----
// 5. Prepare for Masking:
//      5_i Make 8 clones if no mask hint provided
//      5_ii Skip the clone if a mask hint is provided
//
//      Note: The specification is unclear about whether the format/version information
//      should be written before/after the data bits
//
//      The specification says that it considers the full glyph, which seems to imply that
//      version and format bits -should- be written beforehand.
//      ZX/rxing both write the version + format areas -before- running the penalty algorithm.
//
//      -- Only Data bits get masked.
//      -- The Mask pattern gets added (XOR) into the Format bitstring before being written into
//      the code.
//
// 6. For each mask version (or one if chosen):
//  6_i. Write the Format bits.
//  6_ii. Write the data bits.
//
// 7. Run the penalty algorithm and pick the best code (unless mask hint supplied -- not yet
//    implemented)
//
//    7_i. The "best" code is the one with the lowest penalty score.
//
// 8. Add the quiet zone -> is performed on render.

// WHITE MODULE = 0 = false
// BLACK MODULE = 1 = true
#[derive(Copy, Clone, Debug)]
pub(crate) enum Module {
    // This indicates the module is unoccupied
    Writable(bool),
    // Make it such that "data" indicates "occupied"
    Data(bool),
    Finder(bool),
    // Technically separators are part of the finder pattern.
    Separator,
    Timing(bool),
    Alignment(bool),
    // Encoding Error level + mask
    Format(bool),
    // For V7 and greater
    Version(bool),
    Dark,
}

impl Module {
    pub(crate) fn inner(&self) -> bool {
        match *self {
            Self::Writable(inner) => inner,
            Self::Data(inner) => inner,
            Self::Finder(inner) => inner,
            Self::Timing(inner) => inner,
            Self::Alignment(inner) => inner,
            Self::Format(inner) => inner,
            Self::Version(inner) => inner,
            Self::Dark => true,
            Self::Separator => false,
        }
    }


    pub(crate) fn writable(&self) -> bool {
        matches!(self, Self::Writable(_))
    }

    // This might be easier to reason about if it were treated like a depth buffer.
    // Or possibly unused? This is more of a safety check for later steps in the matrix
    // emplacement so that I don't do something silly.
    pub(crate) fn can_overwrite_with(&self, with_module: &Self) -> bool {
        match *self {
            Self::Writable(_) => true,
            // WRITE THE DATA LAST. Data should not be considered part of a painter's algorithm
            // approach.
            Self::Data(_) => false,
            // Separators are part of finder-patterns and they cannot be overlapped with anything.
            Self::Finder(_) => false,
            Self::Separator => false,
            Self::Timing(_) => {
                // Timing patterns can be overwritten by:
                // Alignment, Finder/Separator, Timing (overdraw),
                matches!(
                    with_module,
                    Self::Alignment(_) | Self::Finder(_) | Self::Separator | Self::Timing(_)
                )
            }
            // Technically nothing, but alignment patterns need to be drawn after
            // writing the finder and timing patterns.
            // Alignment squares need to blend in with the timing modules.
            Self::Alignment(_) => false,
            // Since there's no pre-reservation of space, these no longer should be overwritable.
            // (The penalty algorithm reads the entire matrix - so Version and Format bits need to
            // be written beforehand).
            Self::Version(_) => false,
            Self::Format(_) => false,
            Self::Dark => false,
        }
    }
}

impl Default for Module {
    fn default() -> Self {
        Self::Writable(true)
    }
}

// NOTE: this could be re-implemented as a 1d bitfield.
// Swap once the api is figured out -> bool => load_be()
// NOTE: the "bytes" in this matrix as "pixels" are 1-minus in module mode.
// When exporting as an image, treat like a texture and sample it.
//
// Current intention is to export QR as square only.
#[derive(Clone, Debug)]
pub struct SquareMatrix<T>
where
    T: Clone + std::fmt::Debug + Default,
{
    data: Vec<T>,
    side_length: usize,
}

// CLEANUP TODO: Option-based implicit bounds checks once the code is tested enough to be
// considered correct.
impl<T> SquareMatrix<T>
where
    T: Clone + std::fmt::Debug + Default,
{
    // VERSION IS SUPPLIED 1-indexed
    pub(crate) fn new(n: usize) -> Self {
        // Create an empty all-black matrix that's fully writable.
        let data = vec![Default::default(); n * n];
        Self {
            data,
            side_length: n,
        }
    }

    pub fn side_length(&self) -> usize {
        self.side_length
    }
    // Returns the actual module cell (which holds state)
    // use .inner() to determine the value (white = false/black = true)
    pub fn get(&self, i: usize, j: usize) -> Option<&T> {
        self.data.get(self.side_length * i + j)
    }
    pub fn get_mut(&mut self, i: usize, j: usize) -> Option<&mut T> {
        self.data.get_mut(self.side_length * i + j)
    }

    // NOTE: this will panic if [i, j] + num_elements goes out of bounds
    // per the logic of a 2D matrix.
    // CLEANUP TODO: refactor into result.
    // NOTE: this is non-inclusive per slice semantics
    pub fn get_row_range(&self, i: usize, j: usize, num_elements: usize) -> Result<&[T]> {
        let n = self.side_length();

        if j + num_elements > n {
            Err(QrError::SampleError {
                reason: format!(
                    "ROW READ OUT OF RANGE! row_length: {n}, column_idx: {} ",
                    j + num_elements - 1
                ),
            })
        } else {
            let idx = n * i + j;

            Ok(&self.data[idx..idx + num_elements])
        }
    }
}

// This will probably go unused.
impl SquareMatrix<u8> {
    // This might go unused, but could be helpful.
    pub fn complement(mut self) -> Self {
        self.data.iter_mut().for_each(|b| {
            *b ^= 0xFF;
            *b &= 0x01;
        });
        self
    }

    pub fn destructure_into_bytes(self) -> (Vec<u8>, usize) {
        let side_length = self.side_length;
        let mat = self.data;
        (mat, side_length)
    }

    // TODO: work this into a proper docstring.
    // For best results, the new side length should be an integer multiple of the original matrix side
    // length (i.e. use this to perform integer scaling)
    // It is ill-advised to resize to a size smaller than the minimum required side length for the QR
    // It is also ill-advised to resize a resized QR code. Since this function returns a new
    // SquareMatrix<u8> the original will not be dropped--resize from the original matrix
    // returned from QRCodeMatrix::render().
    // If you require fine-control over scaling, prefer exporting to svg.
    //
    // The minimum side length, s: (((v-1) * 4) + 21), where v is the version (counting from 1)
    // TODO: guard against this; abstract over SquareMatrix<u8> and append the QR version.
    // TODO TWICE: refactor this to return a result if the new side length is smaller than the
    // original.
    pub fn resize(&self, new_side_length: usize) -> Result<SquareMatrix<u8>> {
        let mut out_mat = SquareMatrix::new(new_side_length);
        for i in 0..new_side_length {
            for j in 0..new_side_length {
                *out_mat.get_mut(i, j).ok_or(QrError::SampleError {
                    reason: format!("Invalid read at i: {i}, j: {j}"),
                })? = self.sample_matrix(i, j, new_side_length)?;
            }
        }

        Ok(out_mat)
    }

    // This might be handled elsewhere, but can be used for re-interpolating
    // a QR (in texels/modules) into larger squares.
    // This is a basic nearest-neighbor sampling of the matrix
    pub fn sample_matrix(&self, i: usize, j: usize, img_side_length: usize) -> Result<u8> {
        let n = self.side_length;

        let ratio = n as f32 / img_side_length as f32;

        // Sample the center of the original point
        // -> this avoids the pixel drift with NN sampling.
        let i0 = (i as f32 + 0.5) * ratio - 0.5;
        let j0 = (j as f32 + 0.5) * ratio - 0.5;

        let i_1 = i0.floor().clamp(0.0, (n - 1) as f32);
        let i_2 = i0.ceil().clamp(0.0, (n - 1) as f32);
        let j_1 = j0.floor().clamp(0.0, (n - 1) as f32);
        // Clamp to the side length to avoid sampling the next row.
        let j_2 = j0.ceil().clamp(0.0, (n - 1) as f32);

        let points = [(i_1, j_1), (i_1, j_2), (i_2, j_1), (i_2, j_2)];
        let sample = (i0, j0);
        let mut min_dist = f32::MAX;
        let mut sample_point: Option<(usize, usize)> = None;

        // Closure to compute the squared distance.
        // If this happens to be required elsewhere, refactor it to an inline function.
        let dist = |p1: (f32, f32), p2: (f32, f32)| {
            let d = (p2.0 - p1.0, p2.1 - p1.1);

            d.0 * d.0 + d.1 * d.1
        };
        points.iter().for_each(|p| {
            let sample_d = dist(sample, *p);
            if sample_d < min_dist || sample_point.is_none() {
                min_dist = sample_d;
                sample_point = Some((p.0 as usize, p.1 as usize))
            }
        });

        let (i_a, j_a) = sample_point.ok_or(QrError::SampleError {
            reason: "Could not find nearest neigbor to sample.".to_string(),
        })?;
        Ok(*self.get(i_a, j_a).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i_a}, j: {j_a}"),
        })?)
    }
}

// This could expose a complement, but the semantic Module information doesn't map 1:1
impl SquareMatrix<Module> {
    // Returns a vector of u8 (booleans cast to u8) and the side length.
    // This is mostly used for testing and may have to change if/when swapping to a bitfield.
    pub(crate) fn destructure_into_bytes(self) -> (Vec<u8>, usize) {
        let side_length = self.side_length;
        let mat = self.data.iter().map(|b| b.inner() as u8).collect();
        (mat, side_length)
    }
}

// TODO: MIGRATE THIS TO A DIFFERENT FILE -> this should be within qr or similar.
// TODO: determine how best to work with the render/resampling.
//  - Could be an enumeration
//  - Could be generic over T with restricted impl blocks.
pub struct QRCodeMatrix {
    // TODO: decide whether to make this concrete over u8 instead instead of Module.
    // The module semantics are useful for construction and inspection, but they aren't and
    // shouldn't be modifiable after construction.
    matrix: SquareMatrix<Module>,
    version: usize,
    ecc_level: ECCLevel,
}

impl QRCodeMatrix {
    // Let the drawing routine happen in the constructor.
    // TODO: add a mask hint + version hints.
    pub fn new(
        codewords: &BitVec<u8, Msb0>,
        version: usize,
        ecc_level: ECCLevel,
        mask_hint: Option<u8>,
    ) -> Result<Self> {
        let matrix = draw_and_pick_best_qr_code(codewords, version, ecc_level, mask_hint)?;
        Ok(Self {
            matrix,
            version,
            ecc_level,
        })
    }
    pub fn version(&self) -> usize {
        self.version
    }
    pub fn ecc_level(&self) -> ECCLevel {
        self.ecc_level
    }
    pub fn side_length(&self) -> usize {
        self.matrix.side_length
    }

    // This likely has very little practical use.
    // Expose it if it's useful.
    pub(crate) fn matrix(&self) -> &SquareMatrix<Module> {
        &self.matrix
    }

    // TODO: determine whether to resample in the render.
    // Or do it later on export.
    pub(crate) fn render(&self) -> Result<SquareMatrix<u8>> {
        let old_matrix = self.matrix();
        let old_side_length = old_matrix.side_length();

        // The quiet zone appears on either side of the matrix.
        let new_side_length = old_side_length + 2 * QUIET_ZONE_SIZE;
        let mut mat = vec![1; new_side_length * new_side_length];
        for i in 0..old_side_length {
            for j in 0..old_side_length {
                // Black = 0, white = 1 -> just write the complement.
                mat[(i + QUIET_ZONE_SIZE) * new_side_length + (j + QUIET_ZONE_SIZE)] = !(old_matrix
                    .get(i, j)
                    .ok_or(QrError::RenderError{
                        reason: format!("Invalid read at i: {i}, j: {j} during matrix render."),
                    })?
                    .inner())
                    as u8;
            }
        }

        // This matrix (u8) can be complemented if there's a bug or a desire to take 1-s complement.
        Ok(SquareMatrix {
            data: mat,
            side_length: new_side_length,
        })
    }
}

// NOTE: this no longer falls-back if a MaskHint is provided due to returning a result.
// Supplying an invalid version mask is considered an error that should not be quietly
// recovered from.
//
// White module = 0 = false
// Black module = 1 = true
pub(crate) fn draw_and_pick_best_qr_code(
    codewords: &BitVec<u8, Msb0>,
    version: usize,
    ecc_level: ECCLevel,
    mask_hint: Option<u8>,
) -> Result<SquareMatrix<Module>> {
    let n = (version - 1) * 4 + 21;
    let mut matrix = SquareMatrix::new(n);

    emplace_timing_patterns(&mut matrix)?;
    // TODO: rename -> just emplace_finder_patterns is fine.
    // No longer need to write it before timing.
    emplace_finder_patterns_into_blank_matrix(&mut matrix, version)?;
    emplace_alignment_squares(&mut matrix, version)?;

    // Write version data -> this can be done before embedding the version information.
    if version >= 7 {
        emplace_version_information(&mut matrix, version)?;
    }

    // If there's a mask hint and it's valid, emplace the
    // format and data bits.
    if let Some(mask) = mask_hint {
        let mask_pattern = mask.try_into()?;

        emplace_format_information_area(&mut matrix, ecc_level, mask_pattern)?;

        emplace_data_bits(&mut matrix, codewords, mask_pattern)?;
        return Ok(matrix);
    }

    // Otherwise, try and pick the best one.
    let mut candidates = Vec::with_capacity(MAX_NUM_MASK_PATTERNS);
    candidates.push(matrix);
    // Make 7 more copies (total 8).
    for _i in 1..MAX_NUM_MASK_PATTERNS {
        candidates.push(candidates[0].clone())
    }

    for (i, candidate) in candidates.iter_mut().enumerate() {
        let mask_pattern = i.try_into()?;
        // Write format information
        emplace_format_information_area(candidate, ecc_level, mask_pattern)?;

        // Write the data bits -> this is done by mutation, since it's faster to just
        // preallocate all 8 matrices.
        emplace_data_bits(candidate, codewords, mask_pattern)?;
    }

    let best_mask = compute_best_mask(&candidates)?;

    // Candidates gets deallocated anyway, but removal avoids implicit clones.
    Ok(candidates.swap_remove(best_mask))
}

// This is just for inspection tests
// Since this is expected to crash, it'll just unwrap on invalid reads.
#[cfg(debug_assertions)]
pub(crate) fn print_matrix_and_crash(matrix: &SquareMatrix<Module>) {
    let side_length = matrix.side_length();
    // num cells * (character + tab) + newlines
    let mut num_data = 0;
    let mut out_string = String::with_capacity(3 * side_length * side_length + side_length);
    for i in 0..side_length {
        for j in 0..side_length {
            let mat = matrix
                .get(i, j)
                .ok_or(QrError::SampleError {
                    reason: format!("Invalid read at i: {i}, j: {j}"),
                })
                .unwrap();

            match mat.inner() {
                true => out_string.push('#'),
                false => out_string.push(' '),
            }
            if let Module::Data(_) = mat {
                num_data += 1;
            }
            out_string.push(' ');
        }
        out_string.push('\n');
    }
    eprintln!("----------------------------------------------------------------");
    eprintln!("{out_string}");
    eprintln!("----------------------------------------------------------------");
    eprintln!("num_data bits written: {num_data}");
    panic!("Breakpoint to check the matrix");
}

// ---- TIMING PATTERNS ---

// Since this -technically- doesn't need to happen before the other elements,
// this will not hard-assert writable invariants.
// It's wiser to draw the timing before drawing the finder pattern though.
pub(crate) fn emplace_timing_patterns(matrix: &mut SquareMatrix<Module>) -> Result<()> {
    let side_length = matrix.side_length();
    // Technically this can work on all matrices of side length 6 or greater, but
    // since this is for qr only, go with the minimum side length for a QR code.

    if side_length < 21 {
        return Err(
            QrError::WriteError{
                reason: format!("Invalid side length during timing pattern emplacement. \
                            Must be greater than 21: {side_length}.")
            }
            );
    }

    // Alternate dark-light, always starting dark.
    // i.e. even parity = dark.

    // The timing is 1-horizontal @ 6th (idx 6) row
    // and 6th column counting from 0.
    //
    // If a 1-module overdraw is ever a bottleneck, this could skip the row write on (6, 6).
    const FIXED_IDX: usize = 6;
    for p in 0..side_length {
        // Column write: i = 6
        let col_module = matrix.get_mut(FIXED_IDX, p).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {FIXED_IDX}, j: {p} during column timing pattern emplacement."),
        })?;

        // Dark is on even parity
        // dark = true = 1
        let write_value = p & 1 != 1;
        let next_module = Module::Timing(write_value);
        if col_module.writable() || col_module.can_overwrite_with(&next_module) {
            *col_module = next_module;
        }

        // Row write: j = 6
        let row_module = matrix.get_mut(p, FIXED_IDX).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {p}, j: {FIXED_IDX} during row timing pattern emplacement."),
        })?;

        if row_module.writable() || row_module.can_overwrite_with(&next_module) {
            *row_module = next_module;
        }
    }
    Ok(())
    // Vertical (j = 5)
}

// ---- FINDER PATTERN ----

// The finder bit pattern drawing order does not matter.
const MAX_SIDE_BLACK_FINDER_BITS: usize = 7;

const MAX_SIDE_WHITE_FINDER_BITS: usize = 5;
const MAX_WHITE_OFFSET: usize = MAX_SIDE_WHITE_FINDER_BITS - 1;
const TOTAL_WHITE_FINDER_BITS: usize = 16;

const MAX_SIDE_SEPARATOR_BITS: usize = MAX_SIDE_BLACK_FINDER_BITS + 1;
const MAX_SIDE_SEPARATOR_OFFSET: usize = MAX_SIDE_SEPARATOR_BITS - 1;

// The drawing pointer arithmetic is a bit difficult to follow
// TODO: consider trying to come up with something more clever
// -- Even with the mild overdraw, it -might- be easier to just overwrite pixels and work bottom
// up.
//
// Finder patterns:
// - 7m * 7m outer black square
// - 5m * 5m inner white square
// - 3m * 3m inner black square
// In all versions, these are placed in the top left, top right, bottom left corners
// -> this effectively amounts to writing a 5m long hollow square
//
// After emplacing the finder patterns, this also emplaces the separators and the dark bit
// (both are part of the finder pattern)
pub(crate) fn emplace_finder_patterns_into_blank_matrix(
    matrix: &mut SquareMatrix<Module>,
    version: usize,
) -> Result<()> {

    let side_length = matrix.side_length();
    // The QR version 1 (the minimum) has side length 21
    if side_length < 21 {
        return Err(
            QrError::WriteError{reason: format!("Invalid side length during finder pattern emplacement.\
                Should be >= 21 but was: {side_length}")}
            );
    }

    // This does a little bit of overdraw (in addition to the timing patterns),
    // to cut down on the drawing complexity.
    // --- BLACK CELLS --- ->
    let inner_black_extent = side_length - MAX_SIDE_BLACK_FINDER_BITS;

    // TOP LEFT (0, 0), start at: (0, 0)
    emplace_black_finder_pattern_into_blank_matrix(0, 0, matrix)?;
    // BOTTOM LEFT (n-7, 0), start at: (n - 7, 0)
    emplace_black_finder_pattern_into_blank_matrix(inner_black_extent, 0, matrix)?;
    // TOP RIGHT (0, n-1), start at: (0, n-7)
    emplace_black_finder_pattern_into_blank_matrix(0, inner_black_extent, matrix)?;

    // --- WHITE CELLS ---
    let inner_white_extent = side_length - MAX_SIDE_WHITE_FINDER_BITS - 1;

    // TOP LEFT (0, 0), start at: (1, 1).
    emplace_white_finder_pattern_into_blank_matrix(1, 1, matrix)?;

    // BOTTOM LEFT (n-1, 0), start at: (n - 1 - 5, 1)
    emplace_white_finder_pattern_into_blank_matrix(inner_white_extent, 1, matrix)?;

    // TOP RIGHT (0, n-1), start at: (1, n-1 -5)
    emplace_white_finder_pattern_into_blank_matrix(1, inner_white_extent, matrix)?;

    // Write the separators
    // top_left starting indices: (0 + MAX_SIDE_SEPARATOR_OFFSET, 0)
    // bottom_left staring indices: (side_length - MAX_SIDE_SEPARATOR_BITS, 0)
    // top_right starting indices: (0 + MAX_SIDE_SEPARATOR_OFFSET, side_length - 1)
    let tl_start = (MAX_SIDE_SEPARATOR_OFFSET, 0);
    let bl_start = (side_length - MAX_SIDE_SEPARATOR_BITS, 0);
    let tr_start = (MAX_SIDE_SEPARATOR_OFFSET, side_length - 1);

    emplace_separator_bits(tl_start, bl_start, tr_start, matrix)?;
    // Write the dark bit

    // The dark bit will be at:
    // (8, [4 * version + 9]),
    // version = 1-indexed here.
    // i.e. the dark bit is 1 cell to the right of the bottom left finder pattern's top right
    // corner.

    const DARK_J: usize = 8;
    let dark_i = version * 4 + 9;
    let module = matrix.get_mut(dark_i, DARK_J).ok_or(QrError::SampleError {
        reason: format!("Invalid read at i: {dark_i}, j: {DARK_J} during dark-bit emplacement"),
    })?;

    // Ensure we can write to the cell (i.e, we're not on a finder/separator)
    if !module.can_overwrite_with(&Module::Dark) {
        return Err(QrError::WriteError {
            reason: format!(
                "Cannot write dark bit at i: {dark_i}, j: {DARK_J}.\n\
        Occupant: {:?}",
                module
            ),
        });
    }

    *module = Module::Dark;
    Ok(())
}
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(C)]
enum Direction {
    Right,
    Down,
    Left,
    Up,
}

impl Direction {
    fn delta(&self) -> (i32, i32) {
        match self {
            Self::Right => (0, 1),
            Self::Down => (1, 0),
            Self::Left => (0, -1),
            Self::Up => (-1, 0),
        }
    }
    fn rotate_clockwise(&mut self) {
        *self = match self {
            Self::Right => Self::Down,
            Self::Down => Self::Left,
            Self::Left => Self::Up,
            Self::Up => Self::Right,
        }
    }
}

// i0 => vertical starting index
// j0 => horizontal starting idx.
// i_max => furthest vertical pointer
// j_max => furthest horizontal pointer
// Per the function name, this only write white bits
// This is a total of 2 * s + 2 * (s - 2) = 4s - 4 modules:
// s = 5 => 16 modules.
// WHITE = false = 0
pub(crate) fn emplace_white_finder_pattern_into_blank_matrix(
    i0: usize,
    j0: usize,
    matrix: &mut SquareMatrix<Module>,
) -> Result<()> {
    // Index pointers
    let mut i = i0;
    let mut j = j0;
    let mut direction = Direction::Right;

    let mut written = 0;

    while written < TOTAL_WHITE_FINDER_BITS {
        *matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during finder pattern (white) emplacement."),
        })? = Module::Finder(false);
        written += 1;
        let (di, dj) = direction.delta();
        i = ((i as i32) + di) as usize;
        j = ((j as i32) + dj) as usize;
        if i < i0 {
            i += 1;
            direction.rotate_clockwise();
            // To prevent overdraw, move the other pointer 1 and jump to the next iteration.
            let (_, dj) = direction.delta();
            j = ((j as i32) + dj) as usize;
            continue;
        }

        if i > i0 + MAX_WHITE_OFFSET {
            i -= 1;
            direction.rotate_clockwise();
            let (_, dj) = direction.delta();
            j = ((j as i32) + dj) as usize;
            continue;
        }

        if j < j0 {
            j += 1;
            direction.rotate_clockwise();
            let (di, _) = direction.delta();
            i = ((i as i32) + di) as usize;
            continue;
        }
        if j > j0 + MAX_WHITE_OFFSET {
            j -= 1;
            direction.rotate_clockwise();
            let (di, _) = direction.delta();
            i = ((i as i32) + di) as usize;
            continue;
        }
    }

    // Assert Invariants: Return Err if the computation is invalid.
    if direction != Direction::Up {
        return Err(QrError::WriteError {
            reason: "Direction Invariant not upheld in finder pattern (white) emplacement.".to_string(),
        });
    }

    if i != i0 {
        return Err(QrError::WriteError {
            reason: format!(
                "Row index pointer invariant not upheld in finder pattern (white) emplacement.\n\
            i: {i}, i0: {i0}."
            ),
        });
    }

    if j != j0 {
        return Err(QrError::WriteError {
            reason: format!(
                "Column index pointer invariant not upheld in finder pattern (white) emplacement.\n\
            j: {j}, j0: {j0}"
            ),
        });
    }
    Ok(())
}

// This is used to encode the state of the module cell to make zig-zagging a little bit easier.
fn emplace_black_finder_pattern_into_blank_matrix(
    i0: usize,
    j0: usize,
    matrix: &mut SquareMatrix<Module>,
) -> Result<()> {
    for i in i0..i0 + MAX_SIDE_BLACK_FINDER_BITS {
        for j in j0..j0 + MAX_SIDE_BLACK_FINDER_BITS {
            let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
                reason: format!("Invalid read at i: {i}, j: {j} during finder pattern (black) emplacement."),
            })?;
            let next_module = Module::Finder(true);
            // This will prevent the loop from overwriting the white inner ring if that's
            // accidentally called first.
            if module.writable() || module.can_overwrite_with(&next_module) {
                // Write to the matrix cell.
                *module = next_module;
            }
        }
    }
    Ok(())
}

/// Args:
/// # tl_bl: bottom left corner indices: (i, j) of top left finder square
/// # bl_tl: top left corner indices: (i, j) of bottom left finder square
/// # tr_br: bottom right corner indices: (i, j) of top right finder square
// This could probably be factored out into a generic function with additional logic for the
// pointer traversal, but until that becomes necessary, this will just be hardcoded.
fn emplace_separator_bits(
    tl_bl: (usize, usize),
    bl_tl: (usize, usize),
    tr_br: (usize, usize),
    matrix: &mut SquareMatrix<Module>,
) -> Result<()> {
    // TOP LEFT, (top left corner should be (0, 0))
    // Traversal is: (right, up)
    let (mut i0, mut j0) = tl_bl;
    let mut i = i0;
    let mut j = j0;

    while j <= (j0 + MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during separator bit emplacement."),
        })?;

        if !module.can_overwrite_with(&Module::Separator) {
            return Err(QrError::WriteError {
                reason: format!("Cannot replace module: {:?} with separator.", module),
            });
        }

        *module = Module::Separator;

        j += 1;
    }

    // Correct j's index
    j -= 1;

    // Skip up one cell, (i, j) has already been written in the previous loop.
    i -= 1;
    while i >= (i0 - MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during separator bit emplacement."),
        })?;
        if !module.can_overwrite_with(&Module::Separator) {
            return Err(QrError::WriteError {
                reason: format!("Cannot replace module: {:?} with separator.", module),
            });
        }
        *module = Module::Separator;

        // Avoid overflow (after last write)
        if i == 0 {
            break;
        }
        i -= 1;
    }

    // The top right and top left have to end at 0
    if i != 0 {
        return Err(QrError::WriteError {
            reason: format!(
                "Index pointer invariant not upheld after separator insertion, i: {i}, i0: 0"
            ),
        });
    }

    if j != MAX_SIDE_SEPARATOR_BITS - 1 {
        return Err(QrError::WriteError {
            reason: format!(
                "Index pointer invariant not upheld after separator insertion, j: {i}, j0: {}",
                MAX_SIDE_SEPARATOR_BITS - 1
            ),
        });
    }

    // BOTTOM LEFT:
    // Traversal is: (right, down)
    (i0, j0) = bl_tl;
    i = i0;
    j = j0;
    while j <= (j0 + MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during separator emplacement."),
        })?;
        if !module.can_overwrite_with(&Module::Separator) {
            return Err(QrError::WriteError {
                reason: format!("Cannot replace module: {:?} with separator.", module),
            });
        }
        *module = Module::Separator;
        j += 1;
    }

    // Correct j's position
    j -= 1;

    // Skip down one cell.
    i += 1;
    while i <= (i0 + MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during separator emplacement."),
        })?;
        if !module.can_overwrite_with(&Module::Separator) {
            return Err(QrError::WriteError {
                reason: format!("Cannot replace module: {:?} with separator.", module),
            });
        }
        *module = Module::Separator;
        i += 1;
    }

    // i ends + 1 without correction.
    if i != i0 + MAX_SIDE_SEPARATOR_BITS {
        return Err(QrError::WriteError {
            reason: format!(
                "Index pointer invariant not upheld after separator insertion, i: {i}, i0: {}",
                i0 + MAX_SIDE_SEPARATOR_BITS - 1
            ),
        });
    }

    // j ends in the proper position because it needs to be corrected.
    if j != j0 + MAX_SIDE_SEPARATOR_BITS - 1 {
        return Err(QrError::WriteError {
            reason: format!(
                "Index pointer invariant not upheld after separator insertion, j: {i}, j0: {}",
                MAX_SIDE_SEPARATOR_BITS - 1
            ),
        });
    }

    // TOP_RIGHT:
    // Traversal is: (left, up)
    (i0, j0) = tr_br;
    i = i0;
    j = j0;

    while j >= j0 - MAX_SIDE_SEPARATOR_OFFSET {
        let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during separator emplacement."),
        })?;
        if !module.can_overwrite_with(&Module::Separator) {
            return Err(QrError::WriteError {
                reason: format!("Cannot replace module: {:?} with separator.", module),
            });
        }

        *module = Module::Separator;

        // Avoid overflow (after last write)
        if j == 0 {
            break;
        }

        j -= 1;
    }

    // j needs to be corrected by +1.
    j += 1;

    // Skip up one cell.
    i -= 1;
    while i >= i0 - MAX_SIDE_SEPARATOR_OFFSET {
        let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j} during separator emplacement."),
        })?;
        if !module.can_overwrite_with(&Module::Separator) {
            return Err(QrError::WriteError {
                reason: format!("Cannot replace module: {:?} with separator.", module),
            });
        }
        *module = Module::Separator;

        // Avoid overflow after last write
        if i == 0 {
            break;
        }

        i -= 1;
    }

    // The top right 0 has to be 0
    if i != 0 {
        return Err(QrError::WriteError {
            reason: format!(
                "Index pointer invariant not upheld after separator insertion, i: {i}, i0: 0"
            ),
        });
    }

    // j should be MAX_SIDE_SEPARATOR_BITS away from its initial position.
    if j != j0 - (MAX_SIDE_SEPARATOR_BITS - 1) {
        return Err(QrError::WriteError {
            reason: format!(
                "Index pointer invariant not upheld after separator insertion, j: {i}, j0: {}",
                j0 - (MAX_SIDE_SEPARATOR_BITS - 1)
            ),
        });
    }
    Ok(())
}

// Lookups: version - 1;
pub(crate) fn emplace_alignment_squares(
    matrix: &mut SquareMatrix<Module>,
    version: usize,
) -> Result<()> {
    // Escape early if it's version one, there are no alignment squares to place.
    if version == 1 {
        return Ok(());
    }

    // Look up the list of indices.
    let centers = ALIGNMENT_BLOCK_CENTERS[version - 1].inner();
    // Make sure there's at least two centres (i.e. not AlignmentCenters::Zero).
    if centers.len() < 2 {
        return Err(
            QrError::WriteError{
                reason: format!("Invalid number of centers (< 2) for alignment square emplacement. {}", centers.len())
            }
            );
    }

    // Produce a list of permutations (with repetitions) of centres.
    // Loop through each centre and test whether there's overlap (corner check).
    // ie. filter out centres that fail.

    for i in 0..centers.len() {
        for j in 0..centers.len() {
            let i_center = centers[i];
            let j_center = centers[j];
            let center = (i_center, j_center);
            let corners = Corners::new(center).map_err(|_| QrError::WriteError{
                reason: format!("Invalid alignment center at i: {i_center}, j: {j_center}.")
            })?;
            // TODO: check whether can write, then pass the centre to the writing function.
            if can_write_alignment_square(&corners, matrix)? {
                write_alignment_square(corners.top_left(), corners.bottom_right(), matrix)?;
            }
        }
    }
    Ok(())
}

#[repr(transparent)]
pub(crate) struct Corners([(usize, usize); 4]);

// Top is i - 2;
// Bottom is i + 2;
//
// Leftmost is j - 2;
// Rightmost is j + 2;
impl Corners {
    // Center: (i, j)
    pub(crate) fn new(center: (usize, usize)) -> Result<Self> {
        if center.0 < 2 || center.1 < 2  {
            return Err(QrError::InvalidCorners
                );
        }
        let top_left = (center.0 - 2, center.1 - 2);
        let top_right = (center.0 - 2, center.1 + 2);
        let bottom_left = (center.0 + 2, center.1 - 2);
        let bottom_right = (center.0 + 2, center.1 + 2);
        Ok(Self([top_left, top_right, bottom_left, bottom_right]))
    }

    pub(crate) fn top_left(&self) -> (usize, usize) {
        self.0[0]
    }


    pub(crate) fn bottom_right(&self) -> (usize, usize) {
        self.0[3]
    }
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, (usize, usize)> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a Corners {
    type Item = &'a (usize, usize);
    type IntoIter = std::slice::Iter<'a, (usize, usize)>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub(crate) fn can_write_alignment_square(
    corners: &Corners,
    matrix: &SquareMatrix<Module>,
) -> Result<bool> {
    for &(i, j) in corners {
        let module = matrix.get(i, j).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i}, j: {j}"),
        })?;
        // If we can write an alignment square at each of the 4 corners, then we can write an
        // alignmnent square. (it doesn't matter if we're writing true or false)
        if !module.can_overwrite_with(&Module::Alignment(false)) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn write_alignment_square(
    from: (usize, usize),
    to: (usize, usize),
    matrix: &mut SquareMatrix<Module>,
) -> Result<()> {
    let mut p = 0;
    for i in from.0..=to.0 {
        for j in from.1..=to.1 {
            let alignment_value = get_alignment_module_value(p);
            let module = matrix.get_mut(i, j).ok_or(QrError::SampleError {
                reason: format!("Invalid read at i: {i}, j: {j}"),
            })?;
            let next_module = Module::Alignment(alignment_value);
            // The cell -has to be writable-
            if !module.can_overwrite_with(&next_module) {
                return Err(QrError::WriteError {
                    reason: format!(
                        "Could not overwrite module: {:?} during alignment placement at i: {i}, j: {j}",
                        module
                    ),
                });
            }
            *module = next_module;
            p += 1;
        }
    }
    Ok(())
}

// This assumes the accumulator is sent in counting from 0 and that it tracks the "written" cells;
// 6, 7, 8, 11, 13, 16, 17, 18 are all white (false)
fn get_alignment_module_value(acc: usize) -> bool {
    // Hopefully this compiles to a LUT.
    // If speed is ever an issue, consider making a LUT in tables.rs.
    !([6, 7, 8, 11, 13, 16, 17, 18].contains(&acc))
}

// https://www.thonky.com/qr-code-tutorial/format-version-information#put-the-format-string-into-the-qr-code
// It's easiest to just hardcode the coordinates for the format info
// do lsb (14 - thonky diagram) -> msb(0).
// Coordinates are (i, j) for the top left corner -> the others can be computed via info the comments below.
const FORMAT_INFO_COORDINATES: [(usize, usize); 15] = [
    (0, 8), // 14 LSB -> RHS of matrix @ (j, side_length - i - 1)
    (1, 8), // 13
    (2, 8), // 12
    (3, 8), // 11
    (4, 8), // 10
    (5, 8), // 9
    (7, 8), // 8
    (8, 8), // 7 -- End RHS
    (8, 7), // 6 -- Start bottom, is @ (side_length - 7 + i - 8 , i)
    (8, 5), // 5
    (8, 4), // 4
    (8, 3), // 3
    (8, 2), // 2
    (8, 1), // 1
    (8, 0), // 0
];

// Needs to encircle the top left finder but not overlap the timing
// Bottom left format: bits are written under the dark bit down to the bottom.
// (i.e.) column to the right of the bottom left finder, 1 module right of the separator.
//
// Top Right format: bits are reserved under the format square, 1 below the separator row.

// THIS FUNCTION STILL NEEDS TESTING.
pub(crate) fn emplace_format_information_area(
    matrix: &mut SquareMatrix<Module>,
    ecc_level: ECCLevel,
    mask_pattern: MaskPattern,
) -> Result<()> {
    let side_length = matrix.side_length();
    // NOTE: FORMAT STRINGS ARE PRECOMPUTED AND CAN BE LOOKED UP.
    // IDX IS ECC_CAPACITY_IDX * 8 + MASK NO;
    //
    // All that's needed is to emplace the format bits
    let table_idx = ecc_level.capacity_idx() * MAX_NUM_MASK_PATTERNS + mask_pattern as usize;

    let format_bitstring = FORMAT_INFO_STRINGS[table_idx];
    for (i, coordinate) in FORMAT_INFO_COORDINATES.iter().enumerate() {
        let mask = (1 << i) as u16;
        // This should be a bit a bit.
        let write_value = ((format_bitstring & mask) >> i) as u8;

        // TODO: refactor into result; for now assertions are fine.
        // This should have either the msb at 0 (if 0, i.e. no msb), or 1 if it's 1
        if !(0u32..=1).contains(&find_msb(write_value as u32)) {
            return Err(QrError::WriteError {
                reason: format!(
                    "Invalid MSB during format emplacement: {write_value}, should be 0/1."
                ),
            });
        }

        let write_bit = write_value & 1 == 1;
        let write_module = Module::Format(write_bit);

        // Get the next coordinate.
        let (i_0, j_0) = *coordinate;
        let lhs_module = matrix.get_mut(i_0, j_0).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i_0}, j: {j_0} in format emplacement."),
        })?;
        // These bits -have- to be writable, otherwise we're pointing at a function pattern,
        // meaning the version coordinates are wrong.
        if !lhs_module.writable() {
            return Err(QrError::WriteError {
                reason: format!(
                    "Cannot overwrite module: {:?} at i: {i_0}, j: {j_0} in format emplacement.",
                    lhs_module
                ),
            });
        }

        *lhs_module = write_module;

        // Handle Bottom/RHS:
        // j_0 == 8 means we're writing the rhs
        let (i_1, j_1) = if j_0 == 8 {
            // Use i here, i_0 doesn't hit 6 on LHS.
            (j_0, side_length - i - 1)
        }
        // Otherwise, we're writing the bottom.VERSION_
        else {
            (side_length - 7 + i - 8, i_0)
        };

        let other_module = matrix.get_mut(i_1, j_1).ok_or(QrError::SampleError {
            reason: format!("Invalid read at i: {i_1}, j: {j_1} during format emplacement."),
        })?;

        if !other_module.writable() {
            return Err(QrError::WriteError {
                reason: format!(
                    "Cannot overwrite module: {:?} at i: {i_1}, j: {j_1} in format emplacement.",
                    other_module
                ),
            });
        }

        *other_module = write_module;
    }
    Ok(())
}

// Do LSB to MSB.
// 0 -> 17 (shifts)

// Adjacent to the bottom left and top right finder.
//  -->
// (i: 3 x j: 6) version information block above bottom left,
//  | msb: 00 | 03 | 06 | 09 | 12 | 15 |
//  ------------------------------------
//  |    01   | 04 | 07 | 10 | 13 | 16 |
//  ------------------------------------
//  |    02   | 05 | 08 | 11 | 14 | 17 |
// (i: 6 x j: 3) version information to the left of the top right (ie. is the transpose)
//
//  | msb: 00 | 01 | 02 |
//  ---------------------
//  |   03    | 04 | 05 |
//  ---------------------
//  |   06    | 07 | 08 |
//  ---------------------
//  |   09    | 10 | 11 |
//  ---------------------
//  |   12    | 13 | 14 |
//  ---------------------
//  |   15    | 16 | 17 |
//
// These are directly adjacent to format square + separators, which are 8 bits in length
// So, the offset is 8 + smallest matrix dimesion (3) = 11

const VERSION_MATRIX_OFFSET: usize = 11;

// This is easiest to handle with a double loop.
// for i in 0..6 {
//   for j in 0..3 {
//      // bit idx = i * 3 + j;
//      ... and so on.
//      Bottom Left table is (side_length - OFFSET + j, i)
//      Top Right table is (i, side_length - OFFSET + j)
//   }
// }

pub(crate) fn emplace_version_information(
    matrix: &mut SquareMatrix<Module>,
    version: usize,
) -> Result<()> {
    // NOTE: VERSION BITSTRINGS ARE PRECOMPUTED AND CAN BE LOOKED UP.
    // TABLE IDX is version - 7;
    if version < 7 {
        return Err(QrError::InvalidVersion);
    }

    // These are tested and should be assumed to be correct.
    let version_string = VERSION_INFO_STRINGS[version - 7];
    let side_length = matrix.side_length();
    // emplace the version bits like so.
    for i in 0..6 {
        for j in 0..3 {
            let bit_idx = i * 3 + j;
            let bit_mask = 1 << bit_idx;
            let write_value = ((version_string & bit_mask) >> bit_idx) as u8;
            let write_bit = (write_value & 1) == 1;
            let write_module = Module::Version(write_bit);

            // Insertion pointer.
            let p = side_length - VERSION_MATRIX_OFFSET + j;

            // Bottom left side:
            let bottom_module = matrix.get_mut(p, i).ok_or(QrError::SampleError {
                reason: format!(
                    "Invalid read at i: {p}, j: {i} during bottom version emplacement."
                ),
            })?;
            // Ensure it's writable -> if it's not, we've hit a reserved spot and I've done
            // something wrong.
            if !bottom_module.writable() {
                return Err(QrError::WriteError {
                    reason: format!(
                        "Could not overwrite module: {:?} at i: {p}, j: {i} during bottom version emplacement.",
                        bottom_module
                    ),
                });
            }

            *bottom_module = write_module;

            // Top Right side:
            let top_module = matrix.get_mut(i, p).ok_or(QrError::SampleError {
                reason: format!("Invalid read at i: {i}, j: {p} during top version emplacement."),
            })?;
            // Again, assert writable invariant and return Err if not correct.
            if !top_module.writable() {
                return Err(QrError::WriteError {
                    reason: format!(
                        "Could not overwrite module: {:?} at i: {i}, j: {p} during top version emplacement.",
                        top_module
                    ),
                });
            }
            *top_module = write_module;
        }
    }
    Ok(())
}

pub(crate) fn emplace_data_bits(
    matrix: &mut SquareMatrix<Module>,
    codewords: &BitVec<u8, Msb0>,
    mask_pattern: MaskPattern,
) -> Result<()> {
    const TIMING_IDX: usize = 6;
    let side_length = matrix.side_length();
    let mut direction = -1;
    let mut row = (side_length - 1) as i32;
    let mut column = side_length - 1;

    let mut bit_idx = 0;

    // Columns are written in 2's, so we can stop when the column
    // index pointer == 0
    while column > 0 {
        // Skip the timing column
        if column == TIMING_IDX {
            column -= 1;
        }

        // Up and down part of the zig-zag
        while row >= 0 && row < side_length as i32 {
            // Skip the timing row.
            if row == TIMING_IDX as i32 {
                row += direction;
            }

            // Right to left part of the zig-zag
            // Index the table at (row, j)
            for k in 0..2 {
                let j = column - k;
                let module = matrix
                    .get_mut(row as usize, j)
                    .ok_or(QrError::SampleError {
                        reason: format!(
                            "Invalid read at i: {row}, j: {j} during data bit emplacement."
                        ),
                    })?;

                if !module.writable() {
                    continue;
                }

                // The remainder bits are
                // Write to the matrix at (row, j):
                // Get the bit value.
                let bit_val = *codewords
                    .get(bit_idx)
                    .as_deref()
                    .ok_or(QrError::WriteError {
                        reason: format!(
                            "Failed to get bit: {bit_idx} from bitvec of size: {} during data bit emplacement.",
                            codewords.len()
                        ),
                    })?;

                // Check if we mask.
                let write_value = if mask_pattern.should_mask(row as usize, j) {
                    !bit_val
                } else {
                    bit_val
                };

                // Write the module
                let write_module = Module::Data(write_value);
                *module = write_module;
                // Increment the bit index pointer and move onto the next write.
                bit_idx += 1;
            }

            row += direction;
        }

        // Negate the direction to flip row traversal.
        direction = -direction;
        row += direction;
        // Bump the column over by two.
        // Saturate the subtraction to avoid OOB -> the loop will still break.
        column = column.saturating_sub(2);
    }

    // Assert invariants:
    // Bit_idx => Should end at codewords.len()
    // column => ends at 0
    // row => ends at side_length - 1
    // directions is now going up => is negative.
    if column != 0 {
        return Err(QrError::WriteError {
            reason: format!(
                "Column invariant violated. Should end at 0 after data emplacement: {column}."
            ),
        });
    }

    if row != (side_length - 1) as i32 {
        return Err(QrError::WriteError {
            reason: format!(
                "Row invariant violated. Should end at {} after data emplacement: {row}.",
                side_length - 1
            ),
        });
    }

    if bit_idx != codewords.len() {
        return Err(QrError::WriteError {
            reason: format!(
                "Bit idx invariant violated. Should end at {} after data emplacement: {bit_idx}",
                codewords.len()
            ),
        });
    }

    if direction.is_positive() {
        return Err(QrError::WriteError{
            reason: 
                "Direction invariant violated. Should be negative (up) after data emplacement but was positive (down)".to_string()
                        
        });
    }
    Ok(())
}
