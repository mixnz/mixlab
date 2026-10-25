//! `mix database`: databases, their accounts, and the client that opens one.

use mixengine_proto::{
    DatabaseAccount, DatabaseClientReport, DatabaseCredentials, DatabaseHandoff, DesktopClient,
    Launch, Made,
};

/// `mix database create` — what was made, and where the credential is.
///
/// **It says where the password lives and never what it is.** Two lines, aligned on the noun, in
/// the words this file uses everywhere else rather than a glyph.
pub(crate) fn database_created(
    account: &DatabaseAccount,
    store: Option<mixengine_proto::CredentialStore>,
) -> String {
    let word = |made: Made| match made {
        Made::Created => "created",
        Made::Existing => "already existed",
    };

    format!(
        "database {} {} on {}\naccount  {} {}, password in {}",
        account.database,
        word(account.made.database),
        account.service,
        account.user,
        word(account.made.user),
        where_kept(&account.secret, store),
    )
}

/// `mix database credentials` — the address, and the password itself, last and alone.
///
/// **The last line is the value and nothing else**, so that
/// `mix database credentials mariadb@main --user blog | tail -1` is the password, for a script
/// writing a project's `.env`.
pub(crate) fn database_credentials(
    answer: &DatabaseCredentials,
    store: Option<mixengine_proto::CredentialStore>,
) -> String {
    // The value at the start of its own line, not indented under the two above: `tail -1` takes
    // the whole line, and a client handed `  value` is handed a different password.
    format!(
        "password for {} on {}\n  stored in {}\n{}",
        answer.user,
        answer.service,
        where_kept(&answer.secret, store),
        answer.password,
    )
}

/// Where a credential is, in words. A home that keeps its passwords in a file says so; any other
/// answer, an older daemon's included, names the store's namespace as it always has.
fn where_kept(
    secret: &mixengine_proto::SecretAddress,
    store: Option<mixengine_proto::CredentialStore>,
) -> String {
    match store {
        Some(mixengine_proto::CredentialStore::Home) => {
            format!("this home's credentials file at {}", secret.key)
        }
        _ => format!("the {} credentials at {}", secret.service, secret.key),
    }
}

/// `mix database client`, for a person.
///
/// Two lines: what the service speaks, and where it could be opened. A service no client opens is
/// said in those words rather than left as a blank, since a blank reads as "nothing installed".
pub(crate) fn database_client(report: &DatabaseClientReport) -> String {
    let protocol = match report.protocol {
        Some(protocol) => format!("{protocol} protocol"),
        None => "not a database a desktop client opens".to_owned(),
    };

    let mut out = format!(
        "{}  {protocol}\n{}",
        report.service,
        desktop_client(&report.client)
    );

    // **Where the credential is, without opening anything.** An address is a name, and printing it
    // is what stops the next question being *"and where would I find the password?"*.
    if let Some(at) = &report.secret {
        out.push_str(&format!(
            "  its administrator's password is in the {} credentials at {}\n",
            at.service, at.key
        ));
    }

    out
}

/// `mix database open`, for a person.
///
/// **The password is never printed**, and neither is anything that looks like one: what is printed
/// is where it was read from and that it went to one process. A client that did not open prints
/// its state in the same words `client` uses, so the two commands agree.
pub(crate) fn database_opened(handoff: &DatabaseHandoff) -> String {
    match (&handoff.client, handoff.launched) {
        (DesktopClient::Installed { name, .. }, Some(launched)) => {
            let account = match &handoff.user {
                Some(user) => format!(" as {user}"),
                None => String::new(),
            };
            let how = match launched {
                Launch::Running { pid } => format!("pid {pid}"),
                Launch::HandedOn => "handed to the copy already running".to_owned(),
            };
            let secret = match &handoff.secret {
                Some(at) => format!(
                    "  password read from the {} credentials at {} and handed to that process \
                     alone\n",
                    at.service, at.key
                ),
                None => String::new(),
            };

            format!(
                "opened {} in {name}{account} ({how})\n{secret}",
                handoff.service
            )
        }
        (client, _) => desktop_client(client),
    }
}

/// The client's state, in the words both commands print.
fn desktop_client(client: &DesktopClient) -> String {
    match client {
        DesktopClient::Installed { name, program } => {
            format!("  {name} installed at {program}\n")
        }
        DesktopClient::NoClient => "  this install has no MixLab window to open a database in\n  \
                                    MixLab comes with MixEngine's installers; the headless \
                                    ones have none\n"
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{SecretAddress, ServiceId};

    use super::*;

    /// It says where the credential is and never what it is. The test is the guard: a renderer that
    /// grew a password would put one in a terminal's scrollback.
    #[test]
    fn a_created_database_says_where_the_password_lives() {
        let rendered = database_created(&blog_account(), None);

        assert!(
            rendered.contains("database blog created on mariadb@main"),
            "{rendered}"
        );
        assert!(
            rendered.contains("account  blog already existed"),
            "{rendered}"
        );
        assert!(rendered.contains("mariadb@main/blog"), "{rendered}");
        assert!(
            !rendered.to_ascii_lowercase().contains("password is"),
            "{rendered}"
        );
    }

    fn blog_account() -> DatabaseAccount {
        DatabaseAccount {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            database: "blog".to_owned(),
            user: "blog".to_owned(),
            secret: SecretAddress::of("mariadb@main/blog"),
            made: mixengine_proto::Provisioned {
                database: Made::Created,
                user: Made::Existing,
            },
        }
    }

    /// The last line is the password and nothing else, so `| tail -1` hands a script the value a
    /// client accepts. It carried the block's two-space indent until a check by hand piped it into
    /// `mariadb -p` and was refused.
    #[test]
    fn the_last_line_of_the_credentials_is_the_password_alone() {
        let answer = DatabaseCredentials {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            user: "blog".to_owned(),
            secret: SecretAddress::of("mariadb@main/blog"),
            password: "s3cr3t-value".to_owned(),
        };

        let rendered = database_credentials(&answer, None);

        assert_eq!(
            rendered.lines().last(),
            Some("s3cr3t-value"),
            "{rendered:?}"
        );
    }

    /// A home that keeps its passwords in a file says so where it says where one is.
    #[test]
    fn a_database_in_a_file_store_says_so() {
        let home = Some(mixengine_proto::CredentialStore::Home);

        let created = database_created(&blog_account(), home);
        assert!(
            created.contains("this home's credentials file at mariadb@main/blog"),
            "{created}"
        );

        let answer = DatabaseCredentials {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            user: "blog".to_owned(),
            secret: SecretAddress::of("mariadb@main/blog"),
            password: "p".to_owned(),
        };
        let told = database_credentials(&answer, home);
        assert!(
            told.contains("this home's credentials file at mariadb@main/blog"),
            "{told}"
        );
        let untold = database_credentials(&answer, None);
        assert!(untold.contains("mixengine credentials"), "{untold}");
    }
}
