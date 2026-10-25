//! The `extension.toml` fixtures.
//!
//! **These are the manifests the first installs shipped**, not examples written to fit the parser.
//! A format proved against files invented for it proves only that it is self-consistent; these are
//! the kinds as the products they describe actually need them, which is where a format finds out
//! what it forgot — and it did: the first draft's `[web-app].root = "{install_dir}/app"` and its
//! `template` field were both wrong about the real artifacts.
//!
//! **Every one that downloads something carries the real hash**, which the manifest format once
//! said none ever would. That rule was written when a hash here could only go stale; what changed
//! is that these are now the same bytes `mixnz/mixengine-packages` publishes under
//! `data/extensions/`, and a fixture that agreed with the roster about everything except the one
//! field somebody would copy is a trap rather than a precaution. Nothing in this workspace
//! downloads one of these, so a hash that goes stale costs a diff and no red test; what proves the
//! published roster against upstream is that repository's own `check-extensions.yml`.
//!
//! `sendmail.toml` has no hash: a `recipe` downloads nothing.

/// A `service` that also carries a recipe, in one file.
pub const MAILPIT: &str = include_str!("../fixtures/extensions/mailpit.toml");

/// A `web-app` on a runtime MixEngine picks, never the user's project version — and the one
/// manifest that declares `[web-app.database].signs_in`.
pub const PHPMYADMIN: &str = include_str!("../fixtures/extensions/phpmyadmin.toml");

/// A `web-app` whose artifact is **one file** rather than an archive, and
/// whose generated `index.php` is what gives Adminer a default server.
pub const ADMINER: &str = include_str!("../fixtures/extensions/adminer.toml");

/// A `recipe` and nothing else.
pub const SENDMAIL: &str = include_str!("../fixtures/extensions/sendmail.toml");

/// A `service` with eight ports, every one passed with its own flag. Its gRPC ports are the reason:
/// `weed` opens one beside each port it is given unless told which, and a port MixEngine did not
/// allocate is a port nothing stops something else holding.
pub const SEAWEEDFS: &str = include_str!("../fixtures/extensions/seaweedfs.toml");
