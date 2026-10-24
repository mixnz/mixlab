//! Windows: the Visual C++ runtimes, from the keys their installer writes.
//!
//! **The 64-bit view**, where a measurement on 2026-09-16 found `…\Runtimes\x64`. The x86 key is
//! not read: no artifact the index publishes is x86.

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_WOW64_64KEY, REG_DWORD, REG_VALUE_TYPE,
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};

use crate::{Machine, MachineFacts, Probe, VisualCppVersion, avx, visual_cpp_from_registry};

/// Where the 2015-and-later redistributable records itself, relative to `HKEY_LOCAL_MACHINE`.
const RUNTIMES: &str = r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes";

/// This system's answer.
#[derive(Debug, Default)]
pub(crate) struct Facts;

impl Machine for Facts {
    fn facts(&self) -> MachineFacts {
        MachineFacts {
            visual_cpp_x64: runtime("x64"),
            visual_cpp_arm64: runtime("arm64"),
            avx: avx(),
            ..MachineFacts::unknown()
        }
    }
}

/// One architecture's runtime.
fn runtime(arch: &str) -> Probe<VisualCppVersion> {
    let key = match open(&format!(r"{RUNTIMES}\{arch}")) {
        Opened::Key(key) => key,
        Opened::Missing => return Probe::Absent,
        Opened::Failed => return Probe::Unknown,
    };

    let (installed, major, minor) = (
        dword(&key, "Installed"),
        dword(&key, "Major"),
        dword(&key, "Minor"),
    );

    if [&installed, &major, &minor]
        .iter()
        .any(|value| matches!(value, Value::Failed))
    {
        return Probe::Unknown;
    }

    visual_cpp_from_registry(installed.number(), major.number(), minor.number())
}

/// What opening a key came to.
enum Opened {
    Key(Key),
    Missing,
    Failed,
}

/// What reading one value came to.
enum Value {
    Number(u32),
    Missing,
    Failed,
}

impl Value {
    fn number(&self) -> Option<u32> {
        match self {
            Self::Number(number) => Some(*number),
            Self::Missing | Self::Failed => None,
        }
    }
}

fn open(path: &str) -> Opened {
    let path = wide(path);
    let mut handle: HKEY = std::ptr::null_mut();

    #[expect(
        unsafe_code,
        reason = "the registry has no safe binding in this tree; the call writes only `handle`"
    )]
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_WOW64_64KEY,
            &raw mut handle,
        )
    };

    match status {
        ERROR_SUCCESS => Opened::Key(Key(handle)),
        ERROR_FILE_NOT_FOUND => Opened::Missing,
        _ => Opened::Failed,
    }
}

fn dword(key: &Key, name: &str) -> Value {
    let name = wide(name);
    let mut kind: REG_VALUE_TYPE = 0;
    let mut value: u32 = 0;
    let mut bytes: u32 = u32::try_from(size_of::<u32>()).unwrap_or(4);

    #[expect(
        unsafe_code,
        reason = "the registry has no safe binding in this tree; the call writes only the three \
                  out-parameters below, all owned by this frame"
    )]
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            &raw mut kind,
            (&raw mut value).cast::<u8>(),
            &raw mut bytes,
        )
    };

    match status {
        ERROR_SUCCESS if kind == REG_DWORD => Value::Number(value),
        ERROR_FILE_NOT_FOUND => Value::Missing,
        _ => Value::Failed,
    }
}

/// An open key that closes itself.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        #[expect(
            unsafe_code,
            reason = "closing a handle this type owns; there is no safe binding for it"
        )]
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

/// A NUL-terminated wide string, which is what every `W` entry point wants.
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;

    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **About the code, not the runner**: a runner with the runtime and one without are both a
    /// pass. What may never come back on a supported machine is "could not tell".
    #[test]
    fn this_machine_answers_about_its_x64_runtime() {
        assert_ne!(Facts.facts().visual_cpp_x64, Probe::Unknown);
    }
}
