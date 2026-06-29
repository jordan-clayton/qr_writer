use crate::ecc::ECCLevel;
use crate::encoding::get_mode_idx;
use crate::tables::*;

use crate::errors::{QrError, Result};

pub(crate) fn get_min_required_version(
    num_chars: usize,
    mode: u8,
    ecc_level: ECCLevel,
) -> Result<u8> {
    // get_mode_idx might best be served by a table) in tables.rs.
    // TODO: cleanup refactoring.
    let mode_idx = get_mode_idx(mode);
    let capacity_idx = ecc_level.capacity_idx();
    for version in 0..40 {
        // i * 16, + j * 4 + k
        let idx = version * 16 + capacity_idx * 4 + mode_idx;
        // TODO: determine how best to deal with this.
        if CHAR_CAPACITIES[idx] >= (num_chars as u16) {
            return Ok(version as u8 + 1);
        }
    }
    Err(QrError::VersionResolution {
        data_len: num_chars,
        ecc_level,
    })
}

// NOTE: version is expected to be >=1 here.
pub(crate) fn version_can_fit_data(
    version: usize,
    num_chars: usize,
    mode: u8,
    ecc_level: ECCLevel,
) -> Result<bool> {
    if version == 0 {
        return Err(QrError::InvalidVersion);
    }
    let mode_idx = get_mode_idx(mode) as usize;
    let capacity_idx = ecc_level.capacity_idx() as usize;
    let version_idx = version - 1;

    let table_idx = version_idx * 16 + capacity_idx * 4 + mode_idx;
    Ok(CHAR_CAPACITIES[table_idx] >= num_chars as u16)
}
