//! The running executable on Windows — T91a.
//!
//! The module handle of the main program is its base, `SizeOfImage` from its PE headers is its
//! range, and the CodeView record in its debug directory carries the GUID and age the `.pdb` is
//! matched against, the same pair a symbol server keys on. The record also holds the path the
//! `.pdb` was written to on the build machine, which is never read here.

use std::ffi::c_void;

use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::crash_image::{Image, ImageKind};

/// `IMAGE_DEBUG_TYPE_CODEVIEW`.
const CODEVIEW: u32 = 2;
/// `IMAGE_DIRECTORY_ENTRY_DEBUG`.
const DEBUG_DIRECTORY: usize = 6;
/// `sizeof(IMAGE_DEBUG_DIRECTORY)`.
const DEBUG_ENTRY_SIZE: usize = 28;
/// The most frames one call to `RtlCaptureStackBackTrace` is documented to take on every Windows.
const PER_CALL: u32 = 62;

pub(crate) fn current() -> Option<Image> {
    #[expect(
        unsafe_code,
        reason = "a null name asks for the main program's own handle, which is never released"
    )]
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    if module.is_null() {
        return None;
    }

    let base = module as usize;
    let image = module.cast::<u8>().cast_const();
    let size = size_of_image(image)?;

    #[expect(
        unsafe_code,
        reason = "the whole image, `SizeOfImage` bytes from its base, is mapped for the life of the \
                  process; reads below stay inside it"
    )]
    let bytes = unsafe { std::slice::from_raw_parts(image, size) };

    Some(Image {
        base,
        start: base,
        end: base + size,
        build_id: codeview(bytes),
        kind: ImageKind::Pe,
    })
}

/// `SizeOfImage`, read through the DOS header's pointer to the PE headers.
fn size_of_image(image: *const u8) -> Option<usize> {
    #[expect(
        unsafe_code,
        reason = "the DOS header is at the module's base, and `e_lfanew` points at the PE headers \
                  inside the same mapping; `SizeOfImage` is at offset 56 of the optional header, \
                  which follows the 4-byte signature and the 20-byte file header"
    )]
    let size = unsafe {
        let pe = usize::try_from(image.add(0x3c).cast::<u32>().read_unaligned()).ok()?;
        image.add(pe + 4 + 20 + 56).cast::<u32>().read_unaligned()
    };

    usize::try_from(size).ok()
}

/// The CodeView record's GUID and age as one uppercase hex string, or [`None`] without one.
fn codeview(image: &[u8]) -> Option<String> {
    let word = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(image.get(at..at + 4)?.try_into().ok()?))
    };

    let pe = usize::try_from(word(0x3c)?).ok()?;
    let optional = pe + 4 + 20;
    // PE32+ (`0x20b`) is every target this product builds for Windows; its data directories start
    // at offset 112 of the optional header, eight bytes each.
    if image.get(optional..optional + 2)? != [0x0b, 0x02] {
        return None;
    }
    let directory = optional + 112 + DEBUG_DIRECTORY * 8;
    let entries_at = usize::try_from(word(directory)?).ok()?;
    let entries_size = usize::try_from(word(directory + 4)?).ok()?;

    for entry in (entries_at..entries_at + entries_size).step_by(DEBUG_ENTRY_SIZE) {
        if word(entry + 12)? != CODEVIEW {
            continue;
        }
        let record = usize::try_from(word(entry + 20)?).ok()?;
        if image.get(record..record + 4)? != b"RSDS" {
            continue;
        }

        let guid = image.get(record + 4..record + 20)?;
        let age = word(record + 20)?;
        let data1 = u32::from_le_bytes(guid[0..4].try_into().ok()?);
        let data2 = u16::from_le_bytes(guid[4..6].try_into().ok()?);
        let data3 = u16::from_le_bytes(guid[6..8].try_into().ok()?);
        let data4: String = guid[8..16]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect();

        return Some(format!("{data1:08X}{data2:04X}{data3:04X}{data4}{age:X}"));
    }

    None
}

/// See [`crate::crash_image::return_addresses`].
///
/// Never inlined, so that the one frame it skips is always its own.
#[inline(never)]
pub(crate) fn return_addresses(into: &mut Vec<usize>, max: usize) {
    let mut frames = [std::ptr::null_mut::<c_void>(); PER_CALL as usize];
    let limit = into.len().saturating_add(max);
    // One for this function's own frame, which is not the caller's stack.
    let mut skip = 1u32;

    while into.len() < limit {
        let wanted = u32::try_from(limit - into.len())
            .unwrap_or(PER_CALL)
            .min(PER_CALL);

        #[expect(
            unsafe_code,
            reason = "`frames` holds `PER_CALL` pointers and `wanted` is at most that; the hash \
                      out-parameter is optional and passed as null"
        )]
        let captured = unsafe {
            RtlCaptureStackBackTrace(skip, wanted, frames.as_mut_ptr(), std::ptr::null_mut())
        };
        if captured == 0 {
            break;
        }

        into.extend(
            frames[..usize::from(captured)]
                .iter()
                .map(|&frame| frame as usize),
        );
        skip += u32::from(captured);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic PE32+ with one CodeView entry: the GUID and age come out as the symbol-server
    /// string, and the path after them is never part of it.
    #[test]
    fn the_codeview_record_becomes_the_symbol_server_string() {
        let mut image = vec![0u8; 0x400];
        let pe = 0x80usize;
        image[0x3c..0x40].copy_from_slice(&u32::try_from(pe).expect("small").to_le_bytes());
        let optional = pe + 24;
        image[optional..optional + 2].copy_from_slice(&[0x0b, 0x02]);

        let entries = 0x200usize;
        let directory = optional + 112 + DEBUG_DIRECTORY * 8;
        image[directory..directory + 4]
            .copy_from_slice(&u32::try_from(entries).expect("small").to_le_bytes());
        image[directory + 4..directory + 8].copy_from_slice(
            &u32::try_from(DEBUG_ENTRY_SIZE)
                .expect("small")
                .to_le_bytes(),
        );

        let record = 0x300usize;
        image[entries + 12..entries + 16].copy_from_slice(&CODEVIEW.to_le_bytes());
        image[entries + 20..entries + 24]
            .copy_from_slice(&u32::try_from(record).expect("small").to_le_bytes());
        image[record..record + 4].copy_from_slice(b"RSDS");
        image[record + 4..record + 20].copy_from_slice(&[
            0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0x78, 0x56, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
            0x07, 0x08,
        ]);
        image[record + 20..record + 24].copy_from_slice(&3u32.to_le_bytes());
        image[record + 24..record + 40].copy_from_slice(b"C:\\builds\\x.pdb\0");

        assert_eq!(
            codeview(&image).as_deref(),
            // The age unpadded, as symbol servers spell it: `…0708` then `3`.
            Some("123456781234567801020304050607083")
        );
    }

    /// A PE32 optional header is not read as PE32+.
    #[test]
    fn a_pe32_image_has_no_build_id_here() {
        let mut image = vec![0u8; 0x200];
        image[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        image[0x80 + 24..0x80 + 26].copy_from_slice(&[0x0b, 0x01]);

        assert_eq!(codeview(&image), None);
    }
}
