//! The running executable on macOS — T91a.
//!
//! dyld's image 0 is the main program. Its slide is what an address is measured from, its `__TEXT`
//! segment is its code, and its `LC_UUID` is the identifier `dwarfdump --uuid` prints and a
//! symbol file is matched against.

use crate::crash_image::{Image, ImageKind};

pub(crate) use crate::unix::unwind::return_addresses;

/// `MH_MAGIC_64`: every executable this product ships on macOS is 64-bit.
const MH_MAGIC_64: u32 = 0xfeed_facf;
/// `LC_SEGMENT_64`.
const LC_SEGMENT_64: u32 = 0x19;
/// `LC_UUID`.
const LC_UUID: u32 = 0x1b;
/// `sizeof(struct mach_header_64)`.
const HEADER_SIZE: usize = 32;

#[expect(
    unsafe_code,
    reason = "the libc crate deprecates its own declarations of these two in favour of `mach2`, a \
              crate this one does not otherwise need; both are in libSystem, as <mach-o/dyld.h> \
              declares them"
)]
unsafe extern "C" {
    fn _dyld_get_image_header(image_index: u32) -> *const u8;
    fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
}

pub(crate) fn current() -> Option<Image> {
    #[expect(
        unsafe_code,
        reason = "both calls take an image index and read dyld's own tables; index 0 always exists \
                  in a running program"
    )]
    let (header, slide) = unsafe { (_dyld_get_image_header(0), _dyld_get_image_vmaddr_slide(0)) };
    if header.is_null() {
        return None;
    }

    let header = header.cast::<u8>();

    #[expect(
        unsafe_code,
        reason = "`header` is the main program's mapped `mach_header_64`, at least 32 bytes"
    )]
    let (magic, commands, commands_size) = unsafe {
        (
            header.cast::<u32>().read_unaligned(),
            header.add(16).cast::<u32>().read_unaligned(),
            header.add(20).cast::<u32>().read_unaligned(),
        )
    };
    if magic != MH_MAGIC_64 {
        return None;
    }

    #[expect(
        unsafe_code,
        reason = "the load commands follow the header and are `sizeofcmds` bytes long, mapped with it"
    )]
    let loads = unsafe {
        std::slice::from_raw_parts(
            header.add(HEADER_SIZE),
            usize::try_from(commands_size).ok()?,
        )
    };

    let slide = slide.cast_unsigned();
    let (text, build_id) = read_commands(loads, commands)?;

    Some(Image {
        base: slide,
        start: slide.wrapping_add(text.0),
        end: slide.wrapping_add(text.0).wrapping_add(text.1),
        build_id,
        kind: ImageKind::MachO,
    })
}

/// `__TEXT`'s link-time address and size, and the UUID, from a run of load commands.
fn read_commands(mut loads: &[u8], count: u32) -> Option<((usize, usize), Option<String>)> {
    fn word(bytes: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    }
    fn quad(bytes: &[u8], at: usize) -> Option<usize> {
        usize::try_from(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?)).ok()
    }

    let mut text = None;
    let mut uuid = None;

    for _ in 0..count {
        let kind = word(loads, 0)?;
        let size = usize::try_from(word(loads, 4)?).ok()?;
        if size < 8 {
            return None;
        }
        let command = loads.get(..size)?;

        if kind == LC_SEGMENT_64 && command.get(8..24)? == b"__TEXT\0\0\0\0\0\0\0\0\0\0" {
            text = Some((quad(command, 24)?, quad(command, 32)?));
        } else if kind == LC_UUID {
            uuid = Some(hyphenated(command.get(8..24)?));
        }

        loads = loads.get(size..)?;
    }

    Some((text?, uuid))
}

/// A UUID as `dwarfdump --uuid` prints it: uppercase, hyphenated 8-4-4-4-12.
fn hyphenated(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(kind: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(8 + body.len()).expect("small").to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    fn segment(name: &str, vmaddr: u64, vmsize: u64) -> Vec<u8> {
        let mut body = [0u8; 64];
        body[..name.len()].copy_from_slice(name.as_bytes());
        body[16..24].copy_from_slice(&vmaddr.to_le_bytes());
        body[24..32].copy_from_slice(&vmsize.to_le_bytes());
        command(LC_SEGMENT_64, &body)
    }

    /// `__TEXT` is found after `__PAGEZERO`, and the UUID is printed the way Apple's tools print it.
    #[test]
    fn text_and_uuid_are_read_from_the_load_commands() {
        let mut loads = segment("__PAGEZERO", 0, 0x1_0000_0000);
        loads.extend(segment("__TEXT", 0x1_0000_0000, 0x4000));
        loads.extend(command(LC_UUID, &(0u8..16).collect::<Vec<_>>()));

        let (text, uuid) = read_commands(&loads, 3).expect("a readable set of commands");

        assert_eq!(text, (0x1_0000_0000, 0x4000));
        assert_eq!(
            uuid.as_deref(),
            Some("00010203-0405-0607-0809-0A0B0C0D0E0F")
        );
    }

    /// A command whose size runs past the end is refused rather than read past.
    #[test]
    fn a_command_that_overruns_is_refused() {
        let mut loads = segment("__TEXT", 0x1_0000_0000, 0x4000);
        loads.truncate(loads.len() - 1);

        assert_eq!(read_commands(&loads, 1), None);
    }
}
