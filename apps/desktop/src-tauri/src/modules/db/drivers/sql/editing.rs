//! Editing a table's rows from the grid, for the engines that reach their server through sqlx:
//! MySQL, PostgreSQL and SQLite. Each engine writes the SQL of the statements; this is where they
//! run.

#[cfg(test)]
mod statements;
