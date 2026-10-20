//! What the five SQL engines — MySQL, PostgreSQL, SQL Server, ClickHouse and SQLite — share: the
//! shapes that cross to the window, and the one trait every engine answers through (ADR 0062).
//!
//! `syntax` and `statements` are how they read a script the user typed, ClickHouse excepted.
//!
//! MongoDB and Redis are not here. They have workspaces of their own and share no operation with
//! these worth a trait.

pub mod engine;
pub mod statements;
pub mod syntax;
pub mod types;
