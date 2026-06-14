use crate::ecc::ECCLevel;
// Again, consider folding this all into lib.rs.
use crate::encoding::get_mode_idx;

// Per: https://www.thonky.com/qr-code-tutorial/character-capacities

// TODO: giant CC table, can be 1D array.
// Value is:
// (Version (-1) + capacity_idx) * 4 + mode_idx
// TODO: initialize this properly.
const CHAR_CAPACITIES: [u8; 40 * 4 * 4] = [0; 40 * 4 * 4];

pub fn get_min_required_version(num_chars: usize, mode: u8, ecc_level: ECCLevel) -> u8 {
    let mode_idx = get_mode_idx(mode);
    let capacity_idx = ecc_level.capacity_idx();

    for version in 0..40 {
        // (Version (-1) + capacity_idx) * 4 + mode_idx
        let idx = version + capacity_idx * 4 + mode_idx;
        // TODO: determine how best to deal with this.
        if CHAR_CAPACITIES[idx] <= (num_chars as u8) {
            return version as u8 + 1;
        }
    }

    40
}
