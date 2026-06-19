use crate::ecc::ECCLevel;
// Again, consider folding this all into lib.rs.
use crate::encoding::get_mode_idx;
use crate::tables::*;

pub fn get_min_required_version(num_chars: usize, mode: u8, ecc_level: ECCLevel) -> u8 {
    let mode_idx = get_mode_idx(mode);
    let capacity_idx = ecc_level.capacity_idx();
    for version in 0..40 {
        // i * 16, + j * 4 + k
        let idx = version * 16 + capacity_idx * 4 + mode_idx;
        // TODO: determine how best to deal with this.
        if CHAR_CAPACITIES[idx] >= (num_chars as u16) {
            return version as u8 + 1;
        }
    }

    40
}
