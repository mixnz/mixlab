import { describe, expect, it } from "vitest";

import type { ExtensionOffer, ExtensionSummary, SiteSummary } from "@mixengine/api";

import {
  installable,
  kindKey,
  notInstalled,
  publishedTargets,
  rowActions,
  summaryDescription,
  webAppSite,
} from "./extensionState";

function summary(over: Partial<ExtensionSummary> = {}): ExtensionSummary {
  return {
    id: "adminer",
    name: "Adminer",
    version: "6.0.1",
    kind: "web-app",
    signed: true,
    service: null,
    ports: [],
    site: "adminer.mixengine.test",
    ...over,
  } as ExtensionSummary;
}

function offer(id: string, installed: boolean): ExtensionOffer {
  return {
    id,
    name: id,
    version: "1.0.0",
    kind: "service",
    description: "",
    installed,
    artifact: { type: "not_required" },
  } as ExtensionOffer;
}

function site(state: "enabled" | "disabled"): SiteSummary {
  return {
    domain: "adminer.mixengine.test",
    owner: { type: "extension", id: "adminer" },
    https: true,
    state,
  } as unknown as SiteSummary;
}

describe("notInstalled", () => {
  it("leaves an installed add-on to the card above", () => {
    expect(notInstalled([offer("a", true), offer("b", false)]).map((o) => o.id)).toEqual(["b"]);
  });
});

describe("installable", () => {
  it("installs what is published here or needs nothing", () => {
    expect(installable({ type: "published", url: "u", sha256: "s" })).toBe(true);
    expect(installable({ type: "not_required" })).toBe(true);
  });

  it("refuses an add-on published only for other systems, and says for which", () => {
    const elsewhere = { type: "other_targets", targets: ["linux-x86_64"] } as const;
    expect(installable({ ...elsewhere, targets: [...elsewhere.targets] })).toBe(false);
    expect(publishedTargets({ ...elsewhere, targets: [...elsewhere.targets] })).toEqual([
      "linux-x86_64",
    ]);
    expect(publishedTargets({ type: "not_required" })).toEqual([]);
  });
});

describe("kindKey", () => {
  it("names the three kinds", () => {
    expect(kindKey("web-app")).toBe("mixengine.extensions.kind.web-app");
    expect(kindKey("service")).toBe("mixengine.extensions.kind.service");
    expect(kindKey("recipe")).toBe("mixengine.extensions.kind.recipe");
  });

  it("leaves a kind this build does not know to be shown as the daemon wrote it", () => {
    expect(kindKey("plugin")).toBeNull();
  });
});

describe("summaryDescription", () => {
  it("is nothing for a daemon older than the field, and for an empty one", () => {
    expect(summaryDescription(summary())).toBeNull();
    expect(summaryDescription(summary({ description: "  " }))).toBeNull();
    expect(summaryDescription(summary({ description: "Database UI" }))).toBe("Database UI");
  });
});

describe("webAppSite", () => {
  it("finds the site the add-on names", () => {
    expect(webAppSite(summary(), [site("enabled")])?.domain).toBe("adminer.mixengine.test");
  });

  it("is nothing when the listing has no such site", () => {
    expect(webAppSite(summary(), [])).toBeNull();
    expect(webAppSite(summary({ site: null }), [site("enabled")])).toBeNull();
  });
});

describe("rowActions", () => {
  it("opens a web-app and offers the switch that applies", () => {
    expect(rowActions(summary(), undefined, site("enabled"))).toEqual(["open", "turnOff"]);
    expect(rowActions(summary(), undefined, site("disabled"))).toEqual(["open", "turnOn"]);
  });

  it("offers nothing but uninstall for a web-app whose site is missing", () => {
    expect(rowActions(summary(), undefined, null)).toEqual([]);
  });

  it("offers a service the one action its state allows", () => {
    const service = summary({ kind: "service", site: null });
    expect(rowActions(service, "stopped", null)).toEqual(["start"]);
    expect(rowActions(service, "failed", null)).toEqual(["start"]);
    expect(rowActions(service, undefined, null)).toEqual(["start"]);
    expect(rowActions(service, "running", null)).toEqual(["stop"]);
    expect(rowActions(service, "starting", null)).toEqual(["stop"]);
    expect(rowActions(service, "degraded", null)).toEqual(["stop"]);
    expect(rowActions(service, "stopping", null)).toEqual([]);
  });

  it("opens a service that declares its page, whatever its state", () => {
    const page = summary({ kind: "service", site: null, ui: "http://127.0.0.1:8025/" });
    expect(rowActions(page, "stopped", null)).toEqual(["open", "start"]);
    expect(rowActions(page, "running", null)).toEqual(["open", "stop"]);
    expect(rowActions(summary({ kind: "service", site: null }), "running", null)).toEqual(["stop"]);
  });

  it("offers a config add-on nothing to run", () => {
    expect(rowActions(summary({ kind: "recipe", site: null }), undefined, null)).toEqual([]);
  });
});
