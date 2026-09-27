// One Durable Object per account (D8).
//
// **Execution here is serialized**, and that is the whole reason this design is on Durable Objects
// rather than on a shared database. Three things that would otherwise need a carefully written
// transaction are free: the registration race settles because two attempts on one address arrive at
// one object; the per-account `seq` counter is monotonic because nothing else is running; and the
// per-record compare-and-swap is correct for the same reason. `server/native/` has to write those
// transactions by hand, and says so where it does.
//
// The object is addressed by the hash of the lowercased address, which is what this design has
// instead of an index: the same address is the same object every time, so there is nothing to keep
// in step. The cost is that changing an address changes the address of the object, so v1 does not
// offer it.
//
// **Nothing in here parses a ciphertext.**

import { readConfig, type Config, type Env } from "./config";
import {
  SALT_BYTES,
  VERIFIER_BYTES,
  WRAPPED_KEY_BYTES,
  accountKey,
  inventedSalt,
  normaliseCode,
  peppered,
  randomCode,
  randomPublicId,
  randomToken,
  sameSecret,
  sha256Hex,
} from "./crypto";
import { senderFor, type LetterKind } from "./email";
import { askBucket, type Allowance } from "./ratelimit";
import { fail, json, notFound } from "./http";
import {
  applyDelete,
  applyPut,
  listSince,
  staleSince,
  type Outcome,
  type Precondition,
} from "./records";
import { SCHEMA, type AccountRow, type DeviceRow, type TokenRow } from "./schema";
import {
  asObject,
  isBase64,
  isEmail,
  isNonEmptyString,
  isPositiveInteger,
} from "./validate";

const ACCESS_TOKEN_SECONDS = 15 * 60;
const REFRESH_TOKEN_SECONDS = 90 * 24 * 60 * 60;
const VERIFICATION_TOKEN_SECONDS = 24 * 60 * 60;
const RESET_TOKEN_SECONDS = 60 * 60;
const LOGIN_WINDOW_SECONDS = 15 * 60;
const SECONDS_A_DAY = 24 * 60 * 60;

const now = (): number => Math.floor(Date.now() / 1000);

/** A token is `<object name>.<secret>`; only the second half is stored, and only as its hash. */
const secretOf = (token: string): string => token.split(".").at(-1) ?? "";

/** Wrong, expired and already-used answer alike: the sentence a person needs is the same. */
const badCode = (): Response => fail(400, "invalid-code", "That code is invalid or has expired.");

/** `If-Match: "41"` and `If-None-Match: *`, which are how a write states what it believes. */
function readPrecondition(headers: Headers): Precondition {
  const ifMatch = headers.get("If-Match");
  const parsed = ifMatch === null ? Number.NaN : Number(ifMatch.replace(/"/g, ""));
  return {
    ifMatch: Number.isSafeInteger(parsed) ? parsed : undefined,
    ifNoneMatch: headers.get("If-None-Match") === "*",
  };
}

/**
 * A record answer, successful or not. On a 409 or a 412 the current record travels **beside** the
 * error rather than instead of it: the client is the one that resolves a conflict (D4), so it
 * needs the other side of it in the same answer it was refused by.
 */
function outcome(result: Outcome): Response {
  const body = {
    ...(result.record ?? {}),
    ...(result.error ? { error: result.error } : {}),
  };
  const headers =
    result.record && !result.error ? { ETag: `"${result.record.version}"` } : undefined;
  return json(result.status, body, headers);
}

export class Account implements DurableObject {
  private readonly sql: SqlStorage;

  constructor(
    private readonly state: DurableObjectState,
    private readonly env: Env,
  ) {
    this.sql = state.storage.sql;
    // `blockConcurrencyWhile` holds every request until this finishes, so no handler can ever meet
    // a half-built schema — the object's equivalent of a migration that runs before the first
    // connection is served.
    state.blockConcurrencyWhile(async () => {
      for (const statement of SCHEMA) this.sql.exec(statement);
      // An object made before `public_id` existed gets the column, and its account an id
      // (T178c, C4). `CREATE TABLE IF NOT EXISTS` leaves an existing table as it was.
      const columns = this.sql.exec<{ name: string }>(`PRAGMA table_info(account)`).toArray();
      if (!columns.some((column) => column.name === "public_id")) {
        this.sql.exec(`ALTER TABLE account ADD COLUMN public_id TEXT`);
      }
      this.sql.exec(`UPDATE account SET public_id = ? WHERE public_id IS NULL`, randomPublicId());
    });
  }

  async fetch(request: Request): Promise<Response> {
    const result = readConfig(this.env);
    if (!result.ok) return fail(503, "server-misconfigured", "Missing configuration.");
    const config = result.config;

    const url = new URL(request.url);
    const body = await this.readBody(request);
    if (body === undefined) return fail(400, "invalid-request", "The body is not JSON.");

    switch (`${request.method} ${url.pathname}`) {
      case "GET /v1/auth/params":
        return this.readParams(config, url);
      case "POST /v1/auth/register":
        return this.register(config, body);
      case "POST /v1/auth/verify":
        return this.verifyAddress(config, body);
      case "POST /v1/auth/login":
        return this.login(config, body);
      case "POST /v1/auth/refresh":
        return this.refresh(body);
      case "POST /v1/auth/password":
        return this.changePassword(config, request, body);
      case "POST /v1/auth/reset":
        return this.reset(config, body);
      case "GET /v1/devices":
        return this.listDevices(request);
      case "GET /v1/account/freeze":
        return this.readFreeze(request);
      case "POST /v1/account/freeze":
        return this.setFreeze(request, body);
      case "POST /v1/account/delete":
        return this.deleteAccount(config, request, body);
      case "POST /v1/account/check":
        return this.checkVerifier(config, request, body);
      case "GET /v1/records":
        return this.readRecords(config, request, url);
      case "POST /v1/records/batch":
        return this.batch(config, request, body);
      case "POST /v1/records/heads":
        return this.readHeads(config, request, body);
      case "GET /__test__/outbox":
        return this.readOutbox(config);
      default:
        break;
    }

    if (request.method === "DELETE" && url.pathname.startsWith("/v1/devices/")) {
      return this.deleteDevice(request, url.pathname.slice("/v1/devices/".length));
    }

    if (
      (request.method === "PUT" || request.method === "DELETE") &&
      url.pathname.startsWith("/v1/records/")
    ) {
      return this.writeRecord(config, request, url, body);
    }

    return notFound();
  }

  // --- holding still while a client copies the account (D4b) --------------------------------

  /**
   * **A freeze ends when a client ends it, and not before.** There is no expiry: posting `active`
   * is reachable from every signed-in machine, and signing in works while frozen, so a copy cut
   * off by a dead machine is one request away from over. A timeout would instead let a copy that
   * finished and was never followed up reopen this server on a clock, for a machine nobody
   * repointed to write into (D4b).
   */
  private freeze(): { state: string; frozenAt: number | null } {
    const account = this.account();
    if (!account) return { state: "active", frozenAt: null };
    return { state: account.freeze_state, frozenAt: account.freeze_at ?? null };
  }

  /**
   * What every mutating route owes an account that is being copied. Reads are not asked: the
   * machine doing the copying needs them, and so does a second machine deciding whether to take
   * the copy over.
   */
  private frozen(): Response | null {
    if (this.freeze().state !== "frozen") return null;
    return fail(423, "account-frozen", "This account is being moved to another server and can't be changed right now.");
  }

  private async readFreeze(request: Request): Promise<Response> {
    // Answered in both states: a machine that meets a refusal has to be able to find out why.
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    return json(200, this.freeze());
  }

  private async setFreeze(request: Request, body: unknown): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");

    const wanted = asObject(body)?.["state"];
    if (wanted !== "active" && wanted !== "frozen") {
      return fail(400, "invalid-request", "`state` must be active or frozen.");
    }

    if (wanted === "active") {
      // **Thawing is the only way out.** This is how the client that finished copying ends the
      // freeze, and how one that gave up abandons it.
      this.sql.exec(`UPDATE account SET freeze_state = 'active', freeze_at = NULL WHERE id = 1`);
      return json(200, this.freeze());
    }

    // Idempotent, and the clock does not restart: `freeze_at` is when this began, which is what a
    // client shows somebody being told their account has been read-only for a while.
    this.sql.exec(
      `UPDATE account SET freeze_state = 'frozen', freeze_at = COALESCE(freeze_at, ?) WHERE id = 1`,
      now(),
    );
    return json(200, this.freeze());
  }

  /**
   * **Deleting an account leaves this object as it was before the account existed** (D4b): no
   * tombstone, no row saying the address was once here, and the address free to register again.
   * Keeping any of it would be keeping the one fact D1 promises a server does not accumulate.
   */
  /**
   * `a` against this account's verifier, counted in the login window, so that no route that asks
   * for `A` is a cheaper way to guess it than signing in. The refusal to send, or `null`.
   */
  private async proveVerifier(config: Config, a: string): Promise<Response | null> {
    const retryAfter = this.tooMany("login", config.limits.loginsPerWindow, LOGIN_WINDOW_SECONDS);
    if (retryAfter !== null) {
      return fail(
        429,
        "too-many-attempts",
        "Too many attempts on this account.",
        { retryAfter },
        { "Retry-After": String(retryAfter) },
      );
    }
    const account = this.account();
    const presented = await peppered(config.pepper, a);
    if (!account || !sameSecret(account.verifier, presented)) {
      return fail(401, "invalid-credentials", "That password does not match this account.");
    }
    return null;
  }

  /** Whether `A` is this account's (T178c, C3): a move asks before it registers anywhere with it. */
  private async checkVerifier(config: Config, request: Request, body: unknown): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    const fields = asObject(body);
    if (!fields || !isBase64(fields["a"], VERIFIER_BYTES)) {
      return fail(400, "invalid-request", "The current verifier is required.");
    }
    return (await this.proveVerifier(config, fields["a"] as string)) ?? new Response(null, { status: 204 });
  }

  private async deleteAccount(config: Config, request: Request, body: unknown): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");

    const fields = asObject(body);
    if (!fields || !isBase64(fields["a"], VERIFIER_BYTES)) {
      return fail(400, "invalid-request", "The current verifier is required.");
    }

    // **A session is not enough.** A borrowed unlocked machine already holds one, so this asks for
    // the verifier: the person deleting the account is then the person who knows the password.
    const refused = await this.proveVerifier(config, fields["a"] as string);
    if (refused) return refused;

    const counted = this.sql
      .exec<{ total: number }>(`SELECT COUNT(*) AS total FROM record`)
      .toArray()[0];

    // **A frozen account may be deleted**, because the last step of a move is deleting the source
    // and the source is frozen at that point. Refusing here would mean thawing first, which is a
    // window for a second machine to write something the copy never saw.
    this.wipe();
    await this.state.storage.deleteAlarm();
    return json(200, { recordsDeleted: Number(counted?.total ?? 0) });
  }

  // --- records ------------------------------------------------------------------------------

  private async readRecords(config: Config, request: Request, url: URL): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    // `Number("")` is 0, which would have quietly turned `?since=` into "from the beginning"
    // here while `server/native/` refused it. Digits or nothing.
    const raw = url.searchParams.get("since");
    const since = raw === null ? 0 : /^\d+$/.test(raw) ? Number(raw) : Number.NaN;
    // A flag that turns expiry off takes one spelling, not `true`, `0` or nothing (T178b, M4).
    const resync = url.searchParams.get("resync");
    if (resync !== null && resync !== "1") {
      return fail(400, "invalid-request", "`resync` is 1 or absent.");
    }

    const result = listSince(
      this.sql,
      config.capabilities,
      since,
      url.searchParams.get("collection"),
      resync === "1",
    );
    return "records" in result ? json(200, result) : outcome(result);
  }

  private async readHeads(config: Config, request: Request, body: unknown): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    // A read, like `GET /v1/records`: a frozen account still answers it (D4b).
    const result = staleSince(this.sql, config.capabilities, body, session.deviceId);
    return "stale" in result ? json(200, result) : outcome(result);
  }

  private async writeRecord(
    config: Config,
    request: Request,
    url: URL,
    body: unknown,
  ): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    const frozen = this.frozen();
    if (frozen) return frozen;

    const parts = url.pathname.slice("/v1/records/".length).split("/");
    if (parts.length !== 2) return notFound();
    const [collection, id] = parts as [string, string];
    const precondition = readPrecondition(request.headers);

    if (request.method === "PUT") {
      return outcome(
        applyPut(this.sql, config.capabilities, collection, id, body, precondition, session.deviceId),
      );
    }

    const removed = applyDelete(
      this.sql,
      collection,
      id,
      precondition.ifMatch,
      asObject(body)?.["updatedAt"],
      session.deviceId,
    );
    if (removed.status === 200) await this.scheduleReaping(config.capabilities.tombstoneRetentionDays);
    return outcome(removed);
  }

  private async batch(config: Config, request: Request, body: unknown): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    const frozen = this.frozen();
    if (frozen) return frozen;

    const operations = asObject(body)?.["operations"];
    const limits = config.capabilities;
    if (
      !Array.isArray(operations) ||
      operations.length === 0 ||
      operations.length > limits.maxBatchOperations
    ) {
      return fail(
        400,
        "invalid-request",
        `A batch needs between 1 and ${limits.maxBatchOperations} operations.`,
      );
    }

    // The envelope refuses only when the account is already full: past that point every entry
    // would fail alike, and answering once is kinder than answering a hundred times.
    const account = this.account();
    if (account && account.stored_bytes > limits.accountQuotaBytes) {
      return fail(507, "quota-exceeded", "This account is full.", {
        limit: limits.accountQuotaBytes,
        used: account.stored_bytes,
      });
    }

    // Independent compare-and-swaps and not a transaction (D4): each entry succeeds or conflicts
    // on its own, and a 409 in entry seven is news for the client rather than a failed request.
    const results = operations.map((operation) => {
      const fields = asObject(operation);
      if (!fields) return { status: 400, error: { code: "invalid-request", message: "Not an operation." } };

      const collection = String(fields["collection"] ?? "");
      const id = String(fields["id"] ?? "");
      const ifMatch = typeof fields["ifMatch"] === "number" ? fields["ifMatch"] : undefined;

      if (fields["op"] === "delete") {
        return applyDelete(this.sql, collection, id, ifMatch, fields["updatedAt"], session.deviceId);
      }
      if (fields["op"] === "put") {
        return applyPut(
          this.sql,
          limits,
          collection,
          id,
          fields["record"],
          { ifMatch, ifNoneMatch: fields["ifNoneMatch"] === true },
          session.deviceId,
        );
      }
      return { status: 400, error: { code: "invalid-request", message: "`op` is put or delete." } };
    });

    if (results.some((entry) => entry.record?.deleted === true)) {
      await this.scheduleReaping(limits.tombstoneRetentionDays);
    }

    return json(200, { results });
  }

  /**
   * What a client needs before it can compute `A` at all. **No authentication**, and an address
   * with no account gets an answer anyway — one nobody can tell from a real one (D4a).
   */
  private async readParams(config: Config, url: URL): Promise<Response> {
    const email = url.searchParams.get("email") ?? "";
    const account = this.account();
    return json(200, {
      saltAccount: account
        ? account.salt_account
        : await inventedSalt(config.pepper, await accountKey(email)),
      argon: account
        ? { m: account.argon_m, t: account.argon_t, p: account.argon_p }
        : { m: 65536, t: 3, p: 4 },
    });
  }

  // --- the account itself ------------------------------------------------------------------

  private async register(config: Config, body: unknown): Promise<Response> {
    const fields = asObject(body);
    if (!fields || !isEmail(fields["email"])) {
      return fail(400, "invalid-email", "That is not a valid email address.");
    }
    if (
      !isBase64(fields["a"], VERIFIER_BYTES) ||
      !isBase64(fields["a"], VERIFIER_BYTES) ||
      !isBase64(fields["saltAccount"], SALT_BYTES) ||
      !isBase64(fields["wrappedMkPassword"], WRAPPED_KEY_BYTES) ||
      !isBase64(fields["wrappedMkRecovery"], WRAPPED_KEY_BYTES)
    ) {
      return fail(400, "invalid-request", "The request needs a verifier, a salt and two wrapped keys.");
    }

    const argon = asObject(fields["argon"]);
    if (
      !argon ||
      !isPositiveInteger(argon["m"]) ||
      !isPositiveInteger(argon["t"]) ||
      !isPositiveInteger(argon["p"])
    ) {
      return fail(400, "invalid-request", "The request needs the Argon2 parameters the client used.");
    }

    // The race settles here and nowhere else: two attempts on one address reach one object, and
    // the second finds this row (D8).
    const existing = this.account();
    if (existing?.verified === 1) {
      return fail(409, "email-taken", "That address already has an account.");
    }

    // **Counted only once a letter is certain to be sent.** Counting before the check above
    // would let anybody spend a verified address's allowance and stop its owner from asking
    // for a reset — a refusal that costs the wrong person.
    const key = await accountKey(fields["email"] as string);
    const letters = await this.letterAllowance(config, key);
    if (!letters.allowed) {
      return fail(
        429,
        "too-many-attempts",
        "Too many letters have been sent to this address.",
        { retryAfter: letters.retryAfter },
        { "Retry-After": String(letters.retryAfter) },
      );
    }
    // **An unverified account is replaced rather than defended.** A verification token lives a day;
    // once it expires the person cannot verify, cannot register again, and cannot reset — a reset
    // is only offered to an address that proved itself. Replacing loses nothing, because D4 forbids
    // writing any record before verification, so there is never anything there to lose.
    if (existing) this.wipe();

    this.sql.exec(
      `INSERT INTO account (id, account_key, email, verifier, salt_account, argon_m, argon_t,
                            argon_p, wrapped_mk_password, wrapped_mk_recovery, verified, created_at,
                            public_id)
       VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)`,
      key,
      (fields["email"] as string).trim().toLowerCase(),
      await peppered(config.pepper, fields["a"] as string),
      fields["saltAccount"],
      argon["m"],
      argon["t"],
      argon["p"],
      fields["wrappedMkPassword"],
      fields["wrappedMkRecovery"],
      now(),
      randomPublicId(),
    );

    // **Registration is not complete until the letter is accepted.** Keeping an account whose
    // letter never went out would hand somebody an address they can never use and never free.
    if (!(await this.sendLetter(config, "verification"))) {
      this.wipe();
      return fail(
        502,
        "letter-not-sent",
        "This server could not send the confirmation letter, so no account was made.",
      );
    }
    return json(201, {});
  }

  private async verifyAddress(config: Config, body: unknown): Promise<Response> {
    const account = this.account();
    // **Eight characters are only safe because guessing is bounded** (D4a). Counted before the code
    // is looked at, so a wrong one costs an attempt whatever was wrong about it — but only when
    // there is an account to count against, or naming a stranger's address writes a row.
    if (account) {
      const retryAfter = this.tooMany(
        "verify",
        config.limits.verifyAttemptsPerWindow,
        LOGIN_WINDOW_SECONDS,
      );
      if (retryAfter !== null) {
        return fail(
          429,
          "too-many-attempts",
          "Too many codes tried on this account.",
          { retryAfter },
          { "Retry-After": String(retryAfter) },
        );
      }
    }

    const presented = asObject(body)?.["token"];
    const code = typeof presented === "string" ? normaliseCode(presented) : null;
    // Already verified, a spent code and a wrong one look alike on purpose: the sentence a
    // person needs is the same in all three cases (D4a).
    if (!code || !account || account.verified === 1) return badCode();
    if (!(await this.spendMailToken(code, "verification"))) return badCode();

    this.sql.exec(`UPDATE account SET verified = 1 WHERE id = 1`);
    return json(200, {});
  }

  // --- losing the password -----------------------------------------------------------------

  /** D6, case 1: re-wrap, one request, nothing else moves. */
  private async changePassword(config: Config, request: Request, body: unknown): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");
    const frozen = this.frozen();
    if (frozen) return frozen;

    const fields = asObject(body);
    if (
      !fields ||
      !isBase64(fields["a"], VERIFIER_BYTES) ||
      !isBase64(fields["newA"], VERIFIER_BYTES) ||
      !isBase64(fields["newSaltAccount"], SALT_BYTES) ||
      !isBase64(fields["newWrappedMkPassword"], WRAPPED_KEY_BYTES)
    ) {
      return fail(400, "invalid-request", "The request needs the current verifier and a new one with its salt.");
    }

    const account = this.account();
    const presented = await peppered(config.pepper, fields["a"] as string);
    if (!account || !sameSecret(account.verifier, presented)) {
      return fail(401, "invalid-credentials", "That password does not match this account.");
    }

    // MK is unchanged, so nothing is re-encrypted and **no record and no sequence number moves**.
    // A server that bumped `seq` here would make every other machine re-download the account.
    this.sql.exec(
      `UPDATE account SET verifier = ?, salt_account = ?, wrapped_mk_password = ? WHERE id = 1`,
      await peppered(config.pepper, fields["newA"] as string),
      fields["newSaltAccount"],
      fields["newWrappedMkPassword"],
    );
    // Every other machine is signed out: the password they hold no longer unwraps anything here.
    this.sql.exec(`DELETE FROM token WHERE device_id != ?`, session.deviceId);

    return json(200, {});
  }

  /**
   * One path, two shapes (D4a). Without a token it asks for the letter and always answers `202`,
   * whether or not the address has an account — registration has to refuse a taken address and
   * therefore leaks one; this route has no such obligation, so it does not.
   *
   * With a token it completes, and **abandons the data** (D6, case 3): every record is encrypted
   * under an MK no surviving key unwraps, so leaving them would leave an account full of bytes
   * that decrypt for nobody.
   */
  private async reset(config: Config, body: unknown): Promise<Response> {
    const fields = asObject(body);
    if (!fields) return fail(400, "invalid-request", "An address is required.");

    // **D6 case 2, second request.** The client already spent the code, got `MK` back under the
    // recovery key, and is setting a new password. The records stay, because the client says it
    // re-wrapped the same `MK` — which this server cannot check and does not try.
    if (fields["ticket"] !== undefined) {
      if (
        !isBase64(fields["a"], VERIFIER_BYTES) ||
        !isBase64(fields["saltAccount"], SALT_BYTES) ||
        !isBase64(fields["wrappedMkPassword"], WRAPPED_KEY_BYTES) ||
        !isBase64(fields["wrappedMkRecovery"], WRAPPED_KEY_BYTES)
      ) {
        return fail(400, "invalid-request", "The request needs a verifier, a salt and two wrapped keys.");
      }
      const ticket = fields["ticket"];
      if (
        typeof ticket !== "string" ||
        !this.account() ||
        !this.spendToken(await sha256Hex(ticket), "reset-ticket")
      ) {
        return fail(401, "invalid-token", "That ticket is invalid or has expired.");
      }

      this.sql.exec(
        `UPDATE account SET verifier = ?, salt_account = ?, wrapped_mk_password = ?,
                            wrapped_mk_recovery = ? WHERE id = 1`,
        await peppered(config.pepper, fields["a"] as string),
        fields["saltAccount"],
        fields["wrappedMkPassword"],
        fields["wrappedMkRecovery"],
      );
      // Every machine is signed out, as in case 3. **`next_seq`, `stored_bytes` and the record
      // table are untouched**: a machine still holding a cursor must not be told that nothing has
      // changed.
      this.sql.exec(`DELETE FROM token`);
      return json(200, { recordsDeleted: 0 });
    }

    if (fields["token"] === undefined) {
      const account = this.account();
      // A reset letter that could not be sent still answers 202: saying otherwise would tell a
      // stranger which addresses have an account, which is the whole point of this answer.
      //
      // **Including when it is this server that refused to send it.** An address over its
      // allowance cannot answer 429 here for the same reason, so it answers 202 and no letter
      // goes out (D4a).
      if (account && account.verified === 1) {
        const letters = await this.letterAllowance(config, account.account_key);
        if (letters.allowed) await this.sendLetter(config, "reset");
      }
      return json(202, {});
    }

    const code = typeof fields["token"] === "string" ? normaliseCode(fields["token"]) : null;

    // **D6 case 2, first request.** A code and nothing else asks for the wrapped key. Spending the
    // code here is what earns it, so one code cannot mint two tickets.
    if (fields["a"] === undefined) {
      const account = this.account();
      if (code === null || !account || !(await this.spendMailToken(code, "reset"))) {
        return badCode();
      }
      const ticket = randomToken();
      this.sql.exec(
        `INSERT INTO mail_token (hash, kind, expires_at, used) VALUES (?, ?, ?, 0)`,
        await sha256Hex(ticket),
        "reset-ticket",
        now() + config.limits.resetTicketSeconds,
      );
      return json(200, {
        wrappedMkRecovery: account.wrapped_mk_recovery,
        ticket,
        expiresIn: config.limits.resetTicketSeconds,
      });
    }
    if (
      code === null ||
      !isBase64(fields["a"], VERIFIER_BYTES) ||
      !isBase64(fields["saltAccount"], SALT_BYTES) ||
      !isBase64(fields["wrappedMkPassword"], WRAPPED_KEY_BYTES) ||
      !isBase64(fields["wrappedMkRecovery"], WRAPPED_KEY_BYTES)
    ) {
      return badCode();
    }
    if (!this.account() || !(await this.spendMailToken(code, "reset"))) {
      return badCode();
    }

    const recordsDeleted = this.sql
      .exec<{ n: number }>(`SELECT COUNT(*) AS n FROM record WHERE deleted = 0`)
      .toArray()[0]!.n;

    // Outright, not tombstoned: a tombstone exists to tell a machine that something it can read is
    // gone, and after this no machine can read anything. `next_seq` is untouched — it is monotonic
    // for the life of the account, and restarting it would hand a machine still holding an old
    // cursor an answer that looks like "nothing has changed".
    this.sql.exec(`DELETE FROM record`);
    this.sql.exec(
      `UPDATE account SET verifier = ?, salt_account = ?, wrapped_mk_password = ?,
                          wrapped_mk_recovery = ?, stored_bytes = 0 WHERE id = 1`,
      await peppered(config.pepper, fields["a"] as string),
      fields["saltAccount"],
      fields["wrappedMkPassword"],
      fields["wrappedMkRecovery"],
    );
    this.sql.exec(`DELETE FROM token`);

    return json(200, { recordsDeleted });
  }

  // --- signing in --------------------------------------------------------------------------

  private async login(config: Config, body: unknown): Promise<Response> {
    const fields = asObject(body);
    if (!fields || !isBase64(fields["a"], VERIFIER_BYTES)) {
      return fail(400, "invalid-request", "A verifier is required.");
    }
    if (!isNonEmptyString(fields["deviceName"], 128)) {
      return fail(400, "invalid-device-name", "The device name must be 1 to 128 characters.");
    }

    // **The account lookup comes first.** Counting an attempt against an address that has no
    // account writes a row — and therefore materialises a Durable Object — for any address a
    // stranger cares to name. `server/native/` looks first, and this now agrees with it.
    const account = this.account();
    if (account) {
      const retryAfter = this.tooMany("login", config.limits.loginsPerWindow, LOGIN_WINDOW_SECONDS);
      if (retryAfter !== null) {
        return fail(
          429,
          "too-many-attempts",
          "Too many attempts on this account.",
          { retryAfter },
          { "Retry-After": String(retryAfter) },
        );
      }
    }

    const presented = await peppered(config.pepper, fields["a"] as string);
    // An unknown address and a wrong verifier answer alike. Registration has to refuse a taken
    // address and therefore leaks one; this route has no such obligation, so it does not (D4a).
    if (!account || !sameSecret(account.verifier, presented)) {
      return fail(401, "invalid-credentials", "That address and password do not match an account.");
    }
    if (account.verified !== 1) {
      return fail(403, "email-not-verified", "Confirm the address before signing in.");
    }
    const deviceId = randomToken();
    const at = now();
    this.sql.exec(
      `INSERT INTO device (id, name, created_at, last_seen_at) VALUES (?, ?, ?, ?)`,
      deviceId,
      (fields["deviceName"] as string).trim(),
      at,
      at,
    );

    // **Both wrapped copies travel here**, after the verifier matched and nowhere else: a
    // fresh install that could not get them would have an account it cannot read, and one
    // handed out before the password was proved is an offline attack waiting to happen.
    return json(200, {
      ...(await this.issue(deviceId)),
      deviceId,
      wrappedMkPassword: account.wrapped_mk_password,
      wrappedMkRecovery: account.wrapped_mk_recovery,
      accountId: account.public_id,
    });
  }

  private async refresh(body: unknown): Promise<Response> {
    const fields = asObject(body);
    const presented = fields?.["refreshToken"];
    if (typeof presented !== "string") {
      return fail(401, "invalid-token", "That token is invalid or has expired.");
    }

    const row = this.token(await sha256Hex(secretOf(presented)));
    if (!row || row.kind !== "refresh" || row.expires_at <= now()) {
      return fail(401, "invalid-token", "That token is invalid or has expired.");
    }

    if (row.rotated === 1) {
      // Either it was copied, or two clients raced. Both want the person to sign in again rather
      // than continue quietly, so the device's whole chain goes (D4a).
      this.sql.exec(`DELETE FROM token WHERE device_id = ?`, row.device_id);
      return fail(401, "invalid-token", "That token is invalid or has expired.");
    }

    this.sql.exec(`UPDATE token SET rotated = 1 WHERE hash = ?`, row.hash);
    this.sql.exec(`DELETE FROM token WHERE device_id = ? AND kind = 'access'`, row.device_id);
    this.touch(row.device_id);
    return json(200, await this.issue(row.device_id));
  }

  // --- devices -----------------------------------------------------------------------------

  private async listDevices(request: Request): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");

    const devices = this.sql
      .exec<DeviceRow>(`SELECT * FROM device ORDER BY created_at ASC`)
      .toArray()
      .map((device) => ({
        id: device.id,
        name: device.name,
        createdAt: device.created_at,
        lastSeenAt: device.last_seen_at,
        current: device.id === session.deviceId,
      }));

    return json(200, { devices });
  }

  private async deleteDevice(request: Request, id: string): Promise<Response> {
    const session = await this.authenticate(request);
    if (!session) return fail(401, "invalid-token", "That token is invalid or has expired.");

    const exists = this.sql.exec(`SELECT id FROM device WHERE id = ?`, id).toArray().length === 1;
    // 404 and not 403: a 403 would confirm that the device exists on somebody else's account.
    if (!exists) return fail(404, "unknown-device", "No such device on this account.");

    this.sql.exec(`DELETE FROM token WHERE device_id = ?`, id);
    this.sql.exec(`DELETE FROM device WHERE id = ?`, id);
    return new Response(null, { status: 204 });
  }

  // --- the test outbox ---------------------------------------------------------------------

  private readOutbox(config: Config): Response {
    if (!config.testOutbox) return notFound();
    const messages = this.sql
      .exec<{ kind: string; token: string; sent_at: number }>(
        `SELECT kind, token, sent_at FROM outbox ORDER BY id ASC`,
      )
      .toArray()
      .map((row) => ({ kind: row.kind, token: row.token, sentAt: row.sent_at }));
    return json(200, { messages });
  }

  // --- the small things everything else stands on ------------------------------------------

  /**
   * How many more letters this address may receive this hour (D4a). Kept in the source-limit
   * namespace rather than in this object, because this object is replaced when somebody
   * registers over an unverified account — which is one of the two ways to ask for a letter.
   */
  private letterAllowance(config: Config, key: string): Promise<Allowance> {
    return askBucket(
      this.env.SOURCE_LIMIT,
      `letter:${key}`,
      "letter",
      config.limits.lettersPerAccountPerHour,
    );
  }

  /** Everything this object holds about one account, for when the account stops existing. */
  private wipe(): void {
    for (const table of ["record", "token", "mail_token", "outbox", "device", "attempt", "account"]) {
      this.sql.exec(`DELETE FROM ${table}`);
    }
  }

  private account(): AccountRow | null {
    return this.sql.exec<AccountRow>(`SELECT * FROM account WHERE id = 1`).toArray()[0] ?? null;
  }

  private token(hash: string): TokenRow | null {
    return this.sql.exec<TokenRow>(`SELECT * FROM token WHERE hash = ?`, hash).toArray()[0] ?? null;
  }

  private touch(deviceId: string): void {
    this.sql.exec(`UPDATE device SET last_seen_at = ? WHERE id = ?`, now(), deviceId);
  }

  private async readBody(request: Request): Promise<unknown> {
    // A record's DELETE carries its time (T178c, C1); a device's has no body and reads as null.
    if (request.method === "GET") return null;
    const text = await request.text();
    if (text.length === 0) return null;
    try {
      return JSON.parse(text);
    } catch {
      return undefined;
    }
  }

  /**
   * A token carries the name of the object that issued it, so that a request bearing one can be
   * routed without an index — the same problem the address hash solves for the routes that name an
   * account, and the same answer. The prefix is the object's id and not the address hash: the id
   * is not reversible to an address, so a token that leaks into a log does not leak a guessable
   * hash of somebody's email. **This format is not protocol** — a token is opaque to a client, and
   * `server/native/` is free to shape its own differently.
   */
  private async issue(deviceId: string): Promise<{ accessToken: string; refreshToken: string; expiresIn: number }> {
    const at = now();
    const name = this.state.id.toString();
    const accessSecret = randomToken();
    const refreshSecret = randomToken();
    const accessToken = `${name}.${accessSecret}`;
    const refreshToken = `${name}.${refreshSecret}`;

    this.sql.exec(
      `INSERT INTO token (hash, kind, device_id, expires_at) VALUES (?, 'access', ?, ?)`,
      await sha256Hex(accessSecret),
      deviceId,
      at + ACCESS_TOKEN_SECONDS,
    );
    this.sql.exec(
      `INSERT INTO token (hash, kind, device_id, expires_at) VALUES (?, 'refresh', ?, ?)`,
      await sha256Hex(refreshSecret),
      deviceId,
      at + REFRESH_TOKEN_SECONDS,
    );

    return { accessToken, refreshToken, expiresIn: ACCESS_TOKEN_SECONDS };
  }

  /**
   * The access token in `Authorization`, checked against the table. It is the same lookup that
   * would notice a revocation, which is why deleting a device ends both its tokens at once and
   * costs nothing to do (D4a).
   */
  async authenticate(request: Request): Promise<{ deviceId: string } | null> {
    const header = request.headers.get("Authorization") ?? "";
    if (!header.startsWith("Bearer ")) return null;
    const row = this.token(await sha256Hex(secretOf(header.slice("Bearer ".length))));
    if (!row || row.kind !== "access" || row.expires_at <= now()) return null;

    this.touch(row.device_id);
    return { deviceId: row.device_id };
  }

  private async spendMailToken(code: string, kind: LetterKind): Promise<boolean> {
    return this.spendToken(await sha256Hex(code), kind);
  }

  /**
   * The same, for a token that is not a letter. **A reset ticket has no message** and must never
   * reach the letter composer, so it is a kind rather than a `LetterKind` (D6).
   */
  private spendToken(hash: string, kind: string): boolean {
    const row = this.sql
      .exec<{ hash: string; kind: string; expires_at: number; used: number }>(
        `SELECT * FROM mail_token WHERE hash = ?`,
        hash,
      )
      .toArray()[0];
    if (!row || row.kind !== kind || row.used === 1 || row.expires_at <= now()) return false;
    this.sql.exec(`UPDATE mail_token SET used = 1 WHERE hash = ?`, hash);
    return true;
  }

  /**
   * A fixed window for one account. `true` means refuse. The allowance is configuration and is not
   * reported by `/v1/capabilities`: publishing the number that stops abuse helps only the abuser.
   */
  private tooMany(action: string, allowance: number, windowSeconds: number): number | null {
    const at = now();
    const row = this.sql
      .exec<{ count: number; started_at: number }>(
        `SELECT count, started_at FROM attempt WHERE action = ?`,
        action,
      )
      .toArray()[0];

    if (!row || at - row.started_at >= windowSeconds) {
      this.sql.exec(
        `INSERT INTO attempt (action, count, started_at) VALUES (?, 1, ?)
         ON CONFLICT (action) DO UPDATE SET count = 1, started_at = excluded.started_at`,
        action,
        at,
      );
      return null;
    }
    if (row.count >= allowance) return row.started_at + windowSeconds - at;
    this.sql.exec(`UPDATE attempt SET count = count + 1 WHERE action = ?`, action);
    return null;
  }

  // --- reaping -------------------------------------------------------------------------------

  /**
   * **An alarm is scheduled only when there is a tombstone to reap** (D8, rule 4), and this is a
   * rule about money rather than tidiness: an alarm invocation is a request, so a daily alarm per
   * account bills for every account that has ever existed rather than for every account in use,
   * and it does it quietly, forever. An account with nothing deleted sets no alarm at all.
   */
  private async scheduleReaping(retentionDays: number): Promise<void> {
    const oldest = this.sql
      .exec<{ at: number | null }>(`SELECT MIN(written_at) AS at FROM record WHERE deleted = 1`)
      .toArray()[0];
    if (!oldest || oldest.at === null) return;

    const due = Math.max((oldest.at + retentionDays * SECONDS_A_DAY) * 1000, Date.now() + 1000);
    const current = await this.state.storage.getAlarm();
    if (current === null || current > due) await this.state.storage.setAlarm(due);
  }

  async alarm(): Promise<void> {
    const result = readConfig(this.env);
    if (!result.ok) return;

    const days = result.config.capabilities.tombstoneRetentionDays;
    const horizon = now() - days * SECONDS_A_DAY;
    const highest = this.sql
      .exec<{ s: number | null }>(
        `SELECT MAX(seq) AS s FROM record WHERE deleted = 1 AND written_at <= ?`,
        horizon,
      )
      .toArray()[0];

    if (highest && highest.s !== null) {
      this.sql.exec(`DELETE FROM record WHERE deleted = 1 AND written_at <= ?`, horizon);
      // Every cursor below the highest seq that has just gone is now incomplete news, and D3 says
      // such a machine is told to resync from empty rather than told it quietly.
      this.sql.exec(
        `UPDATE account SET reaped_below_seq = MAX(reaped_below_seq, ?) WHERE id = 1`,
        highest.s,
      );
    }

    await this.scheduleReaping(days);
  }

  /** `false` means the provider refused it, and the caller decides what that costs. */
  private async sendLetter(config: Config, kind: LetterKind): Promise<boolean> {
    const account = this.account();
    if (!account) return false;

    const code = randomCode();
    const lifetime = kind === "verification" ? VERIFICATION_TOKEN_SECONDS : RESET_TOKEN_SECONDS;
    this.sql.exec(
      `INSERT INTO mail_token (hash, kind, expires_at) VALUES (?, ?, ?)`,
      await sha256Hex(code.replace("-", "")),
      kind,
      now() + lifetime,
    );

    // **The outbox is test-mode only.** An earlier version wrote here whenever there was no
    // sender, which conflated "this is the suite" with "this deployment is misconfigured": a
    // production server missing an endpoint recorded the letter in a table nobody can read,
    // answered 201, and left a person waiting for a message that was never sent. That is the exact
    // failure D8's refuse-to-start rule exists to prevent, arriving through the back door.
    if (config.testOutbox) {
      this.sql.exec(
        `INSERT INTO outbox (kind, token, sent_at) VALUES (?, ?, ?)`,
        kind,
        code,
        now(),
      );
      return true;
    }

    const sender = senderFor(config);
    if (!sender) {
      console.error(`cannot send the ${kind} letter: this server has no email provider configured`);
      return false;
    }

    try {
      await sender.send({ kind, to: account.email, code });
      return true;
    } catch (reason) {
      console.error(`could not send the ${kind} letter:`, reason);
      return false;
    }
  }
}
