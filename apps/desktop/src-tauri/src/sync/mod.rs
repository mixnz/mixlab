//! MixLab's sync: the key hierarchy, the record envelope, the transport above them, and the account.
//!
//! **[`crypto`](crate::sync::crypto) touches nothing but byte arrays**, so the promise that the
//! server cannot read a record is checkable by reading one file rather than by trusting a
//! deployment — the design's D1, and
//! [ADR 0045](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0045-mixlab-has-an-account-and-mixengine-does-not.md).
//!
//! [`crypto`](crate::sync::crypto) is the key hierarchy and the envelope.
//! [`wire`](crate::sync::wire), [`merge`](crate::sync::merge), [`chunk`](crate::sync::chunk),
//! [`store`](crate::sync::store) and [`transport`](crate::sync::transport) are one concern each,
//! and [`engine`](crate::sync::engine) puts them together as `pull` and `push`.
//! [`lend`](crate::sync::lend) turns what a module lends into the changes the engine moves, and
//! pulled records back into items — deciding `updatedAt` on the way (D4).
//! [`account`](crate::sync::account) is the account routes, and nothing that decides what to keep.
//! [`saved`](crate::sync::saved) is what survives a restart, and
//! [`session`](crate::sync::session) is the signed-in state that holds a page until a module has
//! written it.
//! [`copy`](crate::sync::copy) carries a whole account to another server, unchanged.
//!
//! **The engine moves ciphertext only** — sealing and opening stay with whoever calls it, so the
//! keyring and a module's plaintext never reach the socket.
//!
//! The D-numbers in this module are those of
//! `docs/specs/2026-09-20-t177-a-copy-only-you-can-read-design.md`.

pub mod account;
pub mod chunk;
pub mod commands;
pub mod copy;
pub mod crypto;
pub mod engine;
pub mod lend;
pub mod merge;
pub mod saved;
pub mod session;
pub mod store;
pub mod transport;
pub mod wire;

/// The error every call here returns, nameable outside the crate: `tests/sync_scenarios.rs` puts a
/// server of its own behind [`transport::Remote`], whose methods say `AppError` in their signature.
pub use crate::error::AppError;
