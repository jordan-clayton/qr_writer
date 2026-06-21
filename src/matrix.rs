use crate::tables::*;
use bitvec::prelude::*;
// NOTE TO SELF: matrices work in units of "modules" (using m to denote).
// These are similar to the concept of a "texel" or a matrix cell.

// Module side length, s: (((v-1) * 4) + 21), where v is the version (1-indexed)
// e.g Version 32: (((32 - 1) * 4) + 21) = 145m

// Drawing order
// 1. Timing        *Done
// 2. Finder        *Done
//  2_i. Separator  *Done
//  2_ii. Dark bit  *Done
// 3. Alignment     * Done
// 4. Version / Format:
//  4i. Version/Format can be drawn in either order.

// Technically this does not matter, really,
// but for reference:
// WHITE MODULE = 0 = false
// BLACK MODULE = 1 = true
#[derive(Copy, Clone, Debug)]
pub(crate) enum Module {
    Writable(bool),
    Finder(bool),
    // Technically separators are part of the finder pattern.
    Separator,
    Timing(bool),
    Alignment(bool),
    Format(bool),
    Version(bool),
    Dark,
}

impl Module {
    pub(crate) fn inner(&self) -> bool {
        match *self {
            Self::Writable(inner) => inner,
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
            // Version/format shouldn't be able to overwrite anything other than Writable.
            Self::Version(_) => matches!(with_module, Self::Version(_)),
            Self::Format(_) => matches!(with_module, Self::Format(_)),
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
pub(crate) struct SquareMatrix {
    data: Vec<Module>,
    side_length: usize,
}

impl SquareMatrix {
    // VERSION IS SUPPLIED 1-indexed
    pub(crate) fn new(n: usize) -> Self {
        // Create an empty all-black matrix that's fully writable.
        let data = vec![Default::default(); n * n];
        Self {
            data,
            side_length: n,
        }
    }

    pub(crate) fn side_length(&self) -> usize {
        self.side_length
    }
    // Returns the actual module cell (which holds state)
    // use .inner() to determine the value (white = false/black = true)
    pub(crate) fn get(&self, i: usize, j: usize) -> &Module {
        &self.data[self.side_length * i + j]
    }
    pub(crate) fn get_mut(&mut self, i: usize, j: usize) -> &mut Module {
        &mut self.data[self.side_length * i + j]
    }

    // Returns a vector of u8 (booleans cast to u8) and the side length.
    // This is mostly used for testing and may have to change if/when swapping to a bitfield.
    pub(crate) fn destructure_into_bytes(self) -> (Vec<u8>, usize) {
        let side_length = self.side_length;
        let mat = self.data.iter().map(|b| b.inner() as u8).collect();
        (mat, side_length)
    }
}

pub(crate) struct QRCodeMatrix {
    matrix: SquareMatrix,
    version: usize,
}

impl QRCodeMatrix {
    // Let the drawing routine happen in the constructor.
    pub(crate) fn new(version: usize, codewords: &BitVec<u8, Msb0>) -> Self {
        let matrix = draw_qr_code(version, codewords);
        Self { matrix, version }
    }
    pub(crate) fn version(&self) -> usize {
        self.version
    }
    pub(crate) fn matrix(&self) -> &SquareMatrix {
        &self.matrix
    }
}

// White module = 0 = false
// Black module = 1 = true
fn draw_qr_code(version: usize, codewords: &BitVec<u8, Msb0>) -> SquareMatrix {
    let n = (version - 1) * 4 + 21;
    let mut matrix = SquareMatrix::new(n);

    emplace_timing_patterns(&mut matrix);
    emplace_finder_patterns_into_blank_matrix(&mut matrix, version);

    todo!("Implement rest of QR drawing routine");
}

// ---- TIMING PATTERNS ---

// Since this -technically- doesn't need to happen before the other elements,
// this will not hard-assert.
// It's wiser to draw the timing before drawing the finder pattern though.
pub(crate) fn emplace_timing_patterns(matrix: &mut SquareMatrix) {
    let side_length = matrix.side_length();
    // Technically this can work on all matrices of side length 6 or greater, but
    // since this is for qr only, go with the minimum side length for a QR code.
    assert!(side_length >= 21);

    // Alternate dark-light, always starting dark.
    // i.e. even parity = dark.

    // The timing is 1-horizontal @ 6th (idx 6) row
    // and 6th column counting from 0.
    //
    // If a 1-module overdraw is ever a bottleneck, this could skip the row write on (6, 6).
    const FIXED_IDX: usize = 6;
    for p in 0..side_length {
        // Column write: i = 6
        let col_module = matrix.get_mut(FIXED_IDX, p);

        // Dark is on even parity
        // dark = true = 1
        let write_value = p & 1 != 1;
        let next_module = Module::Timing(write_value);
        if col_module.writable() || col_module.can_overwrite_with(&next_module) {
            *col_module = next_module;
        }

        // Row write: j = 6
        let row_module = matrix.get_mut(p, FIXED_IDX);

        if row_module.writable() || row_module.can_overwrite_with(&next_module) {
            *row_module = next_module;
        }
    }

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
pub(crate) fn emplace_finder_patterns_into_blank_matrix(matrix: &mut SquareMatrix, version: usize) {
    // The QR version 1 (the minimum) has side length 21
    assert!(matrix.side_length() >= 21);

    let side_length = matrix.side_length();
    // This does a little bit of overdraw (in addition to the timing patterns),
    // to cut down on the drawing complexity.
    // --- BLACK CELLS --- ->
    let inner_black_extent = side_length - MAX_SIDE_BLACK_FINDER_BITS;

    // TOP LEFT (0, 0), start at: (0, 0)
    emplace_black_finder_pattern_into_blank_matrix(0, 0, matrix);
    // BOTTOM LEFT (n-7, 0), start at: (n - 7, 0)
    emplace_black_finder_pattern_into_blank_matrix(inner_black_extent, 0, matrix);
    // TOP RIGHT (0, n-1), start at: (0, n-7)
    emplace_black_finder_pattern_into_blank_matrix(0, inner_black_extent, matrix);

    // --- WHITE CELLS ---
    let inner_white_extent = side_length - MAX_SIDE_WHITE_FINDER_BITS - 1;

    // TOP LEFT (0, 0), start at: (1, 1).
    emplace_white_finder_pattern_into_blank_matrix(1, 1, matrix);

    // BOTTOM LEFT (n-1, 0), start at: (n - 1 - 5, 1)
    emplace_white_finder_pattern_into_blank_matrix(inner_white_extent, 1, matrix);

    // TOP RIGHT (0, n-1), start at: (1, n-1 -5)
    emplace_white_finder_pattern_into_blank_matrix(1, inner_white_extent, matrix);

    // Write the separators
    // top_left starting indices: (0 + MAX_SIDE_SEPARATOR_OFFSET, 0)
    // bottom_left staring indices: (side_length - MAX_SIDE_SEPARATOR_BITS, 0)
    // top_right starting indices: (0 + MAX_SIDE_SEPARATOR_OFFSET, side_length - 1)
    let tl_start = (MAX_SIDE_SEPARATOR_OFFSET, 0);
    let bl_start = (side_length - MAX_SIDE_SEPARATOR_BITS, 0);
    let tr_start = (MAX_SIDE_SEPARATOR_OFFSET, side_length - 1);

    emplace_separator_bits(tl_start, bl_start, tr_start, matrix);
    // Write the dark bit

    // The dark bit will be at:
    // (8, [4 * version + 9]),
    // version = 1-indexed here.
    // i.e. the dark bit is 1 cell to the right of the bottom left finder pattern's top right
    // corner.

    const DARK_I: usize = 8;
    let dark_j = version * 4 + 9;
    let module = matrix.get_mut(DARK_I, dark_j);
    // Ensure we can write to the cell (i.e, we're not on a finder/separator)
    assert!(
        module.can_overwrite_with(&Module::Dark),
        "POINTER ARITHMETIC OFF WRITING DARK BIT."
    );

    *module = Module::Dark;
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
    matrix: &mut SquareMatrix,
) {
    // Index pointers
    let mut i = i0;
    let mut j = j0;
    let mut direction = Direction::Right;

    let mut written = 0;

    while written < TOTAL_WHITE_FINDER_BITS {
        *matrix.get_mut(i, j) = Module::Finder(false);
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

    // Assert the direction invariant
    // Since this writes precisely 16 bytes, this should still be "heading up"
    assert_eq!(direction, Direction::Up);
    // AND: i and j should both be i0, j0
    assert_eq!(i, i0);
    assert_eq!(j, j0);
}

// This is used to encode the state of the module cell to make zig-zagging a little bit easier.
fn emplace_black_finder_pattern_into_blank_matrix(i0: usize, j0: usize, matrix: &mut SquareMatrix) {
    for i in i0..i0 + MAX_SIDE_BLACK_FINDER_BITS {
        for j in j0..j0 + MAX_SIDE_BLACK_FINDER_BITS {
            let module = matrix.get_mut(i, j);
            let next_module = Module::Finder(true);
            // This will prevent the loop from overwriting the white inner ring if that's
            // accidentally called first.
            if module.writable() || module.can_overwrite_with(&next_module) {
                // Write to the matrix cell.
                *module = next_module;
            }
        }
    }
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
    matrix: &mut SquareMatrix,
) {
    // TOP LEFT, (top left corner should be (0, 0))
    // Traversal is: (right, up)
    let (mut i0, mut j0) = tl_bl;
    let mut i = i0;
    let mut j = j0;

    while j <= (j0 + MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j);
        assert!(
            module.can_overwrite_with(&Module::Separator),
            "POINTER ARITHMETIC OFF, TOP LEFT SEPARATOR LOOP, j\n\
            i0: {i0}, j0: {j0}, i: {i}, j: {j}, module: {:?}",
            std::mem::discriminant(module)
        );
        *module = Module::Separator;

        // TODO: delete this once the error is located.
        // Make sure the module at (i0, 7) is a Separator.
        assert!(
            matches!(matrix.get(i0, j), Module::Separator),
            "Cell did not write in loop: {:?} at i: {}, j: {}",
            matrix.get(i, j),
            i,
            j
        );

        j += 1;
    }

    // Correct j's index
    j -= 1;

    // Skip up one cell, (i, j) has already been written in the previous loop.
    i -= 1;
    while i >= (i0 - MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j);
        assert!(
            module.can_overwrite_with(&Module::Separator),
            "POINTER ARITHMETIC OFF, TOP LEFT SEPARATOR LOOP i\n\
            i0: {i0}, j0: {j0}, i: {i}, j: {j}, module: {:?}",
            std::mem::discriminant(module)
        );
        *module = Module::Separator;

        // Avoid overflow (after last write)
        if i == 0 {
            break;
        }
        i -= 1;
    }

    // The top right and top left have to end at 0
    assert_eq!(i, 0, "POINTER ARITHMETIC OFF, TOP LEFT i");

    // j ends at 7 (after 8 bits have been written)
    assert_eq!(
        j,
        MAX_SIDE_SEPARATOR_BITS - 1,
        "POINTER ARITHMETIC OFF, TOP LEFT j"
    );

    // BOTTOM LEFT:
    // Traversal is: (right, down)
    (i0, j0) = bl_tl;
    i = i0;
    j = j0;
    while j <= (j0 + MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j);
        assert!(
            module.can_overwrite_with(&Module::Separator),
            "POINTER ARITHMETIC OFF, BOTTOM LEFT SEPARATOR LOOP j\n\
            i0: {i0}, j0: {j0}, i: {i}, j: {j}, module: {:?}",
            std::mem::discriminant(module)
        );
        *module = Module::Separator;
        j += 1;
    }

    // Correct j's position
    j -= 1;

    // Skip down one cell.
    i += 1;
    while i <= (i0 + MAX_SIDE_SEPARATOR_OFFSET) {
        let module = matrix.get_mut(i, j);
        assert!(
            module.can_overwrite_with(&Module::Separator),
            "POINTER ARITHMETIC OFF, BOTTOM LEFT SEPARATOR LOOP i\n\
            i0: {i0}, j0: {j0}, i: {i}, j: {j}, module: {:?}",
            std::mem::discriminant(module)
        );
        *module = Module::Separator;
        i += 1;
    }

    // i ends + 1 without correction.
    assert_eq!(
        i,
        i0 + MAX_SIDE_SEPARATOR_BITS,
        "POINTER ARITHMETIC OFF, BOTTOM LEFT i"
    );

    // j ends in the proper position because it needs to be corrected.
    assert_eq!(
        j,
        j0 + MAX_SIDE_SEPARATOR_BITS - 1,
        "POINTER ARITHMETIC OFF, BOTTOM LEFT j"
    );

    // TOP_RIGHT:
    // Traversal is: (left, up)
    (i0, j0) = tr_br;
    i = i0;
    j = j0;

    while j >= j0 - MAX_SIDE_SEPARATOR_OFFSET {
        let module = matrix.get_mut(i, j);
        assert!(
            module.can_overwrite_with(&Module::Separator),
            "POINTER ARITHMETIC OFF, TOP RIGHT SEPARATOR LOOP j\n\
            i0: {i0}, j0: {j0}, i: {i}, j: {j}, module: {:?}",
            std::mem::discriminant(module)
        );

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
        let module = matrix.get_mut(i, j);
        assert!(
            module.can_overwrite_with(&Module::Separator),
            "POINTER ARITHMETIC OFF, TOP RIGHT SEPARATOR LOOP i\n\
            i0: {i0}, j0: {j0}, i: {i}, j: {j}, module: {:?}",
            std::mem::discriminant(module)
        );
        *module = Module::Separator;

        // Avoid overflow after last write
        if i == 0 {
            break;
        }

        i -= 1;
    }

    // The top right 0 has to be 0
    assert_eq!(i, 0, "POINTER ARITHMETIC OFF, TOP RIGHT i");

    // j should be MAX_SIDE_SEPARATOR_BITS away from its initial position.
    assert_eq!(
        j,
        j0 - (MAX_SIDE_SEPARATOR_BITS - 1),
        "POINTER ARITHMETIC OFF, TOP RIGHT j"
    );
}

// Lookups: version - 1;
pub(crate) fn emplace_alignment_squares(matrix: &mut SquareMatrix, version: usize) {
    // Escape early if it's version one, there are no alignment squares to place.
    if version == 1 {
        return;
    }

    // Look up the list of indices.
    let centers = ALIGNMENT_BLOCK_CENTERS[version - 1].inner();
    // Make sure there's at least two centres (i.e. not AlignmentCenters::Zero).
    assert!(centers.len() >= 2);

    // Produce a list of permutations (with repetitions) of centres.
    // Loop through each centre and test whether there's overlap (corner check).
    // ie. filter out centres that fail.

    for i in 0..centers.len() {
        for j in 0..centers.len() {
            let i_center = centers[i];
            let j_center = centers[j];
            let center = (i_center, j_center);
            let corners = Corners::new(center);
            // TODO: check whether can write, then pass the centre to the writing function.
            if can_write_alignment_square(&corners, matrix) {
                write_alignment_square(corners.top_left(), corners.bottom_right(), matrix);
            }
        }
    }
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
    pub(crate) fn new(center: (usize, usize)) -> Self {
        assert!(center.0 > 1);
        assert!(center.1 > 1);
        let top_left = (center.0 - 2, center.1 - 2);
        let top_right = (center.0 - 2, center.1 + 2);
        let bottom_left = (center.0 + 2, center.1 - 2);
        let bottom_right = (center.0 + 2, center.1 + 2);
        Self([top_left, top_right, bottom_left, bottom_right])
    }

    pub(crate) fn top_left(&self) -> (usize, usize) {
        self.0[0]
    }

    pub(crate) fn top_right(&self) -> (usize, usize) {
        self.0[1]
    }

    pub(crate) fn bottom_left(&self) -> (usize, usize) {
        self.0[2]
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

const ALIGNMENT_CENTRE_OFFSET: usize = 2;
pub(crate) fn can_write_alignment_square(corners: &Corners, matrix: &SquareMatrix) -> bool {
    for &(i, j) in corners {
        let module = matrix.get(i, j);
        // If we can write an alignment square at each of the 4 corners, then we can write an
        // alignmnent square. (it doesn't matter if we're writing true or false)
        if !module.can_overwrite_with(&Module::Alignment(false)) {
            return false;
        }
    }
    true
}

pub(crate) fn write_alignment_square(
    from: (usize, usize),
    to: (usize, usize),
    matrix: &mut SquareMatrix,
) {
    let mut p = 0;
    for i in from.0..=to.0 {
        for j in from.1..=to.1 {
            let alignment_value = get_alignment_module_value(p);
            let module = matrix.get_mut(i, j);
            let next_module = Module::Alignment(alignment_value);
            // The cell -has to be writable-
            assert!(module.can_overwrite_with(&next_module));
            *module = next_module;
            p += 1;
        }
    }
}

// This assumes the accumulator is sent in counting from 0 and that it tracks the "written" cells;
// 6, 7, 8, 11, 13, 16, 17, 18 are all white (false)
fn get_alignment_module_value(acc: usize) -> bool {
    // Hopefully this compiles to a LUT.
    // If speed is ever an issue, consider making a LUT in tables.rs.
    !([6, 7, 8, 11, 13, 16, 17, 18].contains(&acc))
}
