//! `mix domain`, for a person.

use mixengine_proto::DomainStatusReport;

/// `domain.dns_status`, as a person reads it.
///
/// **One column per fact, because the four fail independently.** A single "works / does not" column
/// would be exactly the derivation the report exists to prevent; the sentence under a failing row is
/// the thing a person acts on, and it is the daemon's sentence rather than this client's.
pub(crate) fn domain_status(report: &DomainStatusReport) -> String {
    if report.domains.is_empty() {
        return "no domains declared
"
        .to_owned();
    }

    let width = report
        .domains
        .iter()
        .map(|row| row.domain.len())
        .max()
        .unwrap_or_default();

    let mut out = String::new();

    for row in &report.domains {
        let resolved = if row.resolves_to.is_empty() {
            "does not resolve".to_owned()
        } else {
            row.resolves_to
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };

        out.push_str(&format!(
            "{:width$}  {}  {}  {}  {}
",
            row.domain,
            if row.site.is_some() {
                "declared"
            } else {
                "unknown "
            },
            if row.hosts_entry { "hosts" } else { "     " },
            if row.wildcard { "wildcard" } else { "        " },
            resolved,
        ));

        if let Some(because) = &row.because {
            out.push_str(&format!(
                "{:width$}  {because}
",
                ""
            ));
        }
    }

    out
}
