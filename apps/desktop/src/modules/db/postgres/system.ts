/** The databases PostgreSQL keeps for itself.
 *
 * A shorter list than MySQL's, because the catalogue is not a database here: `pg_catalog` and
 * `information_schema` are schemas inside every database, and the backend leaves them out of the
 * table list rather than offering them as something to select.
 *
 * What is left is `postgres`, the maintenance database every server is created with and the one
 * MixLab dials when the connection form names none. Dropping it is not fatal the way dropping
 * `mysql` is, but it is what tools connect to when they need a database to connect to, so it is
 * held to the same rule. The two templates never reach this list — see `list_databases`. */
const SYSTEM_DATABASES = new Set(["postgres", "template0", "template1"]);

/** Whether the database belongs to the server rather than to the user. Matched case-insensitively,
 *  though PostgreSQL — unlike MySQL on Windows — does not fold these names itself. */
export function isPostgresSystemDatabase(database: string): boolean {
  return SYSTEM_DATABASES.has(database.toLowerCase());
}
