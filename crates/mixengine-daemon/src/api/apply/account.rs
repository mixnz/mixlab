//! The account a blueprint's database step ends up with — roadmap task **T202**, D1.
//!
//! **Routes around a foreign account, never through it.** `database.create` refuses an account
//! that is on the server and that this home holds no credential for (T77a, D3: a keyring entry is
//! the deed of ownership), and a blueprint has nobody at the keyboard to pick another name. So the
//! step picks one: the plan's, then `-2` … `-9`, against the same database, and takes the first
//! the server does not have or MixEngine owns. What it did is said in the step's note (D2).

use mixengine_core::generate::databases::IDENTIFIER_LIMIT;
use mixengine_proto::{DatabaseAccount, DatabaseCreate, Error, Made};

use crate::services::databases::Making;

/// The one call the step makes, narrow so a test can stand in for the daemon's `Databases`.
pub(crate) trait MakesDatabases {
    /// [`crate::databases::Databases::make`].
    async fn make(&self, asked: &DatabaseCreate) -> Result<DatabaseAccount, Making>;
}

impl MakesDatabases for crate::databases::Databases {
    async fn make(&self, asked: &DatabaseCreate) -> Result<DatabaseAccount, Making> {
        Self::make(self, asked).await
    }
}

/// How many names are tried: the plan's, then `-2` … `-9`.
const TRIES: usize = 9;

/// What the step made, and what it has to say about it.
#[derive(Debug)]
pub(crate) struct MadeForPlan {
    pub(crate) account: DatabaseAccount,
    /// D2's sentence, or `None` when the plan was followed to the letter.
    pub(crate) note: Option<String>,
}

/// The `n`th name tried for `base`: the base itself for `1`, `base-n` after, cut so that the
/// suffix fits [`IDENTIFIER_LIMIT`] and never leaves a hyphen at the join.
pub(crate) fn nth_name(base: &str, n: usize) -> String {
    if n <= 1 {
        return base.to_owned();
    }

    let suffix = format!("-{n}");
    let room = IDENTIFIER_LIMIT - suffix.chars().count();
    let mut cut: String = base.chars().take(room).collect();
    while cut.ends_with('-') {
        cut.pop();
    }

    format!("{cut}{suffix}")
}

/// Make the plan's database with the first account name that is free or ours.
///
/// # Errors
///
/// Every refusal that is not a foreign account, as `database.create` would have returned it; and
/// T77a's refusal with the last name tried in the hint when all [`TRIES`] names are foreign.
pub(crate) async fn with_a_free_name(
    maker: &impl MakesDatabases,
    asked: &DatabaseCreate,
) -> Result<MadeForPlan, Error> {
    let planned = asked.user.clone().unwrap_or_else(|| asked.database.clone());

    let mut last = None;
    for n in 1..=TRIES {
        let attempt = DatabaseCreate {
            user: Some(nth_name(&planned, n)),
            ..asked.clone()
        };

        match maker.make(&attempt).await {
            Ok(account) => {
                return Ok(MadeForPlan {
                    note: note_for(&planned, &account),
                    account,
                });
            }
            Err(Making::Foreign { service, user }) => {
                tracing::info!(
                    service = service.as_str(),
                    user,
                    "the blueprint's account is somebody else's; trying the next name"
                );
                last = Some(Making::Foreign { service, user });
            }
            Err(other) => return Err(other.into_wire()),
        }
    }

    let refused = last.expect("nine tries, each a foreign account");
    Err(refused.into_wire().with_hint(format!(
        "every name from {planned} to {} is somebody else's on this server; free one of them or \
         name another in the blueprint",
        nth_name(&planned, TRIES)
    )))
}

/// D2's sentence: what differed from the plan, or nothing.
fn note_for(planned: &str, account: &DatabaseAccount) -> Option<String> {
    let mut said = Vec::new();

    if account.user != planned {
        said.push(format!(
            "the account {planned} is somebody else's, so this project's is {}",
            account.user
        ));
    }
    if account.made.database == Made::Existing {
        said.push(format!(
            "the database {} was already there and was not emptied",
            account.database
        ));
    }

    (!said.is_empty()).then(|| said.join("; "))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use mixengine_proto::{ErrorCode, Provisioned, SecretAddress, ServiceId};

    use super::*;

    /// A server with some accounts somebody else made and some that are ours.
    struct Server {
        foreign: Vec<&'static str>,
        ours: Vec<&'static str>,
        database_existing: bool,
        /// What `make` was asked for, in order.
        asked: Mutex<Vec<String>>,
        /// `Some` to refuse the `n`th call (1-based) with this error instead of answering.
        down_at: Option<(usize, ErrorCode)>,
    }

    impl Server {
        fn new(foreign: &[&'static str]) -> Self {
            Self {
                foreign: foreign.to_vec(),
                ours: Vec::new(),
                database_existing: false,
                asked: Mutex::new(Vec::new()),
                down_at: None,
            }
        }
    }

    impl MakesDatabases for Server {
        async fn make(&self, asked: &DatabaseCreate) -> Result<DatabaseAccount, Making> {
            let user = asked
                .user
                .clone()
                .expect("the step always names the account");
            let call = {
                let mut seen = self.asked.lock().expect("not poisoned");
                seen.push(user.clone());
                seen.len()
            };

            if let Some((at, code)) = &self.down_at
                && call == *at
            {
                return Err(Making::Failed(Error::new(*code, "the server is down")));
            }
            if self.foreign.contains(&user.as_str()) {
                return Err(Making::Foreign {
                    service: asked.service.clone(),
                    user,
                });
            }

            Ok(DatabaseAccount {
                service: asked.service.clone(),
                database: asked.database.clone(),
                secret: SecretAddress::of(format!("home/{}/{user}", asked.service)),
                made: Provisioned {
                    database: if self.database_existing {
                        Made::Existing
                    } else {
                        Made::Created
                    },
                    user: if self.ours.contains(&user.as_str()) {
                        Made::Existing
                    } else {
                        Made::Created
                    },
                },
                user,
            })
        }
    }

    fn ask() -> DatabaseCreate {
        DatabaseCreate {
            service: ServiceId::parse("mariadb@main").expect("an id"),
            database: "shop".to_owned(),
            user: Some("shop".to_owned()),
            password: None,
        }
    }

    #[tokio::test]
    async fn the_plans_name_is_used_when_it_is_free_and_nothing_is_noted() {
        let server = Server::new(&[]);

        let made = with_a_free_name(&server, &ask()).await.expect("made");

        assert_eq!(made.account.user, "shop");
        assert_eq!(made.note, None);
        assert_eq!(*server.asked.lock().expect("not poisoned"), vec!["shop"]);
    }

    /// D1. The first free name after the plan's, the database made once, the note saying so.
    #[tokio::test]
    async fn a_foreign_account_is_routed_around_with_the_next_name_and_noted() {
        let server = Server::new(&["shop", "shop-2"]);

        let made = with_a_free_name(&server, &ask()).await.expect("made");

        assert_eq!(made.account.user, "shop-3");
        assert_eq!(
            made.note.as_deref(),
            Some("the account shop is somebody else's, so this project's is shop-3")
        );
        assert_eq!(
            *server.asked.lock().expect("not poisoned"),
            vec!["shop", "shop-2", "shop-3"]
        );
    }

    /// D1. An account that is ours under the next name is reused, as `database.create` reuses it.
    #[tokio::test]
    async fn a_name_that_is_ours_is_taken_rather_than_passed_over() {
        let mut server = Server::new(&["shop"]);
        server.ours = vec!["shop-2"];

        let made = with_a_free_name(&server, &ask()).await.expect("made");

        assert_eq!(made.account.user, "shop-2");
        assert_eq!(made.account.made.user, Made::Existing);
    }

    /// D2. A database that was already there is said, with or without a renamed account.
    #[tokio::test]
    async fn a_database_that_was_already_there_is_noted() {
        let mut server = Server::new(&["shop"]);
        server.database_existing = true;

        let made = with_a_free_name(&server, &ask()).await.expect("made");

        assert_eq!(
            made.note.as_deref(),
            Some(
                "the account shop is somebody else's, so this project's is shop-2; the database \
                 shop was already there and was not emptied"
            )
        );
    }

    /// D1. Nine foreign names end in T77a's refusal, naming the last one tried.
    #[tokio::test]
    async fn nine_foreign_names_are_the_refusal_with_the_last_name_in_the_hint() {
        let server = Server::new(&[
            "shop", "shop-2", "shop-3", "shop-4", "shop-5", "shop-6", "shop-7", "shop-8", "shop-9",
        ]);

        let error = with_a_free_name(&server, &ask())
            .await
            .expect_err("every name is somebody else's");

        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.message.contains("shop-9"), "{}", error.message);
        let hint = error.hint.expect("a hint");
        assert!(hint.contains("from shop to shop-9"), "{hint}");
        assert_eq!(server.asked.lock().expect("not poisoned").len(), 9);
    }

    /// Review focus 3. Any other refusal on a later try is the error, not a tenth name.
    #[tokio::test]
    async fn a_refusal_that_is_not_a_foreign_account_is_not_retried() {
        let mut server = Server::new(&["shop"]);
        server.down_at = Some((2, ErrorCode::PreconditionFailed));

        let error = with_a_free_name(&server, &ask())
            .await
            .expect_err("the server went down");

        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        assert_eq!(server.asked.lock().expect("not poisoned").len(), 2);
    }

    /// Review focus 1 and 2. A base at the limit is cut so the suffix fits, and never joins on a
    /// hyphen.
    #[test]
    fn a_long_name_is_cut_for_its_suffix_and_never_ends_the_cut_on_a_hyphen() {
        let thirty_two = "a".repeat(32);
        assert_eq!(nth_name(&thirty_two, 1), thirty_two);
        assert_eq!(nth_name(&thirty_two, 2), format!("{}-2", "a".repeat(30)));
        assert_eq!(nth_name(&thirty_two, 2).chars().count(), IDENTIFIER_LIMIT);

        let hyphen_at_the_cut = format!("{}-{}", "b".repeat(29), "c".repeat(2));
        assert_eq!(
            nth_name(&hyphen_at_the_cut, 2),
            format!("{}-2", "b".repeat(29))
        );

        assert_eq!(nth_name("shop", 2), "shop-2");
        assert_eq!(nth_name("shop", 9), "shop-9");
        for n in 1..=TRIES {
            mixengine_core::generate::databases::validated_identifier(&nth_name(&thirty_two, n))
                .expect("every name tried is a valid identifier");
        }
    }
}
