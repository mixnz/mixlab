//! The running executable as a crash report needs it: where it is loaded, which addresses are its
//! own, and which build it is — roadmap task **T91a**.
//!
//! A crash report records each frame as an **offset into the executable** rather than as a name,
//! and the release keeps the daemon's symbols beside the download instead of inside it. Turning an
//! offset back into a name needs three facts about the image the report came from, and every OS
//! keeps them in a different place: the program headers and a `PT_NOTE` on Linux, the load
//! commands and `LC_UUID` on macOS, the PE headers and the CodeView record on Windows. See
//! `docs/specs/2026-10-01-a-crash-report-names-its-frames-after-the-fact-design.md` and
//! [ADR 0060](../../../docs/decisions/0060-a-crash-report-carries-offsets-and-the-release-keeps-the-symbols.md).
//!
//! **Read once, before the panic hook is installed.** [`current`] walks headers and allocates; the
//! hook calls only [`return_addresses`] and [`Image::offset_of`], which do neither beyond the
//! vector the caller sized.

use crate::sys::crash_image as sys;

/// Which object format an [`Image`] is, which decides what its offsets are relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// Linux. An offset is a link-time virtual address: the address minus the load bias.
    Elf,

    /// macOS. An offset is a link-time address, `__TEXT`'s `0x100000000` included: the address
    /// minus the dyld slide.
    MachO,

    /// Windows. An offset is a relative virtual address: the address minus the module's base.
    Pe,
}

/// The running executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// What an address is measured from to become an offset — see [`ImageKind`].
    pub base: usize,

    /// The first address that belongs to the executable's code.
    pub start: usize,

    /// One past the last.
    pub end: usize,

    /// The build identifier a symbol file is matched against, in the form each platform's own
    /// tools print it: the GNU build-id as lowercase hex, `LC_UUID` as uppercase hyphenated hex,
    /// and the CodeView GUID and age as one uppercase hex string, the form a symbol server keys on.
    /// [`None`] when the executable carries none, or it could not be read.
    pub build_id: Option<String>,

    /// The object format.
    pub kind: ImageKind,
}

impl Image {
    /// The offset of `address` into the executable, or [`None`] when it is not the executable's —
    /// a frame in `libc`, `ntdll` or a system framework.
    #[must_use]
    pub fn offset_of(&self, address: usize) -> Option<usize> {
        (self.start..self.end)
            .contains(&address)
            .then(|| address - self.base)
    }
}

/// The running executable, or [`None`] when this platform could not say.
#[must_use]
pub fn current() -> Option<Image> {
    sys::current()
}

/// The return addresses of the calling thread's stack, innermost first, appended to `into` up to
/// `max` of them.
///
/// **No allocation when `into` already has room for `max` more**, which is how the panic hook
/// calls it: the hook runs on a thread that may hold the allocator's own lock.
pub fn return_addresses(into: &mut Vec<usize>, max: usize) {
    sys::return_addresses(into, max);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[inline(never)]
    fn a_function_of_this_test() -> usize {
        a_function_of_this_test as fn() -> usize as usize
    }

    fn image() -> Image {
        current().expect("every supported platform describes its own executable")
    }

    /// The executable's range holds its own code — a function of this very test.
    #[test]
    fn the_range_holds_a_function_of_this_binary() {
        let image = image();
        let here = a_function_of_this_test();

        assert!(image.start < image.end, "{image:?}");
        assert!(
            image.offset_of(here).is_some(),
            "{here:#x} is outside {:#x}..{:#x}",
            image.start,
            image.end
        );
    }

    /// Every executable these three toolchains link carries an identifier: the toolchain default on
    /// Linux, always on macOS, and always on Windows because rustc always asks MSVC's linker for a
    /// `.pdb`.
    #[test]
    fn the_executable_has_a_build_id() {
        let id = image().build_id.expect("a build identifier");

        assert!(!id.is_empty());
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
            "{id}"
        );
    }

    /// The unwind reaches code of this binary, and stops at the limit it was given.
    #[test]
    fn the_stack_starts_inside_this_binary_and_respects_the_limit() {
        let image = image();
        let mut addresses = Vec::with_capacity(64);

        return_addresses(&mut addresses, 64);

        assert!(!addresses.is_empty());
        assert!(addresses.len() <= 64);
        assert!(
            addresses
                .iter()
                .any(|&address| image.offset_of(address).is_some()),
            "{addresses:x?} against {:#x}..{:#x}",
            image.start,
            image.end
        );

        let mut two = Vec::with_capacity(2);
        return_addresses(&mut two, 2);
        assert_eq!(two.len(), 2);
    }

    /// An address outside the range has no offset, and one inside is measured from the base.
    #[test]
    fn an_offset_is_measured_from_the_base() {
        let image = Image {
            base: 0x1000,
            start: 0x2000,
            end: 0x3000,
            build_id: None,
            kind: ImageKind::Elf,
        };

        assert_eq!(image.offset_of(0x2010), Some(0x1010));
        assert_eq!(image.offset_of(0x1fff), None);
        assert_eq!(image.offset_of(0x3000), None);
    }
}
