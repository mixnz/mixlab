// MixLab's sync server: the routes, and nothing that knows what a record is.
//
// The contract is normative in the design document, not here — D2 to D4a of
// docs/specs/2026-09-20-t177-a-copy-only-you-can-read-design.md — and `server/conformance/` is
// written against that document rather than against this file. When the two disagree, this file is
// the one with the bug.
//
// **This file's only job is deciding which account object a request belongs to.** Everything that
// touches state happens inside that object, where execution is serialized. Two routes never reach
// one: `/v1/capabilities`, answered from configuration, and the verification page, which renders a
// form and validates nothing.

import { accountKey, sameSecret } from "./crypto";
import { readConfig, type Capabilities, type Env } from "./config";
import { fail, json, methodNotAllowed, misconfigured, notFound } from "./http";
import { askSource } from "./ratelimit";
import { asObject, isEmail } from "./validate";

export { Account } from "./account";
export { SourceLimit } from "./ratelimit";

function capabilities(reported: Capabilities): Response {
  // Answered from configuration and never routed to an object. Routed to one, it would spend a
  // duration window every time a client said hello — and duration, not requests, is what runs out
  // first on the free plan (D8).
  return json(200, reported, { "Cache-Control": "public, max-age=3600" });
}

/** The object that holds an address. This is the whole of what replaces an index (D8). */
async function objectForEmail(env: Env, email: string): Promise<DurableObjectStub> {
  return env.ACCOUNT.get(env.ACCOUNT.idFromName(await accountKey(email)));
}

/**
 * The object that issued a token. The name travels in the token's first half, so a bearer token
 * routes without a lookup; `idFromString` refuses anything this namespace did not mint, which is
 * what makes an attacker-supplied prefix a `401` rather than a new object.
 */
function objectForToken(env: Env, header: string | null): DurableObjectStub | null {
  if (!header?.startsWith("Bearer ")) return null;
  const [name] = header.slice("Bearer ".length).split(".");
  if (!name) return null;
  try {
    return env.ACCOUNT.get(env.ACCOUNT.idFromString(name));
  } catch {
    return null;
  }
}

/** Forward to the object with the body already read, since routing had to read it. */
function forward(stub: DurableObjectStub, request: Request, body: string | null): Promise<Response> {
  const headers = new Headers(request.headers);
  return stub.fetch(
    new Request(request.url, { method: request.method, headers, body: body ?? undefined }),
  );
}

const BY_EMAIL = new Set([
  "/v1/auth/register",
  "/v1/auth/verify",
  "/v1/auth/login",
  "/v1/auth/reset",
]);
const BY_TOKEN = new Set([
  "/v1/devices",
  "/v1/records",
  "/v1/records/batch",
  "/v1/auth/password",
  "/v1/account/freeze",
  "/v1/account/delete",
  "/v1/account/check",
]);

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const result = readConfig(env);
    if (!result.ok) return misconfigured(result.missing);
    const config = result.config;

    const url = new URL(request.url);
    const path = url.pathname;

    // **The whole host, or none of it.** A capabilities document that answered anybody would tell
    // somebody who found the address that the server is there, what it allows, and that it is
    // worth coming back to (D4a). A client is given the token before it makes its first request.
    if (config.accessToken) {
      const presented = request.headers.get("X-MixLab-Access") ?? "";
      if (!sameSecret(presented, config.accessToken)) {
        // The operator picked this string and may have picked a short one, so guessing costs.
        const verdict = await askSource(
          env.SOURCE_LIMIT,
          request,
          "access",
          config.limits.authPerHour,
        );
        if (!verdict.allowed) {
          const retryAfter = verdict.retryAfter ?? 3600;
          return fail(
            429,
            "too-many-requests",
            "Too many requests from this network.",
            { retryAfter },
            { "Retry-After": String(retryAfter) },
          );
        }
        return fail(
          401,
          "invalid-access-token",
          "This server is private. Ask its operator for the access token.",
        );
      }
    }

    if (path === "/v1/capabilities") {
      return request.method === "GET" ? capabilities(config.capabilities) : methodNotAllowed();
    }

    // Read once, here: routing needs the address out of the body, and a body can only be read
    // once. Everything downstream is handed the text.
    // A DELETE carries a body too: the time the deletion was made.
    const body = request.method === "GET" ? null : await request.text();

    // Unauthenticated, asked about any address, and on this server every question wakes that
    // address's object whether or not an account is there — so probing costs the deployment
    // something, and the source limiter is what bounds it (D4a).
    if (path === "/v1/auth/params") {
      if (request.method !== "GET") return methodNotAllowed();
      const email = url.searchParams.get("email");
      if (!isEmail(email)) {
        return fail(400, "invalid-email", "That is not a valid email address.");
      }
      const verdict = await askSource(env.SOURCE_LIMIT, request, path, config.limits.paramsPerHour);
      if (!verdict.allowed) {
        return fail(429, "too-many-requests", "Too many requests from this address.", {}, {
          "Retry-After": String(verdict.retryAfter ?? 3600),
        });
      }
      return forward(await objectForEmail(env, email), request, null);
    }

    // **The third limit, and the one that bounds a request rather than a record.**
    // `maxBatchOperations` times `maxRecordBytes` is a number no server intends to buffer, so
    // without this a client can compose a request every other limit calls legal (D4a).
    if (body !== null && body.length > config.capabilities.maxBatchBytes) {
      return fail(413, "request-too-large", "That request is larger than this server accepts.", {
        limit: config.capabilities.maxBatchBytes,
      });
    }

    if (path === "/__test__/outbox") {
      if (!config.testOutbox) return notFound();
      const email = url.searchParams.get("email");
      if (!isEmail(email)) return notFound();
      return forward(await objectForEmail(env, email), request, body);
    }

    if (BY_EMAIL.has(path)) {
      // These four take a body and nothing else. **There is no link to click** (D4a), so there is
      // no GET here to serve a page for.
      if (request.method !== "POST") return methodNotAllowed();
      const fields = asObject(body === null ? null : safeParse(body));
      const email = fields?.["email"];
      if (!isEmail(email)) {
        return fail(400, "invalid-email", "That is not a valid email address.");
      }

      // The two routes that create work out of nothing are counted per source rather than per
      // account: they are abused by opening many accounts, which a counter kept inside one
      // account's object cannot see. `ratelimit.ts` has the argument.
      const askingForReset = path === "/v1/auth/reset" && fields?.["token"] === undefined;
      // Signing in and spending a code are counted here as well as per account: a per-account
      // counter cannot see somebody working through a list of addresses, and on this server every
      // address named materialises an object whether or not an account is behind it.
      const perSource =
        path === "/v1/auth/register" ||
        askingForReset ||
        path === "/v1/auth/login" ||
        path === "/v1/auth/verify";
      if (perSource) {
        const allowance =
          path === "/v1/auth/register"
            ? config.limits.registrationsPerHour
            : askingForReset
              ? config.limits.resetsPerHour
              : config.limits.authPerHour;
        const verdict = await askSource(env.SOURCE_LIMIT, request, path, allowance);
        if (!verdict.allowed) {
          const retryAfter = verdict.retryAfter ?? 3600;
          return fail(
            429,
            "too-many-requests",
            "Too many requests from this network.",
            { retryAfter },
            { "Retry-After": String(retryAfter) },
          );
        }
      }

      return forward(await objectForEmail(env, email), request, body);
    }

    if (path === "/v1/auth/refresh") {
      const refreshToken = asObject(body === null ? null : safeParse(body))?.["refreshToken"];
      const stub = objectForToken(env, typeof refreshToken === "string" ? `Bearer ${refreshToken}` : null);
      if (!stub) return fail(401, "invalid-token", "That token is invalid or has expired.");
      return forward(stub, request, body);
    }

    if (BY_TOKEN.has(path) || path.startsWith("/v1/devices/") || path.startsWith("/v1/records/")) {
      const stub = objectForToken(env, request.headers.get("Authorization"));
      if (!stub) return fail(401, "invalid-token", "That token is invalid or has expired.");
      return forward(stub, request, body);
    }

    return notFound();
  },
} satisfies ExportedHandler<Env>;

function safeParse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}
