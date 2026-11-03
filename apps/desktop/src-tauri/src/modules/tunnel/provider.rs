//! Which program a tunnel runs. Everything that differs between cloudflared and ngrok is asked of
//! this enum; the running list, the stop and the exit treat both alike.

use std::collections::BTreeMap;
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

    /// The variables passed on to the program beyond the floor `spawn_supervised` gives every
    /// child. ngrok reads its token from `NGROK_AUTHTOKEN` and finds its config under
    /// `XDG_CONFIG_HOME` on Unix; without them the tunnel would read another config than the one
    /// Settings reports. `lookup` is this process's environment.
    pub fn env(self, lookup: impl Fn(&str) -> Option<String>) -> BTreeMap<String, String> {
        let names: &[&str] = match self {
            Provider::Cloudflare => &[],
            Provider::Ngrok => &["NGROK_AUTHTOKEN", "XDG_CONFIG_HOME"],
        };
        names
            .iter()
            .filter_map(|name| {
                let value = lookup(name).filter(|value| !value.is_empty())?;
                Some(((*name).to_owned(), value))
            })
            .collect()
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

    /// `spawn_supervised` hands a child only a floor of this process's environment, so the two
    /// variables that change what ngrok reads are passed on by name.
    #[test]
    fn ngrok_gets_its_token_and_config_home_from_the_environment() {
        let lookup = |name: &str| match name {
            "NGROK_AUTHTOKEN" => Some("2abc".to_owned()),
            "XDG_CONFIG_HOME" => Some("/home/a/.cfg".to_owned()),
            "HTTP_PROXY" => Some("http://proxy".to_owned()),
            _ => None,
        };
        let env = Provider::Ngrok.env(lookup);
        assert_eq!(env.get("NGROK_AUTHTOKEN").map(String::as_str), Some("2abc"));
        assert_eq!(
            env.get("XDG_CONFIG_HOME").map(String::as_str),
            Some("/home/a/.cfg")
        );
        assert_eq!(env.len(), 2);
    }

    #[test]
    fn nothing_unset_or_empty_is_passed_and_cloudflared_gets_nothing() {
        assert!(Provider::Ngrok.env(|_| Some(String::new())).is_empty());
        assert!(Provider::Cloudflare
            .env(|_| Some("x".to_owned()))
            .is_empty());
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
