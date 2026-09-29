import { describe, expect, it } from "vitest";
import type { ListeningPort } from "./api";
import { matchesFilter } from "./filter";

const row = (over: Partial<ListeningPort> = {}): ListeningPort => ({
  port: 3000,
  address: "0.0.0.0",
  pid: 123,
  process: "node.exe",
  ...over,
});

describe("matchesFilter", () => {
  it("accepts every row when the filter is empty", () => {
    expect(matchesFilter(row(), "")).toBe(true);
    expect(matchesFilter(row(), "   ")).toBe(true);
  });

  it("matches by port number", () => {
    expect(matchesFilter(row({ port: 8080 }), "8080")).toBe(true);
  });

  // Typing `80` finds 80, 8080 and 3080 — handy when you do not remember the exact port.
  it("matches ports by substring", () => {
    expect(matchesFilter(row({ port: 8080 }), "80")).toBe(true);
    expect(matchesFilter(row({ port: 3080 }), "80")).toBe(true);
  });

  it("matches by process name", () => {
    expect(matchesFilter(row({ process: "postgres" }), "postgres")).toBe(true);
  });

  // People type `node`, not `Node.exe`.
  it("ignores case in process names", () => {
    expect(matchesFilter(row({ process: "Node.exe" }), "node")).toBe(true);
    expect(matchesFilter(row({ process: "node.exe" }), "NODE")).toBe(true);
  });

  it("matches part of a process name", () => {
    expect(matchesFilter(row({ process: "com.docker.backend" }), "docker")).toBe(true);
  });

  it("does not break when the process name could not be looked up", () => {
    expect(matchesFilter(row({ process: null }), "node")).toBe(false);
    expect(matchesFilter(row({ process: null, port: 3000 }), "3000")).toBe(true);
  });

  it("excludes rows matching neither port nor name", () => {
    expect(matchesFilter(row({ port: 3000, process: "node.exe" }), "nginx")).toBe(false);
  });
});
