//! `mix cert`: certificates and the local CA.

use mixengine_proto::{
    BrowserDatabase, Browsers, CaRotateReport, CaState, CaStatus, CaUninstallReport,
    CertIssueReport, CertProblem, CertState, CertStatusReport, Handshake, IssueOutcome,
    RotateOutcome, Trust, UninstallOutcome, Unusable, Verdict,
};

/// `mix cert issue`, for a person — roadmap task **T50**.
///
/// **A line per site and never a count.** "3 certificates issued" is a number nobody can act on;
/// the domain is what somebody opens in a browser, and the reason a refusal gives is the only part
/// of this output that ever needs doing something about.
pub(crate) fn cert_issue(report: &CertIssueReport) -> String {
    if report.sites.is_empty() {
        return "  no site in this home declares HTTPS\n".to_owned();
    }

    report
        .sites
        .iter()
        .map(|site| match (&site.outcome, &site.state) {
            (IssueOutcome::Issued {}, CertState::Present { cert }) => format!(
                "  {}  issued: {} days, {} name(s)\n",
                site.domain,
                cert.days_left,
                cert.sans.len()
            ),
            (IssueOutcome::Reused {}, CertState::Present { cert }) => format!(
                "  {}  unchanged: {} days left\n",
                site.domain, cert.days_left
            ),
            // Roadmap task **T52**: a site that declares no HTTPS asked for nothing, and printing
            // it as "not issued" reads as a fault where there is none.
            (IssueOutcome::NotWanted { because }, _) => {
                format!("  {}  nothing to do: {because}\n", site.domain)
            }
            (IssueOutcome::Refused { because }, _) => {
                format!("  {}  not issued: {because}\n", site.domain)
            }
            // A written or reused certificate that does not read back is a state nothing should
            // produce, and printing it as success would hide exactly the case worth seeing.
            (_, state) => format!("  {}  unclear: {state:?}\n", site.domain),
        })
        .collect()
}

/// `mix cert status`, for a person — roadmap task **T53**.
///
/// **The command to run is written here and not by the daemon.** The answer carries a
/// [`CertProblem`], which is a name for a condition; turning that into `mix cert issue --site …` is
/// this client's job, because a graphical client renders a button for the same condition and would
/// have no use for a sentence telling its user to open a terminal.
///
/// **Two lines per site and not one**, unlike [`cert_issue`]: what is on the wire and what is on
/// the disk are two facts, and the whole point of this command is the case where they disagree.
pub(crate) fn cert_status(report: &CertStatusReport) -> String {
    if report.sites.is_empty() {
        return "  no site in this home
"
        .to_owned();
    }

    report
        .sites
        .iter()
        .map(|site| {
            let mut lines = format!(
                "  {}
",
                site.domain
            );

            lines.push_str(&match &site.handshake {
                Handshake::NotAsked {} => "    served over HTTP only
"
                .to_owned(),
                Handshake::NotServed { because } => {
                    format!(
                        "    not served over TLS: {because}
"
                    )
                }
                Handshake::Failed { because } => format!(
                    "    the handshake failed: {because}
"
                ),
                Handshake::Presented { cert, trust } => format!(
                    "    presented {}: {} days, {} name(s), {}
",
                    short(&cert.fingerprint),
                    cert.days_left,
                    cert.sans.len(),
                    match trust {
                        Verdict::Trusted {} => "trusted by this home's authority".to_owned(),
                        Verdict::Rejected { because } => format!("not trusted: {because}"),
                    }
                ),
            });

            if let Some(problem) = site.problem {
                lines.push_str(&format!(
                    "    {}
",
                    advice(&site.domain, problem)
                ));
            }

            lines
        })
        .collect()
}

/// The first sixteen characters of a fingerprint, which is what a person compares by eye.
///
/// The whole hash is in `--json` for anything that compares by machine.
fn short(fingerprint: &str) -> &str {
    &fingerprint[..fingerprint.len().min(16)]
}

/// What to do about a condition, in this client's own words.
fn advice(domain: &str, problem: CertProblem) -> String {
    match problem {
        CertProblem::NoCertificate | CertProblem::NamesDiffer | CertProblem::Expiring => {
            format!("run `mix cert issue --site {domain}`")
        }
        CertProblem::NotServed => {
            "start this home's front end; `mix service list` says which it is".to_owned()
        }
        CertProblem::ServedCertificateDiffers => {
            "the running server is holding an older certificate; restart this home's front end"
                .to_owned()
        }
        CertProblem::NotTrusted => {
            "this was not signed by this home's authority; `mix cert ca-status` says which \
             authority this home has"
                .to_owned()
        }
        // `CertProblem` is `#[non_exhaustive]`: a variant added by a newer daemon reaches an older
        // `mix`, and printing nothing at all would be worse than saying there is something to look
        // at.
        _ => "run `mix doctor`".to_owned(),
    }
}

/// `mix cert ca-status`, for a person.
///
/// **The trust line prints what the daemon said and never a word more.** T48 left it off entirely,
/// because there was no such fact in the answer and a line implying one would be the client
/// inventing something — `CLAUDE.md`'s "a client only renders what the daemon returns". T49a put the
/// fact in the answer, including the case where the daemon could not find out, which prints as
/// exactly that rather than as "no".
///
/// **And it says which store**, because "trusted" and "in this machine's own store, for every
/// account on it" are different claims and only the second is what happened.
pub(crate) fn ca_status(status: &CaStatus) -> String {
    let mut rendered = certificate(&status.state);

    rendered.push_str(&match &status.trust {
        Trust::Installed { store } => format!(
            "  trusted    yes, in {store}
"
        ),
        Trust::NotInstalled { because } => format!(
            "  trusted    no: {because}
"
        ),
        Trust::NoStore { because } => format!(
            "  trusted    n/a: {because}
"
        ),
        Trust::Unknown { because } => format!(
            "  trusted    unknown: {because}
"
        ),
    });

    // **A line per database, and never a summary count.** "2 of 3" is a number nobody can act on;
    // the path is what a person opens and the owner is what tells them which browser to restart.
    rendered.push_str(&match &status.browsers {
        Browsers::Reached { databases } if databases.is_empty() => {
            "  browsers   none found; Firefox and Chrome keep certificate databases of their own, \
             and this machine has none
"
            .to_owned()
        }
        Browsers::Reached { databases } => databases.iter().map(browser).collect::<String>(),
        Browsers::NoTool { because } => format!(
            "  browsers   not asked: {because}
"
        ),
        Browsers::NotSearched { because } => format!(
            "  browsers   n/a: {because}
"
        ),
        Browsers::Unknown { because } => format!(
            "  browsers   unknown: {because}
"
        ),
    });

    rendered
}

/// `mix cert ca-rotate` — roadmap task **T54**.
///
/// **What is left comes from the status, because that is the measurement**; the outcome supplies the
/// reason a measurement cannot give.
pub(crate) fn ca_rotate(report: &CaRotateReport) -> String {
    let mut rendered = match &report.outcome {
        RotateOutcome::Rotated {} => {
            let mut said = format!(
                "this home has a new certificate authority\n{} site certificate(s) were reissued under it\n",
                report.sites.len()
            );

            // **The one thing a rotation can leave behind and not otherwise mention.** A previous
            // authority that could not be read cannot be named for removal — T49a's D5 forbids
            // guessing — so the old certificate is still in the store, and saying nothing here
            // would read as a clean rotation.
            if report.previous.is_none() {
                said.push_str(
                    "the previous certificate was left in this machine's trust store: it could not\nbe read, and nothing is removed that cannot be named\n",
                );
            }

            said
        }

        RotateOutcome::NotCommitted { because } => format!(
            "nothing was changed: {because}\n\nrun `mix cert ca-status` to see what this machine holds now\n"
        ),

        RotateOutcome::NothingToRotate { because } => {
            format!("{because}\n\nrun `mix doctor --repair` to make one\n")
        }

        // `RotateOutcome` is `#[non_exhaustive]`: a variant a newer daemon knows reaches an older
        // `mix`, and printing nothing would be worse than saying the question was asked.
        _ => "this home's certificate authority was asked about\n".to_owned(),
    };

    rendered.push('\n');
    rendered.push_str(&ca_status(&report.status));
    rendered
}

/// `mix cert ca-uninstall` — roadmap task **T54**.
///
/// **What is left comes from the status, because that is the measurement**; the outcome supplies the
/// reason a measurement cannot give. So the two halves cannot disagree: there is only one reading.
pub(crate) fn ca_uninstall(report: &CaUninstallReport) -> String {
    let mut rendered = match &report.outcome {
        UninstallOutcome::Removed {} => {
            "this home's certificate authority was taken out of every store that held it
the certificate and its key are still on disk; `mix doctor --repair` puts the trust back
"
            .to_owned()
        }
        UninstallOutcome::PartlyRemoved { because } => format!(
            "some of it is still there: {because}

run `mix elevation grant` if a prompt was refused, or `mix cert ca-status` to look again
"
        ),
        UninstallOutcome::NothingToRemove { because } => format!("{because}\n"),
        // `UninstallOutcome` is `#[non_exhaustive]`: a variant a newer daemon knows reaches an older
        // `mix`, and printing nothing would be worse than saying the question was asked.
        _ => "this home's certificate authority was asked about\n".to_owned(),
    };

    rendered.push('\n');
    rendered.push_str(&ca_status(&report.status));
    rendered
}

/// One database's line.
fn browser(database: &BrowserDatabase) -> String {
    let verdict = if database.installed {
        "yes".to_owned()
    } else {
        match &database.because {
            Some(because) => format!("no: {because}"),
            None => "no".to_owned(),
        }
    };

    format!(
        "  browsers   {verdict}: {} ({})
",
        database.path, database.owner
    )
}

/// The authority itself, which is the half T48 built.
fn certificate(state: &CaState) -> String {
    match state {
        // Reachable, and worth a sentence rather than an empty screen: a start whose generation
        // failed warns into the daemon's log and carries on, so this is what the next question gets.
        CaState::Absent {} => "  authority  none; one is made when the daemon starts
"
        .to_owned(),

        CaState::Unusable { because } => {
            format!(
                "  authority  unusable: {}
",
                unusable(*because)
            )
        }

        CaState::Present { ca } => {
            let mut rendered = format!(
                "  authority  {}
",
                ca.subject
            );
            rendered.push_str(&format!(
                "  sha256     {}
",
                ca.fingerprint
            ));

            // Negative rather than clamped: an expired authority is a true state, and a screen that
            // said "in -3 days" — or silently "in 0 days" — would be hiding the one thing worth
            // acting on.
            rendered.push_str(&if ca.days_left < 0 {
                format!(
                    "  expired    {} days ago
",
                    ca.days_left.abs()
                )
            } else {
                format!(
                    "  expires    in {} days
",
                    ca.days_left
                )
            });

            rendered
        }
    }
}

/// Each way of being unusable, in a sentence. **Not what to do about it**: the reason is a fact
/// about this home, and the remedy is `mix cert ca-rotate`, which T54 builds.
fn unusable(because: Unusable) -> &'static str {
    match because {
        Unusable::KeyMissing => "the certificate is here and its private key is not",
        Unusable::CertificateMissing => "the private key is here and the certificate is not",
        Unusable::KeyUnreadable => "the private key is not one this build can read",
        Unusable::CertificateUnreadable => "the certificate is not a certificate",
        Unusable::KeyAndCertificateDisagree => {
            "the certificate and the private key are not each other's"
        }
    }
}
