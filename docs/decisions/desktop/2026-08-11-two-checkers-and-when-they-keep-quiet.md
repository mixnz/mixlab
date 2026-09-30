# Two checkers for the Query tab, and when each keeps quiet

The Query tab underlines what is wrong with a script. Two things do that, and the interesting part
of the design is not what they catch — it is what they deliberately let past.

## The two

**Instant, on the client.** [`src/modules/db/sql/lint.ts`](../../../apps/desktop/src/modules/db/sql/lint.ts) tokenises every statement
on every pause in the typing and compares the names in it against the schema outline completion is
already working from. It costs nothing and covers the whole script.

**Accurate, on the server.** `mysql_validate_sql` hands one statement to MySQL and asks it to
`PREPARE` the text, then throws the plan away. `PREPARE` parses and plans; nothing executes. That is
what makes it safe to fire at a half-typed `DELETE`, and it was checked rather than assumed — a
`DELETE` and a `CREATE TABLE` put through it on 5.7.44 and on 8.4.8 left the rows and the schema
exactly as they were.

`PREPARE` will not take a placeholder for the text it prepares, so the statement goes into a user
variable first (`SET @<old>_check = ?`), which *can* be bound. No user text is ever interpolated
into SQL.

Neither is enough alone: only the server knows the dialect of the version actually connected, and
only the client can answer while the key is still down.

## Why almost everything is a warning

The server check runs on a pooled connection, not on the session a script runs on. A temporary
table, a `USE`, a `SET` from earlier in the script is invisible to it — so "table doesn't exist" may
well mean "not yet". Only error `1064`, the server refusing to parse the text at all, is reported as
an error. Everything else is a warning, and the privilege errors and `1295` ("this kind of statement
cannot be prepared") are reported as nothing at all.

The same logic runs through the client checker. It is certain about text that cannot be read — an
unclosed quote, an unbalanced bracket — and those are its only errors.

## Why it stays quiet more often than it could

Three rules, each of which throws away a real finding to avoid a false one:

- **A subquery, a `UNION`, a derived table or a `PARTITION` clause stops the schema checks for that
  statement.** Each gives names a scope this does not model, and an unknown name reported against
  the wrong scope is worse than one not reported at all.
- **An unqualified name is only reported when a real column is within two edits of it.** A bare word
  can be a function without brackets, a unit in an `INTERVAL`, an alias written without `AS`. Where
  there is a near-miss, a typo is the likely explanation and there is something useful to say; where
  there is not, there is neither. A qualified `alias.column` *is* checked unconditionally — what the
  alias stands for is known exactly.
- **The planned "`SELECT` with no `FROM` where one is clearly meant" rule was dropped.** Every
  version cheap enough to run per keystroke also fired on valid SQL, and the server check catches
  the real cases exactly, in MySQL's own words.

The principle behind all three: a checker that cries wolf on valid SQL gets switched off, and then
it catches nothing at all. Silence is the cheap failure here.

## Consequences

- **The two share one `linter()`.** CodeMirror applies one lint configuration to every linter in an
  editor, so two delays are not available. Instead the single source returns the client's findings
  immediately and remembers the server's against the exact text they were about — see
  [`SqlEditor/lint.ts`](../../../apps/desktop/src/modules/db/components/SqlEditor/lint.ts).

- **`forceLinting` is not how you show a late answer, and the mistake is silent.** It only brings
  forward a run that is *already scheduled*; once a run has finished there is no timer, and the call
  does nothing at all. Written that way, the server's reply arrived, was cached, and was never
  drawn — everything logged correctly and the screen stayed blank. Nothing caught it but opening the
  window and looking. The reply now goes on screen through `setDiagnostics` directly, and the one
  case that really is "re-run the checks" — the schema arriving after the editor — uses the
  `needsRefresh` config hook and a `recheck` state effect.
- **It does not re-check when the caret moves to another statement.** Re-running the whole script's
  checks on every arrow key is a cost paid on long scripts for very little. A statement is checked
  when it is edited, which is when there is something new to say about it.
- **The reserved-word list comes from `MySQL.spec`**, the same dialect the editor highlights with,
  so the two cannot drift. The dialect's `builtin` list is left out on purpose: those are the
  command-line client's words (`status`, `source`, `edit`), and treating them as reserved would
  quietly stop a column actually named `status` from ever being checked.
