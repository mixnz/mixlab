//! Which program a tunnel runs. Everything that differs between cloudflared and ngrok is asked of
//! this enum; the running list, the stop and the exit treat both alike.

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
}
