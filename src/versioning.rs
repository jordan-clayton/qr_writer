use crate::ecc::ECCLevel;
use crate::encoding::get_mode_idx;
use crate::tables::*;

// TODO TWICE: this file has only one function -> move it to encoding module.
pub fn get_min_required_version(num_chars: usize, mode: u8, ecc_level: ECCLevel) -> u8 {
    // get_mode_idx might best be served by a table) in tables.rs.
    // TODO: cleanup refactoring.
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

    // NOTE: this should actually fail if the loop completes without picking a version.
    // TODO: refactor this into result once errors have been designed.
    unreachable!(
        "Invalid number of characters: {num_chars} for mode: {mode} at Ec level: {:?}",
        &ecc_level
    );
}
