//! The fixtures every test suite in this workspace shares.
//!
//! Seven things live here: a home directory that exists only for the test that made it, a way to
//! stop a process this test is not the parent of, the `fakeservice` binary the supervisor is
//! tested against (`docs/standards/testing.md`,
//! `docs/architecture/process-supervision.md`), the rows a test has to write for itself because
//! no method produces them, a signed package index over a real socket, and a real archive to
//! install from it.
//! The first two were each written twice somewhere else before they were written once here; the third arrives here first, because the four crates that will spawn it
//! could not have shared it anywhere but a package of its own; the fourth is what no call can do
//! for a test — see [`mod@declare`]; the next two exist because the network is forbidden in
//! tests and what they stand in for is a network; and the seventh is a FastCGI client, because a
//! test that proves php-fpm is up by connecting to its socket proves only that something is
//! listening — see [`mod@fastcgi`].
//!
//! Two more were each copied into a dozen suites before they were written once here: a
//! [`Daemon`] a test started, from the binary path the suite hands it, and the [`Client`] that
//! talks to it over the API.
//!
//! **A dev-dependency and nothing else.** Nothing in this crate may end up inside `mixengined`,
//! `mix` or `mixengine-elevate`, which `crates/mixengine-proto/tests/workspace_layering.rs` checks
//! rather than trusts. That is what makes the exception in [`process`] affordable: it is a `#[cfg]`
//! that ships to nobody.
//!
//! What is deliberately *not* here is anything that would answer a question a test is asking. A
//! fixture that computed a path the way the daemon computes it would make a suite agree with itself
//! by construction — so [`Home`] restates the three conventions it needs, out loud, and the tests
//! that care keep the two answers side by side. See [`Home::run_dir`].

#![warn(missing_docs)]

pub mod client;
pub mod create;
pub mod daemon;
pub mod declare;
pub mod extension;
pub mod fastcgi;
pub mod home;
pub mod package;
pub mod process;
pub mod registry;
pub mod service;
pub mod signing;
pub mod upgrade;

pub use client::Client;
pub use create::{call, create, create_blocking};
pub use daemon::Daemon;
pub use declare::{Service, VERSION};
pub use home::Home;
pub use package::{FakePackage, Packed, Packing};
pub use process::{end_daemon, kill, stop, try_kill, try_stop};
pub use registry::MockRegistry;
pub use service::FakeService;
pub use signing::Signer;
