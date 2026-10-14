use super::*;
use crate::BlueprintError;

fn a_manifest() -> BlueprintManifest {
    BlueprintManifest {
        schema: 1,
        blueprint: Header {
            name: "laravel-php82".to_owned(),
            description: "Laravel + MariaDB".to_owned(),
            created_at: "2026-09-01T09:00:00Z".to_owned(),
            created_on: Provenance {
                os: "windows".to_owned(),
                version: "0.1.0".to_owned(),
            },
        },
        runtimes: [(
            RuntimeKind::Php,
            VersionConstraint::parse("8.2.23").expect("a constraint"),
        )]
        .into_iter()
        .collect(),
        sites: vec![BlueprintSite {
            kind: SiteKind::PhpFpm { pool: None },
            doc_root: "public".to_owned(),
            https: true,
            domain_pattern: "{project}.test".to_owned(),
            aliases: vec!["api.{project}.test".to_owned()],
            routes: Vec::new(),
            services: None,
        }],
        services: vec![BlueprintService {
            name: "mariadb".to_owned(),
            version: Some(VersionConstraint::parse("11.4.3").expect("a constraint")),
            instance: Some("main".to_owned()),
            database: Some("{project}".to_owned()),
            user: Some("{project}".to_owned()),
            dotenv: None,
        }],
        php: Some(Php {
            extensions: vec!["redis".to_owned(), "xdebug".to_owned()],
        }),
        scaffold: None,
        archive: None,
        next_steps: Vec::new(),
    }
}

/// A manifest with every section this feature has, in the order the renderer writes them.
///
/// Taken from the renderer's own output rather than written by hand, which is what makes the
/// assertion below about the *format* rather than about somebody's typing.
const GALLERY_SHAPED: &str = r#"schema = 1

[blueprint]
name = "laravel-php82"
description = "Laravel + MariaDB"
created_at = "2026-09-01T09:00:00Z"

[blueprint.created_on]
os = "windows"
version = "0.1.0"

[runtimes]
php = "8.2.23"

[site]
kind = "php-fpm"
doc_root = "public"
https = true
domain_pattern = "{project}.test"
aliases = ["api.{project}.test"]

[[services]]
name = "mariadb"
version = "11.4.3"
instance = "main"
database = "{project}"
user = "{project}"

[php]
extensions = ["redis", "xdebug"]

[scaffold]
command = "composer create-project laravel/laravel {project}"
"#;

/// **The rendering is not the signed artifact, and in practice it is the same bytes** — roadmap
/// task **T78a**, its design's D16. Nothing depends on this: trust is decided over the bytes
/// that were handed in, once, at import. But a gallery file that came back differently would
/// mean anybody checking a `.minisig` against `blueprints/<slug>.toml` finds a failure with no
/// tampering behind it, and this is what T77's byte-identical renderer was for.
#[test]
fn a_gallery_shaped_manifest_renders_back_byte_for_byte() {
    let read_back = read(GALLERY_SHAPED).expect("it parses");

    assert_eq!(render(&read_back), GALLERY_SHAPED);
}

/// **A command asks for an empty directory by saying so, and otherwise asks for nothing.**
///
/// The default has to be `false` or every `[scaffold]` written before this key existed would
/// start refusing directories its author was happy to run in — `composer install` on a cloned
/// tree is a scaffold, and a blueprint that carried one is not this key's business.
#[test]
fn a_scaffold_asks_for_nothing_about_the_directory_unless_it_says_so() {
    let read_back = read(GALLERY_SHAPED).expect("it parses");

    assert!(!read_back.scaffold.expect("a scaffold").needs_empty_dir);
}

/// **A site's routes survive a render and a read** — roadmap task **T135**, that design's D11.
///
/// The one property this test is really about is the ordering the renderer's own note explains:
/// TOML puts a table after the values of the table it sits in, so an array of tables written
/// before `doc_root` would take every key after it into the wrong table. Reading the render back
/// is what would catch that.
#[test]
fn a_sites_routes_survive_the_round_trip() {
    let routes = vec![
        mixengine_proto::SiteRoute {
            path: "/api".to_owned(),
            target: mixengine_proto::RouteTarget::Proxy {
                upstream: "http://127.0.0.1:3003/xyz".to_owned(),
            },
        },
        mixengine_proto::SiteRoute {
            path: "/admin".to_owned(),
            target: mixengine_proto::RouteTarget::PhpFpm { pool: None },
        },
        mixengine_proto::SiteRoute {
            path: "/assets".to_owned(),
            target: mixengine_proto::RouteTarget::Static {
                root: "dist".to_owned(),
            },
        },
    ];

    let mut manifest = a_manifest();
    manifest.sites.first_mut().expect("a site").routes = routes.clone();

    let rendered = render(&manifest);
    let read_back = read(&rendered).expect("it parses");
    let site = read_back.sites.first().expect("a site");

    assert_eq!(site.routes, routes);
    assert_eq!(
        site.doc_root, "public",
        "a key written before the array of tables is still in the site table:\n{rendered}"
    );
    assert_eq!(site.kind, SiteKind::PhpFpm { pool: None });
    assert_eq!(render(&read_back), rendered, "and it renders back the same");
}

/// And a manifest that does say so renders it back, which is what lets one travel.
#[test]
fn a_scaffold_that_asks_for_an_empty_directory_survives_the_round_trip() {
    let asking = GALLERY_SHAPED.replace(
        "command = \"composer create-project laravel/laravel {project}\"\n",
        "command = \"composer create-project laravel/laravel {project}\"\n\
             needs_empty_dir = true\n",
    );

    let read_back = read(&asking).expect("it parses");

    assert!(
        read_back
            .scaffold
            .as_ref()
            .expect("a scaffold")
            .needs_empty_dir
    );
    assert_eq!(render(&read_back), asking);
}

/// **A command that names itself after its directory says so too** — roadmap task **T120c**.
/// Written only when true, like its neighbour, so a manifest captured or imported before this
/// key existed renders back exactly the bytes it arrived as.
#[test]
fn a_scaffold_that_needs_an_npm_safe_directory_survives_the_round_trip() {
    let asking = GALLERY_SHAPED.replace(
        "command = \"composer create-project laravel/laravel {project}\"\n",
        "command = \"composer create-project laravel/laravel {project}\"\n\
             needs_empty_dir = true\n\
             needs_npm_safe_dir = true\n",
    );

    let read_back = read(&asking).expect("it parses");

    assert!(
        read_back
            .scaffold
            .as_ref()
            .expect("a scaffold")
            .needs_npm_safe_dir
    );
    assert_eq!(render(&read_back), asking);
}

/// And a scaffold that says nothing about its directory's name is judged on nothing.
#[test]
fn a_scaffold_says_nothing_about_its_directorys_name_unless_it_says_so() {
    let read_back = read(GALLERY_SHAPED).expect("it parses");

    assert!(!read_back.scaffold.expect("a scaffold").needs_npm_safe_dir);
}

/// What is written can be read, and reading it back gives the same value — the property every
/// later task leans on.
#[test]
fn a_rendered_manifest_reads_back_as_itself() {
    let manifest = a_manifest();
    let rendered = render(&manifest);

    assert_eq!(read(&rendered).expect("it parses"), manifest, "{rendered}");
}

/// Every kind survives the round trip, including the two that carry a payload beside the tag.
#[test]
fn every_site_kind_survives_being_written_and_read() {
    for kind in [
        SiteKind::PhpFpm { pool: None },
        SiteKind::Static,
        SiteKind::ReverseProxy {
            upstream: "http://127.0.0.1:3000".to_owned(),
        },
        SiteKind::NodeApp { port: 3000 },
    ] {
        let mut manifest = a_manifest();
        manifest.sites.first_mut().expect("a site").kind = kind.clone();

        let rendered = render(&manifest);
        let read_back = read(&rendered).expect("it parses");

        assert_eq!(
            read_back.sites[0].kind, kind,
            "{kind:?} did not survive:\n{rendered}"
        );
    }
}

/// **D7.** Two captures of one project must produce two identical files, or a golden test says
/// nothing and a re-capture's diff is noise.
#[test]
fn rendering_is_deterministic_and_puts_the_schema_first() {
    let rendered = render(&a_manifest());

    assert_eq!(rendered, render(&a_manifest()));
    assert!(rendered.starts_with("schema = 1\n"), "{rendered}");
    assert!(
        rendered.find("[blueprint]") < rendered.find("[runtimes]"),
        "{rendered}"
    );
    assert!(
        rendered.find("[[services]]") < rendered.find("[php]"),
        "{rendered}"
    );
}

/// The pool is a fact about the machine that was captured, and `SiteKind::PhpFpm` can carry
/// one. It never goes in.
#[test]
fn a_site_kind_is_written_flat_and_carries_no_pool() {
    let rendered = render(&a_manifest());

    assert!(rendered.contains("kind = \"php-fpm\""), "{rendered}");
    assert!(!rendered.contains("pool"), "{rendered}");
}

/// A file from a build that knew more than this one is refused by name rather than half-read.
#[test]
fn a_newer_schema_is_refused_by_name() {
    let text = render(&a_manifest()).replace("schema = 1", "schema = 4");

    assert!(
        matches!(
            read(&text),
            Err(Error::Blueprint(BlueprintError::UnknownBlueprintSchema { schema: 4, ref name })) if name == "laravel-php82"
        ),
        "{:?}",
        read(&text)
    );
}

/// A `[site]` without a name to answer to is not a site, and saying so beats defaulting to
/// something the author did not write.
#[test]
fn a_site_without_a_domain_pattern_does_not_parse() {
    let text = r#"
schema = 1

[blueprint]
name = "x"
created_at = "2026-09-01T09:00:00Z"

[blueprint.created_on]
os = "linux"
version = "0.1.0"

[site]
kind = "static"
doc_root = "public"
"#;

    assert!(matches!(
        read(text),
        Err(Error::Blueprint(BlueprintError::BlueprintManifest { .. }))
    ));
}
fn with_scaffold(table: &str) -> String {
    format!(
        "schema = 2\n\n[blueprint]\nname = \"wp\"\ncreated_at = \"2026-10-08T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n[scaffold]\n{table}\n"
    )
}

fn with_steps(steps: &str) -> String {
    format!(
        "schema = 1\n\n[blueprint]\nname = \"x\"\ncreated_at = \"2026-10-08T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n{steps}\n"
    )
}

fn refusal(text: &str) -> String {
    match read(text) {
        Err(error) => format!("{error:?}"),
        Ok(_) => panic!("read accepted:\n{text}"),
    }
}

#[test]
fn an_archive_scaffold_reads_and_renders_at_schema_2() {
    let text = with_scaffold(
        "archive = \"https://wordpress.org/latest.zip\"\nstrip = \"wordpress\"\nneeds_empty_dir = true",
    );
    let manifest = read(&text).expect("reads");

    assert!(manifest.scaffold.is_none());
    assert_eq!(
        manifest.archive,
        Some(Archive {
            url: "https://wordpress.org/latest.zip".to_owned(),
            strip: Some("wordpress".to_owned()),
            needs_empty_dir: true,
            when_empty: false,
        })
    );
    assert_eq!(schema_of(&manifest), 2);
    assert!(render(&manifest).starts_with("schema = 2\n"));
    assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
}

#[test]
fn a_manifest_without_an_archive_renders_at_schema_1_whatever_it_said() {
    let mut manifest = a_manifest();
    manifest.schema = 2;

    assert_eq!(schema_of(&manifest), 1);
    assert!(render(&manifest).starts_with("schema = 1\n"));
}

#[test]
fn an_archive_and_a_command_together_are_refused() {
    let text = with_scaffold("command = \"composer install\"\narchive = \"https://x.org/a.zip\"");
    assert!(refusal(&text).contains("both"), "{}", refusal(&text));
}

#[test]
fn a_scaffold_with_neither_is_refused() {
    let text = with_scaffold("needs_empty_dir = true");
    assert!(refusal(&text).contains("neither"), "{}", refusal(&text));
}

/// **`when_empty` reads, renders and round-trips** — a starter that is unpacked only into an
/// empty folder and skipped over a clone (T205, `php-mysql` and `static`).
#[test]
fn an_archive_unpacked_only_when_empty_round_trips() {
    let text = with_scaffold(
        "archive = \"https://x.org/a-starter.zip\"\nstrip = \"a\"\nwhen_empty = true",
    );
    let manifest = read(&text).expect("reads");

    let archive = manifest.archive.as_ref().expect("an archive");
    assert!(archive.when_empty);
    assert!(!archive.needs_empty_dir);
    assert!(render(&manifest).contains("when_empty = true"));
    assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
}

/// **`when_empty` and `needs_empty_dir` contradict each other**: one skips the archive over a
/// full folder, the other blocks the apply there.
#[test]
fn when_empty_beside_needs_empty_dir_is_refused() {
    let text = with_scaffold(
        "archive = \"https://x.org/a.zip\"\nneeds_empty_dir = true\nwhen_empty = true",
    );
    assert!(refusal(&text).contains("when_empty"), "{}", refusal(&text));
}

/// **`when_empty` describes an archive.** A command that skipped itself over a full folder
/// would be a scaffold that silently did nothing where it is most often run.
#[test]
fn when_empty_on_a_command_is_refused() {
    let text = with_scaffold("command = \"composer install\"\nwhen_empty = true");
    assert!(refusal(&text).contains("when_empty"), "{}", refusal(&text));
}

fn with_services(services: &str) -> String {
    format!(
        "schema = 2\n\n[blueprint]\nname = \"x\"\ncreated_at = \"2026-10-09T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n{services}\n"
    )
}

/// **`dotenv` reads, renders and round-trips, at schema 2** — roadmap task **T205a**.
#[test]
fn a_dotenv_key_round_trips_at_schema_2() {
    let text = with_services(
        "[[services]]\nname = \"postgres\"\ndatabase = \"{project}\"\nuser = \"{project}\"\n\
             dotenv = \"DATABASE_URL\"",
    );
    let manifest = read(&text).expect("reads");

    assert_eq!(manifest.services[0].dotenv.as_deref(), Some("DATABASE_URL"));
    assert_eq!(schema_of(&manifest), 2);
    assert!(render(&manifest).contains("dotenv = \"DATABASE_URL\""));
    assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
}

#[test]
fn dotenv_without_an_account_is_refused() {
    let text = with_services("[[services]]\nname = \"redis\"\ndotenv = \"REDIS_URL\"");
    assert!(refusal(&text).contains("dotenv"), "{}", refusal(&text));
}

#[test]
fn dotenv_that_is_not_a_variable_name_is_refused() {
    let text = with_services(
        "[[services]]\nname = \"postgres\"\ndatabase = \"d\"\nuser = \"u\"\n\
             dotenv = \"DATABASE-URL\"",
    );
    assert!(
        refusal(&text).contains("DATABASE-URL"),
        "{}",
        refusal(&text)
    );
}

#[test]
fn dotenv_on_a_second_service_is_refused() {
    let text = with_services(
        "[[services]]\nname = \"postgres\"\ndatabase = \"d\"\nuser = \"u\"\ndotenv = \"A\"\n\n\
             [[services]]\nname = \"mysql\"\ndatabase = \"d\"\nuser = \"u\"\ndotenv = \"B\"",
    );
    assert!(refusal(&text).contains("one service"), "{}", refusal(&text));
}

#[test]
fn a_plain_http_archive_is_refused() {
    let text = with_scaffold("archive = \"http://wordpress.org/latest.zip\"");
    assert!(refusal(&text).contains("https"), "{}", refusal(&text));
}

/// **`strip` names one folder at the archive's top level, and nothing else.** A path in it
/// would move whatever it reaches outside the unpacked tree into the project, and the consent
/// a person gives names the URL, not this.
#[test]
fn a_strip_that_is_not_one_folder_name_is_refused() {
    for strip in [
        "..",
        "../outside",
        "a/b",
        "a\\\\b",
        "C:\\\\Users",
        "/etc",
        ".",
        "",
    ] {
        let text = with_scaffold(&format!(
            "archive = \"https://x.org/a.zip\"\nstrip = \"{strip}\""
        ));
        assert!(
            refusal(&text).contains("strip"),
            "{strip}: {}",
            refusal(&text)
        );
    }
}

#[test]
fn an_npm_name_check_beside_an_archive_is_refused() {
    let text = with_scaffold("archive = \"https://x.org/a.zip\"\nneeds_npm_safe_dir = true");
    assert!(
        refusal(&text).contains("needs_npm_safe_dir"),
        "{}",
        refusal(&text)
    );
}

#[test]
fn next_steps_read_render_and_do_not_raise_the_schema() {
    let text = with_steps(
        "[[next_steps]]\nkind = \"once\"\nrun = \"npm install\"\n\n\
             [[next_steps]]\nkind = \"serve\"\nrun = \"npm run dev\"\nnote = \"port 3000\"\n\n\
             [[next_steps]]\nkind = \"open\"\npath = \"/wp-admin/install.php\"\ncredentials = true\noptional = true",
    );
    let manifest = read(&text).expect("reads");

    assert_eq!(manifest.next_steps.len(), 3);
    assert_eq!(manifest.next_steps[1].note.as_deref(), Some("port 3000"));
    assert!(manifest.next_steps[2].credentials);
    assert_eq!(schema_of(&manifest), 1);
    assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
}

#[test]
fn an_unknown_key_inside_a_step_is_ignored() {
    let text = with_steps(
        "[[next_steps]]\nkind = \"once\"\nrun = \"npm install\"\ndone_when = \"node_modules\"",
    );
    assert_eq!(read(&text).expect("reads").next_steps.len(), 1);
}

#[test]
fn every_run_rule_is_refused_naming_the_step() {
    for bad in [
        "npm install && npm run dev",
        "npm install || true",
        "cd x; npm i",
        "ls | wc",
        "echo > x",
        "sort < x",
        "echo `id`",
        "echo $HOME",
        "echo %PATH%",
        r"vendor\bin\pest",
        "",
    ] {
        let text = with_steps(&format!(
            "[[next_steps]]\nkind = \"once\"\nrun = \"npm install\"\n\n\
                 [[next_steps]]\nkind = \"once\"\nrun = {bad:?}"
        ));
        let said = refusal(&text);
        assert!(said.contains("next_steps[2]"), "{bad:?}: {said}");
    }

    let long = "a".repeat(513);
    let text = with_steps(&format!(
        "[[next_steps]]\nkind = \"once\"\nrun = \"{long}\""
    ));
    assert!(refusal(&text).contains("512"));
}

#[test]
fn a_token_other_than_project_is_refused() {
    let text = with_steps("[[next_steps]]\nkind = \"once\"\nrun = \"echo {domain}\"");
    assert!(refusal(&text).contains("{project}"));
}

#[test]
fn run_on_open_and_path_elsewhere_are_refused() {
    assert!(
        refusal(&with_steps(
            "[[next_steps]]\nkind = \"open\"\nrun = \"npm i\""
        ))
        .contains("open")
    );
    assert!(
        refusal(&with_steps(
            "[[next_steps]]\nkind = \"once\"\nrun = \"npm i\"\npath = \"/x\""
        ))
        .contains("path")
    );
    assert!(refusal(&with_steps("[[next_steps]]\nkind = \"serve\"")).contains("run"));
    assert!(refusal(&with_steps("[[next_steps]]\nkind = \"open\"\npath = \"x\"")).contains("/"));
}

#[test]
fn a_site_on_a_step_is_refused_when_the_blueprint_has_one() {
    let text = with_steps("[[next_steps]]\nkind = \"once\"\nrun = \"npm i\"\nsite = \"web\"");
    assert!(refusal(&text).contains("site"));
}

fn with_body(schema: u32, body: &str) -> String {
    format!(
        "schema = {schema}\n\n[blueprint]\nname = \"x\"\ncreated_at = \"2026-10-09T00:00:00Z\"\n\n\
             [blueprint.created_on]\nos = \"any\"\nversion = \"0.0.1\"\n\n{body}\n"
    )
}

const TWO_SITES: &str = "[[sites]]\nkind = \"static\"\ndoc_root = \"\"\nhttps = true\n\
        domain_pattern = \"{project}.test\"\nservices = [\"redis\"]\n\n\
        [[sites]]\nkind = \"reverse-proxy\"\nupstream = \"http://127.0.0.1:5173\"\ndoc_root = \"\"\n\
        https = true\ndomain_pattern = \"vite.{project}.test\"\nservices = []\n\n\
        [[services]]\nname = \"redis\"\ninstance = \"main\"\n";

/// **T204a, D1.** `[[sites]]` reads as its entries, in order, each with its own links.
#[test]
fn several_sites_read_in_order_with_their_links() {
    let manifest = read(&with_body(3, TWO_SITES)).expect("reads");

    assert_eq!(manifest.sites.len(), 2);
    assert_eq!(manifest.sites[0].domain_pattern, "{project}.test");
    assert_eq!(manifest.sites[0].services, Some(vec!["redis".to_owned()]));
    assert_eq!(manifest.sites[1].services, Some(Vec::new()));
    assert_eq!(schema_of(&manifest), 3);

    let rendered = render(&manifest);
    assert!(rendered.contains("[[sites]]"), "{rendered}");
    assert_eq!(read(&rendered).expect("round trip"), manifest);
}

/// **T204a, D1.** `[site]` is still one entry, and links every service.
#[test]
fn a_single_site_table_is_one_entry_with_no_list_of_links() {
    let manifest = read(GALLERY_SHAPED).expect("reads");

    assert_eq!(manifest.sites.len(), 1);
    assert_eq!(manifest.sites[0].services, None);
}

#[test]
fn both_forms_in_one_file_are_refused() {
    let text = with_body(
        3,
        &format!("[site]\nkind = \"static\"\ndomain_pattern = \"a.test\"\n\n{TWO_SITES}"),
    );
    let said = refusal(&text);

    assert!(
        said.contains("[site]") && said.contains("[[sites]]"),
        "{said}"
    );
}

/// **T204a, D1.** An older build would apply this as a project with no site.
#[test]
fn several_sites_under_an_older_schema_are_refused_naming_3() {
    for schema in [1, 2] {
        let said = refusal(&with_body(schema, TWO_SITES));
        assert!(said.contains("schema = 3"), "{said}");
    }
}

#[test]
fn a_name_two_entries_answer_to_is_refused_with_both_positions() {
    let text = with_body(
        3,
        "[[sites]]\nkind = \"static\"\ndomain_pattern = \"{project}.test\"\n\n\
             [[sites]]\nkind = \"static\"\ndomain_pattern = \"api.{project}.test\"\n\
             aliases = [\"{project}.test\"]\n",
    );
    let said = refusal(&text);

    assert!(
        said.contains("sites[1]") && said.contains("sites[2]"),
        "{said}"
    );
}

/// **T204a, D3.** An item names one `[[services]]` entry, by name or by `name@instance`.
#[test]
fn a_link_names_exactly_one_service() {
    let services = |item: &str| {
        with_body(
            3,
            &format!(
                "[[sites]]\nkind = \"static\"\ndomain_pattern = \"a.test\"\nservices = [{item}]\n\n\
                     [[sites]]\nkind = \"static\"\ndomain_pattern = \"b.test\"\n\n\
                     [[services]]\nname = \"mariadb\"\ninstance = \"main\"\n\n\
                     [[services]]\nname = \"mariadb\"\ninstance = \"per-project\"\n"
            ),
        )
    };

    assert!(refusal(&services("\"redis\"")).contains("redis"));
    assert!(refusal(&services("\"mariadb\"")).contains("mariadb@"));
    assert!(read(&services("\"mariadb@per-project\"")).is_ok());
}

#[test]
fn services_on_a_single_site_table_is_refused() {
    let text = with_body(
        1,
        "[site]\nkind = \"static\"\ndomain_pattern = \"a.test\"\nservices = []\n",
    );

    assert!(refusal(&text).contains("services"));
}

/// **T204a, D2.** One entry with no links is a `[site]` again, at the lowest schema.
#[test]
fn one_entry_without_links_renders_as_a_single_site_table() {
    let text = with_body(
        3,
        "[[sites]]\nkind = \"static\"\ndomain_pattern = \"a.test\"\n",
    );
    let manifest = read(&text).expect("reads");
    let rendered = render(&manifest);

    assert_eq!(schema_of(&manifest), 1);
    assert!(
        rendered.contains("[site]") && !rendered.contains("[[sites]]"),
        "{rendered}"
    );
}

/// **T204a, D2.** One entry naming its links keeps them, so it stays `[[sites]]` at 3.
#[test]
fn one_entry_with_links_stays_a_list() {
    let text = with_body(
        3,
        "[[sites]]\nkind = \"static\"\ndomain_pattern = \"a.test\"\nservices = []\n",
    );
    let manifest = read(&text).expect("reads");

    assert_eq!(schema_of(&manifest), 3);
    assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
}

/// **T204a, D2.** Routes nest under their own entry, after its links.
#[test]
fn a_routed_entry_round_trips_among_several() {
    let mut manifest = read(&with_body(3, TWO_SITES)).expect("reads");
    manifest.sites[0].routes = vec![mixengine_proto::SiteRoute {
        path: "/api".to_owned(),
        target: mixengine_proto::RouteTarget::Proxy {
            upstream: "http://127.0.0.1:3000".to_owned(),
        },
    }];

    let rendered = render(&manifest);
    assert_eq!(read(&rendered).expect("round trip"), manifest, "{rendered}");
}

#[test]
fn a_fourth_schema_is_refused_by_name() {
    let error = read(&with_body(4, "")).expect_err("refused");

    assert!(matches!(
        error,
        Error::Blueprint(BlueprintError::UnknownBlueprintSchema { schema: 4, .. })
    ));
}

/// **T204a, D5.** With several sites every step names one, by its `domain_pattern`.
#[test]
fn a_step_names_its_site_when_there_are_several() {
    let step = |site: &str| {
        with_body(
            3,
            &format!(
                "{TWO_SITES}\n[[next_steps]]\nkind = \"serve\"\nrun = \"npm run dev\"{site}\n"
            ),
        )
    };

    assert!(refusal(&step("")).contains("site"));
    assert!(refusal(&step("\nsite = \"web\"")).contains("vite.{project}.test"));

    let manifest = read(&step("\nsite = \"vite.{project}.test\"")).expect("reads");
    assert_eq!(
        manifest.next_steps[0].site.as_deref(),
        Some("vite.{project}.test")
    );
    assert_eq!(read(&render(&manifest)).expect("round trip"), manifest);
}
