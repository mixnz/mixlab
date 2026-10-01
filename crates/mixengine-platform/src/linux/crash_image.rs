//! The running executable on Linux — T91a.
//!
//! `dl_iterate_phdr` visits the main program first: its load bias is what an address is measured
//! from, its `PT_LOAD` segments are its range, and its `PT_NOTE` segments hold the GNU build-id
//! that `readelf -n` prints.

use std::ffi::c_void;

use crate::crash_image::{Image, ImageKind};

pub(crate) use crate::unix::unwind::return_addresses;

/// `NT_GNU_BUILD_ID`.
const NT_GNU_BUILD_ID: u32 = 3;

pub(crate) fn current() -> Option<Image> {
    let mut found: Option<Image> = None;

    #[expect(
        unsafe_code,
        reason = "the callback matches `dl_iterate_phdr`'s signature, and the argument is a pointer \
                  to a local that outlives the call"
    )]
    unsafe {
        libc::dl_iterate_phdr(Some(first_object), (&raw mut found).cast::<c_void>());
    }

    found
}

/// Describe the first object the loader reports, which is the main program, and stop.
extern "C" fn first_object(
    info: *mut libc::dl_phdr_info,
    _size: libc::size_t,
    argument: *mut c_void,
) -> libc::c_int {
    #[expect(
        unsafe_code,
        reason = "`argument` is the `&mut Option<Image>` `current` passed; `info` is the loader's, \
                  valid for this call"
    )]
    let (found, info) = unsafe { (&mut *argument.cast::<Option<Image>>(), &*info) };

    let base = info.dlpi_addr as usize;
    let mut start = usize::MAX;
    let mut end = 0usize;
    let mut build_id = None;

    for index in 0..usize::from(info.dlpi_phnum) {
        #[expect(
            unsafe_code,
            reason = "`dlpi_phdr` points at `dlpi_phnum` program headers, and `index` is below that"
        )]
        let header = unsafe { &*info.dlpi_phdr.add(index) };

        let at = base + header.p_vaddr as usize;
        let size = header.p_memsz as usize;

        if header.p_type == libc::PT_LOAD {
            start = start.min(at);
            end = end.max(at + size);
        } else if header.p_type == libc::PT_NOTE && build_id.is_none() {
            #[expect(
                unsafe_code,
                reason = "a `PT_NOTE` segment is loaded, so its `p_memsz` bytes at the biased \
                          address are mapped and readable for the life of the process"
            )]
            let notes = unsafe { std::slice::from_raw_parts(at as *const u8, size) };
            build_id = gnu_build_id(notes);
        }
    }

    if start < end {
        *found = Some(Image {
            base,
            start,
            end,
            build_id,
            kind: ImageKind::Elf,
        });
    }

    // Non-zero ends the iteration: the main program is the only object wanted.
    1
}

/// The `NT_GNU_BUILD_ID` note in a run of notes, as lowercase hex, or [`None`] without one.
///
/// Each note is `namesz`, `descsz` and `type` as 32-bit words, then the name and the descriptor,
/// each padded to four bytes.
fn gnu_build_id(mut notes: &[u8]) -> Option<String> {
    fn word(bytes: &[u8], at: usize) -> Option<usize> {
        let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_ne_bytes(raw)).ok()
    }
    fn padded(length: usize) -> usize {
        length.div_ceil(4) * 4
    }

    while notes.len() >= 12 {
        let name_size = word(notes, 0)?;
        let descriptor_size = word(notes, 4)?;
        let kind = u32::try_from(word(notes, 8)?).ok()?;

        let name_at = 12;
        let descriptor_at = name_at + padded(name_size);
        let next = descriptor_at + padded(descriptor_size);

        let name = notes.get(name_at..name_at + name_size)?;
        let descriptor = notes.get(descriptor_at..descriptor_at + descriptor_size)?;

        if kind == NT_GNU_BUILD_ID && name == b"GNU\0" && !descriptor.is_empty() {
            return Some(
                descriptor
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            );
        }

        notes = notes.get(next..)?;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(name: &[u8], kind: u32, descriptor: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(name.len()).expect("small").to_ne_bytes());
        bytes.extend_from_slice(
            &u32::try_from(descriptor.len())
                .expect("small")
                .to_ne_bytes(),
        );
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(name);
        bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        bytes.extend_from_slice(descriptor);
        bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        bytes
    }

    /// The build-id is found after a note of another kind, and printed as `readelf -n` prints it.
    #[test]
    fn the_build_id_is_found_among_other_notes() {
        let mut notes = note(
            b"GNU\0",
            1,
            &[0, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        );
        notes.extend(note(
            b"GNU\0",
            NT_GNU_BUILD_ID,
            &[0xde, 0xad, 0xbe, 0xef, 0x01],
        ));

        assert_eq!(gnu_build_id(&notes).as_deref(), Some("deadbeef01"));
    }

    /// A note truncated by a bad size ends the search rather than reading past the segment.
    #[test]
    fn a_truncated_note_is_no_build_id() {
        let mut notes = note(b"GNU\0", NT_GNU_BUILD_ID, &[1, 2, 3, 4]);
        notes.truncate(notes.len() - 2);

        assert_eq!(gnu_build_id(&notes), None);
    }
}
