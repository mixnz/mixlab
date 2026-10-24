//! macOS: the product version the kernel reports.

use crate::{Machine, MachineFacts, Probe, avx, dotted_version};

/// This system's answer.
#[derive(Debug, Default)]
pub(crate) struct Facts;

impl Machine for Facts {
    fn facts(&self) -> MachineFacts {
        MachineFacts {
            macos: macos(),
            avx: avx(),
            ..MachineFacts::unknown()
        }
    }
}

fn macos() -> Probe<String> {
    let mut buffer = [0u8; 32];
    let mut length: libc::size_t = buffer.len();

    #[expect(
        unsafe_code,
        reason = "sysctlbyname writes at most `length` bytes into `buffer`, both owned by this frame, \
                  and the name is a NUL-terminated literal"
    )]
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &raw mut length,
            std::ptr::null_mut(),
            0,
        )
    };

    if status != 0 {
        return Probe::Unknown;
    }

    std::str::from_utf8(&buffer[..length.min(buffer.len())]).map_or(Probe::Unknown, dotted_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_mac_names_its_version() {
        assert!(matches!(macos(), Probe::Present(_)), "{:?}", macos());
    }
}
