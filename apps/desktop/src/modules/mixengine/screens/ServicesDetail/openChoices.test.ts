import { describe, expect, it } from "vitest";
import type { DesktopClient } from "@mixengine/api";
import { createsDatabases, openChoices, opensADatabase } from "./openChoices";

/** The window itself — the only client `database.client` names (T107, T165). */
const thisWindow: DesktopClient = { state: "installed", name: "MixLab", program: "/usr/bin/mixlab" };

const noClient: DesktopClient = { state: "no_client" };

describe("openChoices", () => {
  it("is one button while this window draws the built-in client", () => {
    expect(openChoices(thisWindow, true)).toEqual(["builtIn"]);
  });

  it("offers to turn the built-in client on when it is hidden, and nothing else", () => {
    expect(openChoices(thisWindow, false)).toEqual(["builtInAfterEnabling"]);
  });

  /* `no_client` is a sentence and not a button, whatever the profile says — T110 leaves what the
     affordance is drawn from alone. */
  it("offers nothing at all when the daemon names no client to open with", () => {
    for (const visible of [true, false]) {
      expect(openChoices(noClient, visible)).toEqual([]);
    }
  });
});

describe("opensADatabase", () => {
  /* `protocol` is the only answer to "is this service a database". Both the panel on the Services
     screen and the three-dot menu on the Dashboard ask it, so it has to be **one** function: two
     places deciding for themselves are two definitions of the same question, and they will drift
     apart. */
  it("says yes to a service a client speaks a protocol to", () => {
    expect(opensADatabase({ protocol: "postgres" })).toBe(true);
  });

  /* nginx, caddy, php-fpm: `database.client` returns `protocol: null` for them — a **state**, not
     an error. This is where that state becomes "draw nothing at all". */
  it("says no to a service no client opens", () => {
    expect(opensADatabase({ protocol: null })).toBe(false);
  });

  /* `protocol` is an optional member under ADR 0019: absence means a daemon older than this
     member, and guessing it is a database would draw a panel with nothing behind it. */
  it("says no when the daemon never answered the member", () => {
    expect(opensADatabase({})).toBe(false);
  });
});

describe("createsDatabases", () => {
  it("draws the form unless the daemon said the server makes no databases", () => {
    expect(createsDatabases({ creates_databases: false })).toBe(false);
    expect(createsDatabases({ creates_databases: true })).toBe(true);
    // An older daemon sends nothing: keep the form it always had.
    expect(createsDatabases({})).toBe(true);
  });
});
