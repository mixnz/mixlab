//! Requests in the `database.*` namespace.

use crate::ServiceId;

/// `database.client` — where one instance could be opened, and with what. Reads only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DatabaseClientQuery {
    /// Which instance: `mariadb@main`, `redis@main`.
    pub service: ServiceId,
}

/// `database.credentials` — the password held for one account. Reads only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DatabaseCredentialsQuery {
    /// Which instance: `mariadb@main`, `postgres@shop`.
    pub service: ServiceId,

    /// The account to read. The server's administrator when nobody says — `database.open`'s own
    /// default, for the same reason: the two commands are one question asked by a process and by
    /// a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// `database.open` — hand one instance to the installed desktop client.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DatabaseOpen {
    /// Which instance.
    pub service: ServiceId,

    /// The account to sign in as. The server's administrator when nobody says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// A database to open at, when the client should land in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
}

/// `database.create` — make sure a database and an account for it exist on one instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct DatabaseCreate {
    /// Which instance: `mariadb@main`, `postgres@shop`.
    pub service: ServiceId,

    /// The database's name.
    pub database: String,

    /// The account's name. The database's own name when nobody says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// A password the caller chose, rather than one MixEngine generates.
    ///
    /// Validated and never escaped: `mixengine_core::generate::databases::validated_password`
    /// refuses everything that could end the quoted SQL literal it is interpolated into. Absent
    /// (rather than empty) means *generate one*, which is what every caller built before this task
    /// still asks for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}
