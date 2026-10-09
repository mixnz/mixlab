//! What a project still needs a person to do, read through its blueprint — roadmap task **T205**,
//! D5 and D6.
//!
//! **A reference, not a copy.** The steps live in the blueprint row, with `{project}` unexpanded,
//! and are expanded here each time they are read. A corrected blueprint corrects its projects, and
//! capture never has to turn a slug back into a token — a replacement that would rewrite any word
//! that merely contains the slug.

use mixengine_proto::{NextStep, NextSteps};

use crate::blueprints::manifest::{self, BlueprintManifest};
use crate::projects::ProjectRecord;
use crate::{Result, Store};

/// `steps` with `{project}` replaced by the slug of `project` (ADR 0030).
///
/// A name with nothing to slug leaves the token in place, as the plan does for `[scaffold]`.
#[must_use]
pub fn expanded(steps: &[NextStep], project: &str) -> Vec<NextStep> {
    let handle = crate::domains::slug(project);
    let expand = |value: &Option<String>| {
        value
            .as_deref()
            .map(|text| crate::blueprints::plan::expand(text, handle.as_deref()))
    };

    steps
        .iter()
        .map(|step| NextStep {
            run: expand(&step.run),
            path: expand(&step.path),
            note: expand(&step.note),
            // A domain pattern, and a client opens the domain — roadmap task **T204a**, D5.
            site: expand(&step.site),
            ..step.clone()
        })
        .collect()
}

/// A manifest's steps for `project`, or [`None`] when it has none.
#[must_use]
pub fn of_manifest(
    manifest: &BlueprintManifest,
    project: &str,
    trusted: bool,
) -> Option<NextSteps> {
    if manifest.next_steps.is_empty() {
        return None;
    }

    Some(NextSteps {
        trusted,
        steps: expanded(&manifest.next_steps, project),
    })
}

/// The blueprint a project was made from, with its row's `trusted`, or [`None`].
///
/// # Errors
///
/// [`crate::Error::Database`] for the read, [`crate::Error::BlueprintManifest`] for a row that does
/// not parse.
pub async fn declared(store: &Store, project_id: i64) -> Result<Option<(BlueprintManifest, bool)>> {
    let row = sqlx::query!(
        r#"SELECT b.manifest_toml AS "manifest!: String", b.trusted AS "trusted!: bool"
           FROM projects p JOIN blueprints b ON b.id = p.blueprint_id
           WHERE p.id = ?1"#,
        project_id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    row.map(|row| Ok((manifest::read(&row.manifest)?, row.trusted)))
        .transpose()
}

/// What `project.show` answers in `next_steps` (D6).
///
/// # Errors
///
/// As [`declared`].
pub async fn of_project(store: &Store, project: &ProjectRecord) -> Result<Option<NextSteps>> {
    Ok(declared(store, project.id)
        .await?
        .and_then(|(manifest, trusted)| of_manifest(&manifest, &project.name, trusted)))
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{NextStep, NextStepKind};

    use super::*;

    fn once(run: &str) -> NextStep {
        NextStep {
            kind: NextStepKind::Once,
            run: Some(run.to_owned()),
            path: None,
            note: None,
            optional: false,
            credentials: false,
            site: None,
        }
    }

    /// **T204a, D5.** A step's site is a domain pattern, and a client opens the domain.
    #[test]
    fn a_steps_site_is_expanded_with_the_rest() {
        let mut step = once("npm run dev");
        step.site = Some("vite.{project}.test".to_owned());

        let steps = expanded(&[step], "My Blog");
        assert_eq!(steps[0].site.as_deref(), Some("vite.my-blog.test"));
    }

    #[test]
    fn expands_through_the_slug_not_the_name() {
        let steps = expanded(&[once("echo {project}")], "My Blog");
        assert_eq!(steps[0].run.as_deref(), Some("echo my-blog"));
    }

    #[test]
    fn a_name_with_nothing_to_slug_leaves_the_token() {
        let steps = expanded(&[once("echo {project}")], "日本");
        assert_eq!(steps[0].run.as_deref(), Some("echo {project}"));
    }

    #[test]
    fn a_manifest_without_steps_has_none() {
        let manifest = crate::blueprints::manifest::read(
            crate::blueprints::gallery::ENTRIES
                .iter()
                .find(|entry| entry.slug == "static")
                .expect("static")
                .manifest,
        )
        .expect("reads");

        assert_eq!(of_manifest(&manifest, "x", true), None);
    }
}
