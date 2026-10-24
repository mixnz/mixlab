//! Certificates: the authority this home signs with, and the leaves it signs.
//!
//! Domain logic only, as everything in this crate is. Writing the private key with permissions
//! nobody else can read through is [`mixengine_platform::write_private`], because a file mode is an
//! operating system's business and this crate makes no OS calls.
//!
//! **Three modules with one shape.** [`leaf`] exports the same four things [`ca`] does — two paths,
//! `ensure`, `read` — so that everything a reader has learned about one transfers: damage is
//! reported and never repaired, `read` describes the disk rather than the last write, and the
//! private key is written first. [`service`] is the same four for a leaf that belongs to a managed
//! server rather than to a site, and it is [`leaf`]'s own body behind a second pair of paths rather
//! than a copy of it.

pub mod ca;
pub mod leaf;
pub mod service;
