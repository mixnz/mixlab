use super::*;
use crate::ProjectError;

fn somewhere() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary directory")
}

fn export(sites: Vec<ExportSite>) -> Export {
    Export {
        name: "blog".to_owned(),
        pins: pins(&[(RuntimeKind::Php, "^8.3")]),
        sites,
    }
}

fn site(domain: &str, aliases: &[&str]) -> ExportSite {
    ExportSite {
        domain: domain.to_owned(),
        aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
        doc_root: String::new(),
        https: false,
        kind: mixengine_proto::SiteKind::Static,
        routes: Vec::new(),
        services: Vec::new(),
    }
}

fn linked(id: &str, version: &str) -> ExportService {
    let link = ServiceId::parse(id).expect("an id");
    ExportService {
        name: link.name().to_owned(),
        instance: link.instance().unwrap_or("main").to_owned(),
        link,
        version: Some(PackageVersion::parse(version).expect("a version")),
    }
}

mod choosing;
mod reading;
mod writing;

fn pins(entries: &[(RuntimeKind, &str)]) -> BTreeMap<RuntimeKind, VersionConstraint> {
    entries
        .iter()
        .map(|(kind, text)| {
            (
                *kind,
                VersionConstraint::parse((*text).to_owned()).expect("a constraint"),
            )
        })
        .collect()
}
