//! `mix project`.

use super::blueprint::next_steps_said;
use mixengine_proto::{
    DeclaredSiteState, PackageVersion, PinSource, ProjectDetail, ProjectExport, ProjectList,
    ProjectRemoval,
};

/// `mix project list` — every registered project, and whether it has a manifest.
pub(crate) fn project_list(list: &ProjectList) -> String {
    if list.projects.is_empty() {
        return "no projects are registered; `mix project create <dir>` adds one\n".to_owned();
    }

    let mut out = format!("{:<24}  {:<9}  {}\n", "PROJECT", "MANIFEST", "ROOT");

    for project in &list.projects {
        out.push_str(&format!(
            "{:<24}  {:<9}  {}\n",
            project.name,
            if project.manifest.is_some() {
                "yes"
            } else {
                "—"
            },
            project.root
        ));
    }

    out
}

/// `mix project show` — one project, and what each pin actually resolves to.
///
/// The **source** column is the whole value of the rendering: a pin read from the manifest outranks
/// the row, so a person looking at a version they did not expect is looking for which of the two is
/// in charge.
pub(crate) fn project_detail(detail: &ProjectDetail) -> String {
    let mut out = format!(
        "{}\n  root      {}\n  created   {}\n",
        detail.project.name, detail.project.root, detail.project.created_at
    );

    if let Some(manifest) = &detail.project.manifest {
        out.push_str(&format!("  manifest  {manifest}\n"));
    }

    if detail.pins.is_empty() {
        out.push_str("\nno runtimes are pinned\n");
    } else {
        out.push_str(&format!(
            "\n{:<8}  {:<10}  {:<10}  {}\n",
            "RUNTIME", "PINNED", "RESOLVES", "FROM"
        ));

        for pin in &detail.pins {
            let from = match &pin.source {
                PinSource::Registered => "this home".to_owned(),
                PinSource::Manifest { path } => path.clone(),
            };

            out.push_str(&format!(
                "{:<8}  {:<10}  {:<10}  {}\n",
                pin.kind.as_str(),
                pin.constraint.as_str(),
                pin.resolved.as_ref().map_or("—", PackageVersion::as_str),
                from
            ));
        }

        for hint in detail.pins.iter().filter_map(|pin| pin.hint.as_ref()) {
            out.push_str(&format!("\n{hint}\n"));
        }
    }

    // **T204, D7.** What mixengine.toml declares, and the command for each one not here yet.
    if !detail.declared_sites.is_empty() {
        out.push_str(&format!("\n{:<28}  {}\n", "IN MIXENGINE.TOML", "HERE"));

        for site in &detail.declared_sites {
            let here = match &site.state {
                DeclaredSiteState::Here => "yes".to_owned(),
                DeclaredSiteState::Missing => {
                    format!("no; `mix site create --from {}` adopts it", site.domain)
                }
                DeclaredSiteState::Elsewhere { owner } => format!("no; {owner} holds it"),
            };
            out.push_str(&format!("{:<28}  {here}\n", site.domain));
        }
    }

    // **T205, D6.** What the project's blueprint says is left to do.
    if let Some(steps) = &detail.next_steps {
        out.push_str(&next_steps_said(steps));
    }

    out
}

/// `mix project delete` — and the directory it did not touch.
pub(crate) fn project_removal(removal: &ProjectRemoval) -> String {
    let mut out = format!("{} is no longer registered\n", removal.removed.name);
    out.push_str(&format!("  the directory is kept: {}\n", removal.root_kept));

    if let Some(manifest) = &removal.manifest_kept {
        out.push_str(&format!("  so is its manifest:   {manifest}\n"));
    }

    out
}

/// `mix project export` — which file, and whether it had to be made.
pub(crate) fn project_export(exported: &ProjectExport) -> String {
    let mut out = match exported.created {
        true => format!("wrote {}\n", exported.path),
        false => format!(
            "updated {}; everything else in it is untouched\n",
            exported.path
        ),
    };

    // **T204, D4.** What the file still declares that this home does not have, left as written.
    if !exported.sites_kept.is_empty() {
        let names = match exported.sites_kept.len() {
            1 => "that name",
            _ => "those names",
        };
        out.push_str(&format!(
            "kept {} in mixengine.toml, though no site here has {names}\n",
            exported.sites_kept.join(", ")
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **T204, D4.** An export names the entries it left, and says nothing when there are none.
    #[test]
    fn an_export_names_the_site_entries_it_kept() {
        let mut exported = ProjectExport {
            path: "/srv/blog/mixengine.toml".to_owned(),
            created: false,
            sites_kept: Vec::new(),
        };
        assert!(!project_export(&exported).contains("kept"));

        exported.sites_kept = vec!["old.test".to_owned()];
        assert!(
            project_export(&exported)
                .contains("kept old.test in mixengine.toml, though no site here has that name"),
            "{}",
            project_export(&exported)
        );
    }

    /// **T204, D7.** The declared sites print even with no pins, each with what to do.
    #[test]
    fn a_project_lists_the_sites_its_manifest_declares() {
        let detail = ProjectDetail {
            project: mixengine_proto::ProjectSummary {
                name: "shop".to_owned(),
                root: "/srv/shop".to_owned(),
                created_at: "2026-10-08T00:00:00Z".to_owned(),
                manifest: Some("/srv/shop/mixengine.toml".to_owned()),
                keep_warm: false,
            },
            pins: Vec::new(),
            declared_sites: vec![
                mixengine_proto::DeclaredSite {
                    domain: "web.test".into(),
                    aliases: Vec::new(),
                    state: mixengine_proto::DeclaredSiteState::Here,
                },
                mixengine_proto::DeclaredSite {
                    domain: "api.test".into(),
                    aliases: Vec::new(),
                    state: mixengine_proto::DeclaredSiteState::Missing,
                },
                mixengine_proto::DeclaredSite {
                    domain: "taken.test".into(),
                    aliases: Vec::new(),
                    state: mixengine_proto::DeclaredSiteState::Elsewhere {
                        owner: "blog".into(),
                    },
                },
            ],
            next_steps: None,
        };

        let out = project_detail(&detail);

        assert!(out.contains("no runtimes are pinned"), "{out}");
        assert!(out.contains("mix site create --from api.test"), "{out}");
        assert!(out.contains("blog holds it"), "{out}");
    }
}
