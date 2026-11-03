//! Whether ngrok has an authtoken, and where it comes from. Read, never shown: MixLab reports that a
//! token is set and where, and nothing of the token itself.

use std::path::PathBuf;

/// Whether a token is set, and where from: `environment` (`NGROK_AUTHTOKEN`) or `config`. `path`
/// is ngrok's config file when MixLab could find it, set or not.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub set: bool,
    pub source: Option<&'static str>,
    pub path: Option<String>,
}

/// Whether ngrok's config holds a non-empty `authtoken:` — under `agent:` in a v3 file, at the top
/// in a v2 one. One line is read; nothing else in the file is.
pub fn has_token(config: &str) -> bool {
    config.lines().any(|line| {
        line.trim()
            .strip_prefix("authtoken:")
            .is_some_and(|value| !value.trim().trim_matches(['"', '\'']).trim().is_empty())
    })
}

/// The config file `ngrok config check` names, valid or not.
pub fn config_path(check_output: &str) -> Option<PathBuf> {
    check_output.lines().find_map(|line| {
        if let Some((_, path)) = line.split_once("Valid configuration file at ") {
            return Some(PathBuf::from(path.trim()));
        }
        let (_, rest) = line.split_once("invalid configuration at ")?;
        let (path, _) = rest.split_once(": ")?;
        Some(PathBuf::from(path.trim()))
    })
}

/// The status from what was found: the environment wins, as it does for ngrok.
pub fn status(env_token: Option<String>, config: Option<(PathBuf, String)>) -> Status {
    let path = config.as_ref().map(|(path, _)| path.display().to_string());
    if env_token.is_some_and(|token| !token.trim().is_empty()) {
        return Status {
            set: true,
            source: Some("environment"),
            path: None,
        };
    }
    if config.is_some_and(|(_, text)| has_token(&text)) {
        return Status {
            set: true,
            source: Some("config"),
            path,
        };
    }
    Status {
        set: false,
        source: None,
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_v3_token_under_agent_is_found() {
        assert!(has_token(
            "version: \"3\"\nagent:\n    authtoken: 2abcDEF\n"
        ));
    }

    #[test]
    fn a_v2_token_at_the_top_is_found() {
        assert!(has_token("authtoken: 2abcDEF\nregion: eu\n"));
    }

    /// What `ngrok config add-authtoken ""` leaves, measured on 3.39.13.
    #[test]
    fn an_emptied_token_is_none() {
        assert!(!has_token("version: \"3\"\nagent:\n    authtoken: \"\"\n"));
        assert!(!has_token("agent:\n  authtoken:\n"));
    }

    #[test]
    fn no_line_or_a_comment_is_none() {
        assert!(!has_token("version: \"3\"\n"));
        assert!(!has_token("# authtoken: 2abcDEF\n"));
    }

    /// The line `ngrok config check` printed on Windows, measured on 3.39.13.
    #[test]
    fn the_config_path_is_read_from_ngrok_s_check() {
        assert_eq!(
            config_path(
                "Valid configuration file at C:\\Users\\a\\AppData\\Local\\ngrok\\ngrok.yml\n"
            ),
            Some(PathBuf::from(
                "C:\\Users\\a\\AppData\\Local\\ngrok\\ngrok.yml"
            ))
        );
        assert_eq!(
            config_path(
                "ERROR:  invalid configuration at /home/a/.config/ngrok/ngrok.yml: Error reading"
            ),
            Some(PathBuf::from("/home/a/.config/ngrok/ngrok.yml"))
        );
        assert_eq!(config_path("something else"), None);
    }

    #[test]
    fn the_environment_wins_over_the_config() {
        let config = Some((PathBuf::from("/c/ngrok.yml"), "authtoken: x\n".to_owned()));
        let from_env = status(Some("x".into()), config.clone());
        assert!(from_env.set);
        assert_eq!(from_env.source, Some("environment"));
        assert_eq!(from_env.path, None);

        let from_config = status(None, config);
        assert_eq!(from_config.source, Some("config"));
        assert_eq!(from_config.path.as_deref(), Some("/c/ngrok.yml"));
    }

    #[test]
    fn nothing_anywhere_is_not_set() {
        let none = status(
            Some(String::new()),
            Some((PathBuf::from("/c/ngrok.yml"), String::new())),
        );
        assert!(!none.set);
        assert_eq!(none.source, None);
        assert_eq!(none.path.as_deref(), Some("/c/ngrok.yml"));
    }
}
