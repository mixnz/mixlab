//! A number of bytes, at the scale a download is read in.
use super::table::MISSING;

/// A number of bytes, at the scale a download is read in.
///
/// Whole mebibytes, and never a fraction: what this number answers is "will this take a while and is
/// there room", and `41 MiB` answers it exactly as well as `40.7 MiB` while being a number a person
/// takes in at a glance. `--json` carries the byte count, unrounded.
pub(super) fn size(bytes: u64) -> String {
    const MIB: u64 = 1 << 20;

    match bytes {
        0 => MISSING.to_owned(),
        // Anything smaller than a mebibyte would round to `0 MiB`, which reads as "nothing" for a
        // file that is really there.
        1..MIB => "< 1 MiB".to_owned(),
        _ => format!("{} MiB", bytes / MIB),
    }
}
