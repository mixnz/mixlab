//! Windows: the three checks, the held file, and the run.
//!
//! The steps are those of D6 in
//! `docs/specs/2026-09-16-t148-what-a-machine-lacks-is-installed-not-reported-design.md`.

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_ATTR_TYPE, CertGetNameStringW, szOID_ORGANIZATION_NAME,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_CHAIN, WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_READ, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use super::prompt::Apartment;
use crate::{
    Error, RedistributableOutcome, Redistributables, Result, VISUAL_CPP_PUBLISHER,
    names_the_redistributable,
};

/// The arguments that make it install without a window of its own. Windows' approval dialog is not
/// the installer's window and still appears.
const QUIET: &str = "/install /quiet /norestart";

/// This system's answer.
#[derive(Debug, Default)]
pub(crate) struct Installer;

impl Redistributables for Installer {
    fn install_visual_cpp(&self, installer: &Path) -> Result<RedistributableOutcome> {
        // **Held from before the first check until the installer has ended** (D6, step 3): a
        // read-only share means nobody can write to or replace the file in between, so the bytes
        // Windows vouched for are the bytes that run.
        let _held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(installer)
            .map_err(|source| Error::Io {
                action: "open",
                path: installer.to_path_buf(),
                source,
            })?;

        verify(installer)?;
        launch(installer)
    }
}

/// All three checks, in the order that makes each refusal the most useful sentence.
pub(crate) fn verify(installer: &Path) -> Result<()> {
    let path = wide(installer.as_os_str());
    let refuse = |reason: String| Error::NotTrusted {
        path: installer.to_path_buf(),
        reason,
    };

    let organisation = signer_organisation(&path).map_err(refuse)?;
    if organisation != VISUAL_CPP_PUBLISHER {
        return Err(refuse(format!(
            "it is signed by {organisation:?}, not by {VISUAL_CPP_PUBLISHER}"
        )));
    }

    match product_name(&path) {
        Some(product) if names_the_redistributable(&product) => Ok(()),
        Some(product) => Err(refuse(format!(
            "it names itself {product:?}, not the Visual C++ Redistributable"
        ))),
        None => Err(refuse("it carries no product name".to_owned())),
    }
}

/// The leaf signer's organisation, once Windows has accepted the signature and its whole chain.
fn signer_organisation(path: &[u16]) -> std::result::Result<String, String> {
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: u32::try_from(size_of::<WINTRUST_FILE_INFO>()).expect("a small struct"),
        pcwszFilePath: path.as_ptr(),
        hFile: std::ptr::null_mut(),
        pgKnownSubject: std::ptr::null_mut(),
    };

    let mut data = WINTRUST_DATA {
        cbStruct: u32::try_from(size_of::<WINTRUST_DATA>()).expect("a small struct"),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &raw mut file,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN,
        ..WINTRUST_DATA::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;

    #[expect(
        unsafe_code,
        reason = "`data` and `file` are locals whose pointers live for the call; no window is passed"
    )]
    let status = unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            &raw mut action,
            (&raw mut data).cast(),
        )
    };

    // Read while the verification state is still open; it is closed below whatever happened.
    let organisation = match status {
        0 => leaf_organisation(data.hWVTStateData),
        _ => None,
    };

    data.dwStateAction = WTD_STATEACTION_CLOSE;
    #[expect(
        unsafe_code,
        reason = "closing the state the call above opened, with the same locals"
    )]
    unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            &raw mut action,
            (&raw mut data).cast(),
        );
    }

    if status != 0 {
        return Err(format!(
            "Windows does not accept its signature (0x{:08X})",
            u32::from_ne_bytes(status.to_ne_bytes())
        ));
    }

    organisation.ok_or_else(|| "its signing certificate names no organisation".to_owned())
}

/// `O=` of the signing certificate, which is index 0 of the signer's chain.
fn leaf_organisation(state: HANDLE) -> Option<String> {
    #[expect(
        unsafe_code,
        reason = "`state` is the open verification state WinVerifyTrust returned"
    )]
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return None;
    }

    #[expect(
        unsafe_code,
        reason = "`provider` was checked non-null and belongs to the open state"
    )]
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return None;
    }

    #[expect(
        unsafe_code,
        reason = "`signer` was checked non-null and lives as long as the state"
    )]
    let signer = unsafe { &*signer };
    if signer.csCertChain == 0 || signer.pasCertChain.is_null() {
        return None;
    }

    #[expect(
        unsafe_code,
        reason = "the chain holds at least one entry, checked above"
    )]
    let leaf = unsafe { &*signer.pasCertChain };
    if leaf.pCert.is_null() {
        return None;
    }

    let oid = szOID_ORGANIZATION_NAME.cast::<c_void>();

    #[expect(unsafe_code, reason = "a length query: no buffer is written")]
    let length = unsafe {
        CertGetNameStringW(
            leaf.pCert,
            CERT_NAME_ATTR_TYPE,
            0,
            oid,
            std::ptr::null_mut(),
            0,
        )
    };
    if length <= 1 {
        return None;
    }

    let mut buffer = vec![0u16; usize::try_from(length).ok()?];

    #[expect(unsafe_code, reason = "`buffer` holds exactly `length` characters")]
    let written = unsafe {
        CertGetNameStringW(
            leaf.pCert,
            CERT_NAME_ATTR_TYPE,
            0,
            oid,
            buffer.as_mut_ptr(),
            length,
        )
    };

    buffer.truncate(usize::try_from(written.saturating_sub(1)).ok()?);
    Some(String::from_utf16_lossy(&buffer))
}

/// `ProductName` from the file's version resource, in the first language that has one.
fn product_name(path: &[u16]) -> Option<String> {
    let mut ignored = 0u32;

    #[expect(
        unsafe_code,
        reason = "a size query on a NUL-terminated path; writes only `ignored`"
    )]
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &raw mut ignored) };
    if size == 0 {
        return None;
    }

    let mut block = vec![0u8; usize::try_from(size).ok()?];

    #[expect(unsafe_code, reason = "`block` holds exactly `size` bytes")]
    let read = unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) };
    if read == 0 {
        return None;
    }

    // The translation table's length is in bytes: two `u16` per language.
    let (table, bytes) = query(&block, r"\VarFileInfo\Translation")?;
    let words = usize::try_from(bytes).ok()? / 2;

    #[expect(
        unsafe_code,
        reason = "`table` points into `block`, `words` u16 long, per VerQueryValueW"
    )]
    let words = unsafe { std::slice::from_raw_parts(table.cast::<u16>(), words) };

    words
        .as_chunks::<2>()
        .0
        .iter()
        .find_map(|[language, codepage]| {
            let (name, characters) = query(
                &block,
                &format!(r"\StringFileInfo\{language:04x}{codepage:04x}\ProductName"),
            )?;

            #[expect(
                unsafe_code,
                reason = "a string value's length is in characters, per VerQueryValueW"
            )]
            let name = unsafe {
                std::slice::from_raw_parts(name.cast::<u16>(), usize::try_from(characters).ok()?)
            };

            let name = String::from_utf16_lossy(name)
                .trim_end_matches('\0')
                .to_owned();
            (!name.is_empty()).then_some(name)
        })
}

/// One value out of a version block.
fn query(block: &[u8], sub_block: &str) -> Option<(*mut c_void, u32)> {
    let sub_block = wide(OsStr::new(sub_block));
    let mut pointer: *mut c_void = std::ptr::null_mut();
    let mut length = 0u32;

    #[expect(
        unsafe_code,
        reason = "`block` is a version block GetFileVersionInfoW filled"
    )]
    let found = unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            sub_block.as_ptr(),
            &raw mut pointer,
            &raw mut length,
        )
    };

    (found != 0 && !pointer.is_null() && length > 0).then_some((pointer, length))
}

/// Start it with the `open` verb, and wait for it to end.
///
/// **`open`, not `runas`** (D6, step 4): the installer asks Windows for elevation itself, exactly
/// as when somebody double-clicks it, so MixEngine never asks for a token on its behalf. Measured
/// in step zero: the bundle detects before it plans, and one with nothing to install ends `1638`
/// without raising the dialog at all.
fn launch(installer: &Path) -> Result<RedistributableOutcome> {
    let directory = installer.parent().unwrap_or_else(|| Path::new("\\"));
    let verb = wide(OsStr::new("open"));
    let file = wide(installer.as_os_str());
    let parameters = wide(OsStr::new(QUIET));
    let directory = wide(directory.as_os_str());

    // `ShellExecuteExW` documents an initialised apartment on the calling thread.
    let _apartment = Apartment::entered();

    #[expect(
        unsafe_code,
        reason = "SHELLEXECUTEINFOW is a plain C struct the API requires zeroed where unset"
    )]
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(size_of::<SHELLEXECUTEINFOW>()).expect("a small struct");
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.lpDirectory = directory.as_ptr();
    info.nShow = SW_HIDE;

    #[expect(
        unsafe_code,
        reason = "every pointer in `info` is a live local for the call"
    )]
    let started = unsafe { ShellExecuteExW(&raw mut info) };

    if started == 0 {
        let refusal = std::io::Error::last_os_error();
        if refusal.raw_os_error() == i32::try_from(ERROR_CANCELLED).ok() {
            return Ok(RedistributableOutcome::Declined);
        }
        return Err(Error::Os {
            action: "start the Visual C++ Redistributable installer",
            source: refusal,
        });
    }

    if info.hProcess.is_null() {
        return Err(Error::Os {
            action: "wait for the Visual C++ Redistributable installer",
            source: std::io::Error::other("Windows started it and handed back nothing to wait on"),
        });
    }

    #[expect(
        unsafe_code,
        reason = "`hProcess` came from ShellExecuteExW and is closed below"
    )]
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
    }

    let mut code = 0u32;
    #[expect(unsafe_code, reason = "as above; `code` is a local")]
    let read = unsafe { GetExitCodeProcess(info.hProcess, &raw mut code) };
    #[expect(unsafe_code, reason = "the handle is ours and is not used again")]
    unsafe {
        CloseHandle(info.hProcess);
    }

    if read == 0 {
        return Err(Error::Os {
            action: "read how the Visual C++ Redistributable installer ended",
            source: std::io::Error::last_os_error(),
        });
    }

    Ok(RedistributableOutcome::from_exit_code(code))
}

/// A NUL-terminated wide string.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("mixengine-t150-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a temporary directory");
        directory.join(name)
    }

    #[test]
    fn a_file_nobody_signed_is_not_believed() {
        let path = scratch("unsigned.exe");
        std::fs::write(&path, b"MZ this is not a program").expect("a file");

        let refused = verify(&path).expect_err("an unsigned file");
        assert!(matches!(refused, Error::NotTrusted { .. }), "{refused}");
    }

    /// Downloads 25 MB from Microsoft and never runs it. The release checklist runs this, the way
    /// `mixengine-core`'s `tests/index.rs` reaches the published index.
    #[test]
    #[ignore = "reaches Microsoft; run with --ignored from the release checklist"]
    fn microsofts_own_installer_is_believed_and_one_flipped_byte_is_not() {
        let path = scratch("vc_redist.x64.exe");
        let fetched = std::process::Command::new("curl.exe")
            .args(["-sSL", "-o"])
            .arg(&path)
            .arg("https://aka.ms/vs/17/release/vc_redist.x64.exe")
            .status()
            .expect("curl ships with Windows 10 and later");
        assert!(fetched.success());

        verify(&path).expect("Microsoft's installer, as Microsoft publishes it");

        let mut bytes = std::fs::read(&path).expect("the download");
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        let tampered = scratch("vc_redist.tampered.exe");
        std::fs::write(&tampered, bytes).expect("a copy");

        let refused = verify(&tampered).expect_err("one byte changed");
        assert!(matches!(refused, Error::NotTrusted { .. }), "{refused}");
    }
}
