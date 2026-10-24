//! Windows: `netsh`, parsed.
//!
//! **A command and not an API**, deliberately. Windows exposes these ranges through `netsh`'s output
//! and through the registry's `ReservedPorts` value, and the two do not agree: the registry holds
//! what an administrator asked for, while `netsh` holds what the system has actually taken —
//! including the dynamic ranges Hyper-V and `winnat` claim at boot. The second is the one a failing
//! bind is about.

use std::ffi::OsStr;

use crate::{PortRange, ReservedPorts, Result};

/// This system's answer.
#[derive(Debug, Default)]
pub(crate) struct Reserved;

impl ReservedPorts for Reserved {
    fn reserved(&self) -> Result<Vec<PortRange>> {
        // Through `command::output_of` and not a `Command` of its own, for the two things that
        // helper settles: the tool is `System32\netsh.exe` rather than whatever `PATH` says, and the
        // child is started without a console window. A daemon has no console, so a bare `Command`
        // here was a Windows Terminal window flashing on the desktop on every `mix doctor`. The exit
        // status is not consulted, as it never was: an empty list is a list.
        let output = super::command::output_of(
            "netsh",
            ["int", "ipv4", "show", "excludedportrange", "protocol=tcp"]
                .iter()
                .map(OsStr::new),
        )?;

        Ok(crate::reserved::parse(&output))
    }
}
