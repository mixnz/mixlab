import { describe, expect, it } from "vitest";
import { mongoUriDatabase, mongoUriHost } from "./mongoUri";

describe("mongoUriHost", () => {
  it("leaves the credentials out", () => {
    expect(mongoUriHost("mongodb://user:secret@db.local:27017/app")).toBe("db.local:27017");
  });
  it("keeps a replica set's seed list, which is not a valid URL", () => {
    expect(mongoUriHost("mongodb://a:27017,b:27017/?replicaSet=rs0")).toBe("a:27017,b:27017");
  });
  it("reads an SRV string", () => {
    expect(mongoUriHost("mongodb+srv://cluster.example.net/app")).toBe("cluster.example.net");
  });
  it("says nothing about a string that is not Mongo's", () => {
    expect(mongoUriHost("postgres://h/db")).toBe("");
  });
});

describe("mongoUriDatabase", () => {
  it("reads the path before the options", () => {
    expect(mongoUriDatabase("mongodb://h/app?retryWrites=true")).toBe("app");
  });
  it("decodes a percent-encoded name", () => {
    expect(mongoUriDatabase("mongodb://h/my%20db")).toBe("my db");
  });
  it("keeps a malformed escape as written", () => {
    expect(mongoUriDatabase("mongodb://h/bad%zz")).toBe("bad%zz");
  });
  it("is empty when there is no path", () => {
    expect(mongoUriDatabase("mongodb://h")).toBe("");
  });
});
