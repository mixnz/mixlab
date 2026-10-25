//! The `ServiceError` half of [`ToWire`](super::ToWire) for `mixengine_core::Error`.
//!
//! Its own file because it is the longest of the library enums, and the one most often added to.

use mixengine_core::ServiceError;
use mixengine_proto::{Error, ErrorCode};

use super::chain;

/// Translate one `ServiceError`, where `whole` is the error it came wrapped in.
///
/// `whole` rather than `error` is what is flattened into the message, so the wording a client
/// sees is the same it was before this arm had a file of its own.
pub(super) fn service(error: &ServiceError, whole: &mixengine_core::Error) -> Error {
    match error {
        // The third way of saying "it is already here", and the one whose repair is
        // different: a service is not replaced by installing something, it is replaced by
        // deleting it.
        ServiceError::ServiceAlreadyDeclared { .. } => {
            Error::new(ErrorCode::AlreadyExists, chain(whole)).with_hint(
                "`mix service delete` first; deleting a service keeps its data directory",
            )
        }

        // The fourth, and the one whose repair is a different *argument* rather than a
        // different call: nothing has to be deleted, the create has to name somewhere else.
        // The message already carries the service that got there first, so the hint spends
        // itself on what to do about it.
        ServiceError::DataDirectoryTaken { holder, .. } => {
            Error::new(ErrorCode::AlreadyExists, chain(whole)).with_hint(format!(
                "give this one a directory of its own, or `mix service delete {holder}` if it \
                 is the one that should go; two servers over one data directory corrupt it"
            ))
        }

        ServiceError::InvalidDatabaseName { .. } => {
            Error::new(ErrorCode::InvalidArgument, chain(whole)).with_hint(
                "a database and its account are named like a project: lower-case letters, \
                 digits and hyphens, up to thirty-two characters",
            )
        }

        // The message never carries the value — `ServiceError::InvalidPassword` has none to
        // leak.
        ServiceError::InvalidPassword { .. } => {
            Error::new(ErrorCode::InvalidArgument, chain(whole)).with_hint(
                "1 to 128 printable ASCII characters, and none of: a quote, a backslash, or \
                 whitespace",
            )
        }

        // `conflict` rather than `already_exists`: what is refused is not the name being
        // taken but MixEngine being unable to prove the account is its own to change.
        ServiceError::AccountNotOurs { .. } => Error::new(ErrorCode::Conflict, chain(whole))
            .with_hint(
                "MixEngine only manages an account whose password it holds; `--user` picks another \
             name, or drop that account on the server first",
            ),

        // **Not `unsupported`**, which means *this operating system cannot* and would be a
        // lie about the machine: every system this ships to runs Redis, and Redis is what
        // has no databases. The same distinction the first planner drew for
        // `blueprint.apply`.
        ServiceError::NoDatabaseVocabulary { .. } => {
            Error::new(ErrorCode::InvalidArgument, chain(whole)).with_hint(
                "only the database servers have databases; `mix service list` shows what this \
                 home runs",
            )
        }

        // **The generated-configuration family.** What separates these is the same thing
        // that separates the index failures above: whose fault it is. An override is the
        // user's, a template and a recipe are ours, and a row that cannot be read back is
        // neither — it is a database somebody edited.
        //
        // All three of these are a person's own overrides, and the message already names
        // the setting — the ones that exist, the shape it had to be, or what is wrong with
        // the value the recipe was handed. The hint says the thing the message cannot: that
        // being refused is the *feature*, because the alternative is a setting silently
        // doing nothing.
        ServiceError::UnknownSetting { .. }
        | ServiceError::SettingType { .. }
        | ServiceError::SettingValue { .. } => Error::new(ErrorCode::InvalidArgument, chain(whole))
            .with_hint(
                "MixEngine refuses a setting it does not know rather than ignoring it, so that \
                 a typo cannot look like a setting that is in effect",
            ),

        // The service's own program said no. Nothing was installed — the rendering is
        // judged in a staging directory — and saying so is the whole point: a user who has
        // just been told their configuration is broken will otherwise assume their site is
        // down.
        ServiceError::ConfigRejected { .. } => Error::new(ErrorCode::InvalidArgument, chain(whole))
            .with_hint(
                "nothing was installed and the configuration that is live is the last one that \
                 worked; the service reading it has not been disturbed",
            ),

        // A `packages.name` this build has no recipe for: a home written by a newer
        // MixEngine, or a service an extension declared and then went away. Not `internal`,
        // because nothing is broken here and the way out is a version rather than a bug
        // report.
        ServiceError::NoRecipe { .. } => Error::new(ErrorCode::PreconditionFailed, chain(whole))
            .with_hint(
                "what MixEngine can run is compiled into it; this home describes a service \
                 from a newer release, or from an extension that is no longer installed",
            ),

        // Ours: a template that does not render, or a recipe that produced a spec the
        // supervisor will not take. An override can *reach* both, which is why they are not
        // unreachable, but neither is a mistake a user could have avoided.
        ServiceError::TemplateBroken { .. } | ServiceError::Unrunnable { .. } => {
            Error::new(ErrorCode::Internal, chain(whole)).with_hint(
                "this is a bug in MixEngine's own configuration templates rather than on this \
                 machine; `logs/daemon.log` has the detail a report needs",
            )
        }

        // Never given a code of their own: what the `_` arm below gives every other error.
        ServiceError::IllegalTransition { .. }
        | ServiceError::StateRaced { .. }
        | ServiceError::UnknownServiceState { .. }
        | ServiceError::UnreadableServiceRow { .. }
        | ServiceError::UnreadableServiceDocument { .. }
        | ServiceError::PortsExhausted { .. }
        | ServiceError::ServiceProvidesNothing { .. }
        | ServiceError::PackageProvidesNothing { .. } => {
            Error::new(ErrorCode::Internal, chain(whole))
        }
    }
}
