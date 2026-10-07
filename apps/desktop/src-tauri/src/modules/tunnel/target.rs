//! What a tunnel may point at — T203, D4: an address on this machine and nothing else.

use crate::error::AppError;

/// The target as cloudflared takes it, or why it is refused.
pub fn parse(input: &str) -> Result<String, AppError> {
    let input = input.trim();
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    let url = url::Url::parse(&with_scheme)
        .map_err(|_| err!("error.tunnelTargetInvalid", target = input))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(err!("error.tunnelTargetInvalid", target = input));
    }
    let local = match url.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.octets() == [127, 0, 0, 1],
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !local {
        return Err(err!("error.tunnelTargetNotLocal", target = input));
    }
    // `Url` adds a `/` to a bare authority; cloudflared takes the address as it was typed.
    let rendered = url.to_string();
    Ok(
        if url.path() == "/" && url.query().is_none() && !input.ends_with('/') {
            rendered.trim_end_matches('/').to_owned()
        } else {
            rendered
        },
    )
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn no_scheme_means_http() {
        assert_eq!(
            parse("localhost:5173").expect("ok"),
            "http://localhost:5173"
        );
    }

    #[test]
    fn https_and_the_three_loopback_names_are_accepted() {
        for input in [
            "https://localhost:8443",
            "127.0.0.1:3000",
            "http://[::1]:8080",
        ] {
            assert!(parse(input).is_ok(), "{input}");
        }
    }

    #[test]
    fn anything_not_on_this_machine_is_refused() {
        for input in [
            "192.168.1.20:3000",
            "example.com",
            "http://myblog.test",
            "ftp://localhost",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
    }

    #[test]
    fn a_path_and_a_query_are_kept() {
        assert_eq!(
            parse("localhost:3000/api?x=1").expect("ok"),
            "http://localhost:3000/api?x=1"
        );
    }
}
