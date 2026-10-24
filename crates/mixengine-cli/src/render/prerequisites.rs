//! What an install lacks on this machine, printed.

use mixengine_proto::Requirement;

/// What an install lacks here, one line each with what can be done.
pub(crate) fn requirements(unmet: &[Requirement]) -> String {
    let mut rendered = String::from("this machine lacks what it needs:\n");
    for requirement in unmet {
        rendered.push_str(&format!(
            "  - {}: {}\n",
            requirement.need, requirement.remedy
        ));
    }
    rendered
}

/// What an install may lack that refuses nothing.
pub(crate) fn advisories(advisories: &[Requirement]) -> String {
    let named: Vec<String> = advisories
        .iter()
        .map(|requirement| requirement.need.label())
        .collect();

    format!(
        "warning: this machine's loader does not list {}; install them with this distribution's \
         package manager; the install goes on\n",
        named.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{Need, PackageVersion, Remedy};

    use super::*;

    #[test]
    fn what_is_lacking_is_said_with_what_can_be_done() {
        let rendered = requirements(&[Requirement {
            need: Need::Macos {
                at_least: "14.0".to_owned(),
                found: "13.6".to_owned(),
            },
            remedy: Remedy::ChooseVersion {
                version: PackageVersion::parse("8.3.33").unwrap(),
            },
        }]);

        assert!(
            rendered.contains("macOS 14.0 or newer, and this Mac runs 13.6"),
            "{rendered}"
        );
        assert!(
            rendered.contains("8.3.33 is the newest release that runs here"),
            "{rendered}"
        );
    }

    /// **A warning names the libraries and says the install goes on.**
    #[test]
    fn a_warning_names_the_libraries_and_says_the_install_goes_on() {
        let said = advisories(&[Requirement {
            need: Need::SharedLibrary {
                soname: "libasound.so.2".to_owned(),
            },
            remedy: Remedy::InstallFromDistribution,
        }]);

        assert!(said.starts_with("warning: "), "{said}");
        assert!(said.contains("libasound.so.2"), "{said}");
        assert!(said.contains("goes on"), "{said}");
    }
}
