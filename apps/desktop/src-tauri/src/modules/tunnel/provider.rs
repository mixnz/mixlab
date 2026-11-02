//! Which program a tunnel runs. Everything that differs between cloudflared and ngrok is asked of
//! this enum; the running list, the stop and the exit treat both alike.

use std::ffi::OsString;

/// A tunnel's provider, as the tab sends it and every row reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// A Cloudflare quick tunnel: `cloudflared`, no account.
    Cloudflare,
    /// `ngrok http`, with the authtoken ngrok keeps in its own config.
    Ngrok,
}

impl Provider {
    /// The program's name, without this system's executable suffix.
    pub fn program(self) -> &'static str {
        match self {
            Provider::Cloudflare => "cloudflared",
            Provider::Ngrok => "ngrok",
        }
    }

    /// The arguments that open a tunnel to `target`. cloudflared never replaces its own binary
    /// (`--no-autoupdate`); ngrok gets no `--config` and no `--authtoken`, so it reads its own.
    pub fn args(self, target: &str) -> Vec<OsString> {
        let args: &[&str] = match self {
            Provider::Cloudflare => &["tunnel", "--no-autoupdate", "--url", target],
            Provider::Ngrok => &["http", target, "--log", "stdout", "--log-format", "json"],
        };
        args.iter().map(OsString::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(provider: Provider) -> Vec<String> {
        provider
            .args("http://localhost:5173")
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    /// ngrok reads the person's own config and authtoken, as it does in a terminal.
    #[test]
    fn ngrok_gets_no_config_and_no_authtoken() {
        assert_eq!(
            args(Provider::Ngrok),
            [
                "http",
                "http://localhost:5173",
                "--log",
                "stdout",
                "--log-format",
                "json"
            ]
        );
    }

    #[test]
    fn cloudflared_s_arguments_are_unchanged() {
        assert_eq!(
            args(Provider::Cloudflare),
            [
                "tunnel",
                "--no-autoupdate",
                "--url",
                "http://localhost:5173"
            ]
        );
    }

    #[test]
    fn providers_travel_as_lowercase_words() {
        assert_eq!(
            serde_json::to_string(&Provider::Ngrok).expect("json"),
            "\"ngrok\""
        );
        assert_eq!(
            serde_json::from_str::<Provider>("\"cloudflare\"").expect("json"),
            Provider::Cloudflare
        );
    }
}
