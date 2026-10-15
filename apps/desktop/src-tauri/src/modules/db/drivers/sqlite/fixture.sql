-- The database the SQLite tests read.
--
-- Committed as SQL rather than as a `.db` file on purpose: a binary fixture in the tree is a thing
-- nobody can review in a diff, and one that has to be rebuilt by hand whenever the schema it is
-- meant to demonstrate changes. This is built into a real file at the start of each test instead —
-- see `fixture()` — which costs milliseconds and keeps the schema readable.
--
-- Every row here is a case the driver has to get right, so nothing in it is arbitrary:
--
--   * `author.id` is an `INTEGER PRIMARY KEY` — the rowid alias, the one column SQLite fills in
--     itself, and the only shape that counts as one.
--   * `post.slug` is a stored generated column: it must appear in the grid and must never appear
--     in an INSERT.
--   * `post.body` is a blob, which has no JSON of its own and comes back base64.
--   * `post.author_id` is a foreign key, and its `to` is named.
--   * `tag.label` is part of a two-column primary key, so neither half is a rowid alias however it
--     is declared.
--   * `sqlite_sequence` appears by itself once an AUTOINCREMENT table exists, and is what the
--     table list has to leave out.
--   * `recent` is a view: listed like a table, with no primary key of its own.

-- `bio` carries a literal default, which SQLite reports back still quoted — see `split_default`.
--
-- No `--` comment inside the parentheses of any table here, deliberately. DROP COLUMN rewrites the
-- stored CREATE TABLE text, and a line comment left in it swallows whatever the rewrite puts after
-- it: the drop then fails with "incomplete input".
CREATE TABLE author (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  bio TEXT DEFAULT 'anonymous'
);

CREATE TABLE post (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  author_id INTEGER NOT NULL REFERENCES author (id),
  title TEXT NOT NULL,
  slug TEXT GENERATED ALWAYS AS (lower(title)) STORED,
  body BLOB,
  views INTEGER DEFAULT 0,
  created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- A two-column primary key. `id` is declared INTEGER and is first in the key, which is exactly the
-- shape that is *not* a rowid alias — the test that would otherwise pass by accident.
CREATE TABLE tag (
  id INTEGER,
  label TEXT,
  PRIMARY KEY (id, label)
);

-- No primary key at all, which is what makes two identical rows indistinguishable by their
-- columns. The write path has to reach one of them by rowid — see `delete_rows`.
CREATE TABLE loose (
  label TEXT
);

CREATE INDEX post_author ON post (author_id);
CREATE UNIQUE INDEX post_title ON post (title);

CREATE VIEW recent AS SELECT id, title FROM post ORDER BY created_at DESC;

INSERT INTO author (name, bio) VALUES ('Ada', 'The first'), ('Grace', NULL);
INSERT INTO post (author_id, title, body, views) VALUES
  (1, 'Hello world', x'00ff10', 7),
  (1, 'Second post', NULL, 3),
  (2, 'Something else', NULL, NULL);
INSERT INTO tag (id, label) VALUES (1, 'draft');
