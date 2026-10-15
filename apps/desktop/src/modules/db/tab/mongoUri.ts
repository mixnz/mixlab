// The Mongo form is a single connection string, so the two things the rest of the app used to
// read off separate fields — the host for a tab title, the database to open first — have to be
// picked back out of it. Both are cosmetic: an unparseable string yields nothing and the
// connection itself still reports the real error.
// Split by hand rather than with `new URL`: a comma-separated seed list —
// `mongodb://a:27017,b:27017/?replicaSet=rs0` — is a perfectly good Mongo string but not a valid
// URL, and that shape is exactly the one a replica set is written in.
const MONGO_URI_RE = /^mongodb(?:\+srv)?:\/\/(?:[^@/]*@)?([^/?]*)(?:\/([^?]*))?/i;

/** Host only — the string itself carries the password, which must never reach a tab title. */
export function mongoUriHost(uri: string): string {
  return MONGO_URI_RE.exec(uri.trim())?.[1] ?? "";
}

/** The default database, i.e. the path segment in `mongodb://host/thisOne?options`. */
export function mongoUriDatabase(uri: string): string {
  const path = MONGO_URI_RE.exec(uri.trim())?.[2] ?? "";
  try {
    return decodeURIComponent(path);
  } catch {
    return path;
  }
}
