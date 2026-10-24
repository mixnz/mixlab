//! Linux: the C library's own answer, and what else the loader knows.
//!
//! Every Linux build of MixEngine targets `-gnu`, so the running process is itself proof that a
//! glibc is here; the only question is which.

use std::collections::BTreeSet;

use crate::{Machine, MachineFacts, Probe, avx, dotted_version, shared_libraries};

/// This system's answer.
#[derive(Debug, Default)]
pub(crate) struct Facts;

impl Machine for Facts {
    fn facts(&self) -> MachineFacts {
        MachineFacts {
            glibc: glibc(),
            avx: avx(),
            shared_libraries: listed(),
            ..MachineFacts::unknown()
        }
    }
}

fn glibc() -> Probe<String> {
    #[expect(
        unsafe_code,
        reason = "gnu_get_libc_version takes nothing and returns a pointer to a static, \
                  NUL-terminated string the C library owns for the life of the process"
    )]
    let text = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };

    text.to_str().map_or(Probe::Unknown, dotted_version)
}

/// What the loader's cache lists, from the first place a distribution keeps `ldconfig`.
///
/// **Named absolutely, and three of them**: a user's `PATH` usually has no `sbin`, and Debian,
/// Fedora and Arch do not agree on which one it is. A machine with none is [`Probe::Unknown`], which
/// is a machine nothing is warned about rather than one everything is.
///
/// **Not `dlopen`**: loading X11 or ALSA into the daemon to find out whether they are there would
/// run their constructors inside a process that has no business doing it.
fn listed() -> Probe<BTreeSet<String>> {
    for ldconfig in ["/sbin/ldconfig", "/usr/sbin/ldconfig", "/usr/bin/ldconfig"] {
        let Ok(output) = std::process::Command::new(ldconfig).arg("-p").output() else {
            continue;
        };

        if !output.status.success() {
            return Probe::Unknown;
        }

        return shared_libraries(
            &String::from_utf8_lossy(&output.stdout),
            std::env::consts::ARCH,
        );
    }

    Probe::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_names_its_c_library() {
        assert!(matches!(glibc(), Probe::Present(_)), "{:?}", glibc());
    }

    /// **About this machine, and the one thing every Linux has**: the C library the test itself is
    /// linked against is in the loader's cache, or the cache was not read at all.
    #[test]
    fn this_machine_lists_the_library_it_is_running_on() {
        match listed() {
            Probe::Present(found) => assert!(
                found.iter().any(|soname| soname.starts_with("libc.so")),
                "{found:?}"
            ),
            other => panic!("a glibc machine has a loader cache: {other:?}"),
        }
    }
}
