//! Whether the account asking can answer macOS's trust dialog at this machine's screen.
//!
//! `add-trusted-cert -d` asks in a SecurityAgent dialog even when root runs it, and the helper
//! raises it in the caller's login session. That session has the screen only when the caller is
//! the one signed in at the console. Otherwise the dialog waits where nobody sees it, or macOS says
//! no user interaction was possible after the certificate is already in the keychain, untrusted.
//! So the helper asks this first and refuses at once, leaving the keychain as it was.
//!
//! Pure, and compiled everywhere so its tests run on any system; only macOS's helper calls it.

/// Refuse unless the console belongs to `caller`, a decimal uid.
///
/// `console` is the owner of `/dev/console`: the account signed in at the screen, or root (0) at the
/// login window when nobody is.
///
/// # Errors
///
/// The sentence a person reads: why the dialog would have nowhere to appear, and where to run the
/// grant instead.
#[cfg_attr(
    not(any(test, all(target_os = "macos", feature = "elevated"))),
    expect(dead_code, reason = "only macOS's helper asks this")
)]
pub(crate) fn callers_screen(console: u32, caller: &str) -> Result<(), String> {
    if console.to_string() == caller {
        return Ok(());
    }

    let who = match console {
        0 => "nobody is signed in at this Mac's screen".to_owned(),
        other => format!("the account signed in at this Mac's screen is uid {other}, not {caller}"),
    };

    Err(format!(
        "{who}, so macOS has nowhere to ask for the System keychain. Nothing was changed; run \
         `mix elevation grant` at the Mac, signed in as this account"
    ))
}

#[cfg(test)]
mod tests {
    use super::callers_screen;

    #[test]
    fn the_caller_at_the_console_can_answer() {
        assert_eq!(callers_screen(501, "501"), Ok(()));
    }

    /// The login window owns the console as root: the dialog would have nowhere to appear.
    #[test]
    fn nobody_signed_in_is_refused_before_anything_changes() {
        let said = callers_screen(0, "501").expect_err("nobody can answer");

        assert!(said.contains("nobody is signed in"), "{said}");
        assert!(said.contains("Nothing was changed"), "{said}");
    }

    /// Somebody else at the screen would be shown a dialog about another account's request, or
    /// none at all: refused too.
    #[test]
    fn another_account_at_the_screen_is_refused() {
        let said = callers_screen(502, "501").expect_err("not the caller's screen");

        assert!(said.contains("502"), "{said}");
    }
}
