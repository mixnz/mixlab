#!/usr/bin/env node
// Every daemon method is reachable from MixLab, or says why not.
//
//   node scripts/check-client-surface.mjs
//
// MixLab is the product (CLAUDE.md, ADR 0056), and `mix` being able to do something is not the same
// as a person in the window being able to. The rule that every mutating method reaches `mix` is
// written down and kept; its other half — that the window reaches them too — was only a sentence,
// and a feature shipped with a `mix runtime adopt` and no button. This makes the other half a check.
//
// **What counts as called:** the method's name quoted anywhere under `apps/desktop/src-tauri/src`,
// which is where every call to the daemon is made (`rpc::call("service.list", …)`).
//
// **What may be missing:** a method listed in `apps/desktop/client-surface-exceptions.json`, with a
// reason, in one of two groups:
//
// - `cliOnly` — the window has no use for it, and the reason says why (ADR 0056: MixLab updates
//   itself; the uninstaller drives `daemon.uninstall`).
// - `knownGaps` — the window should have it and does not yet. This list only ever shrinks: a new
//   method goes into the window, or into `cliOnly` with a reason somebody can review.
//
// Fails on a method in neither place, on an entry with no reason, and on an entry that no longer
// applies (the window calls it now, or no such method exists).
//
// Exit status: 0 when nothing is found, 1 when something is.

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(fileURLToPath(import.meta.url), '..', '..');
const RPC = 'crates/mixengine-proto/src/rpc.rs';
const WINDOW = 'apps/desktop/src-tauri/src';
const EXCEPTIONS = 'apps/desktop/client-surface-exceptions.json';

/** The method names in `rpc.rs`: every `&str` constant whose value has a dot in it. */
export function daemonMethods(source) {
  const names = [...source.matchAll(/pub const [A-Z0-9_]+: &str = "([a-z_]+\.[a-z_.]+)";/g)].map(
    ([, name]) => name,
  );
  return [...new Set(names)].sort();
}

/** Which of `methods` appear quoted in any of `sources`. */
export function calledMethods(sources, methods) {
  const wanted = new Set(methods);
  const called = new Set();
  for (const source of sources) {
    for (const [, name] of source.matchAll(/"([a-z_]+\.[a-z_.]+)"/g)) {
      if (wanted.has(name)) called.add(name);
    }
  }
  return called;
}

/** Every way `methods`, `called` and `exceptions` disagree, one sentence each. */
export function problems(methods, called, exceptions) {
  const found = [];
  const known = new Set(methods);
  const groups = [
    ['cliOnly', exceptions.cliOnly ?? {}],
    ['knownGaps', exceptions.knownGaps ?? {}],
  ];

  for (const [group, entries] of groups) {
    for (const [method, reason] of Object.entries(entries)) {
      if (!known.has(method)) {
        found.push(`${method} (${group}) is not a daemon method any more; remove it from ${EXCEPTIONS}`);
      } else if (called.has(method)) {
        found.push(`${method} (${group}): the window now calls it; remove it from ${EXCEPTIONS}`);
      } else if (typeof reason !== 'string' || reason.trim() === '') {
        found.push(`${method} (${group}) has no reason in ${EXCEPTIONS}`);
      }
    }
  }

  const listed = new Set(groups.flatMap(([, entries]) => Object.keys(entries)));
  for (const method of methods) {
    if (!called.has(method) && !listed.has(method)) {
      found.push(
        `${method} is a daemon method MixLab cannot reach: give the window its screen, or list it in ` +
          `${EXCEPTIONS} with a reason`,
      );
    }
  }

  return found;
}

function* rustFiles(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      yield* rustFiles(path);
    } else if (name.endsWith('.rs')) {
      yield path;
    }
  }
}

function main() {
  const methods = daemonMethods(readFileSync(join(ROOT, RPC), 'utf8'));
  const sources = [...rustFiles(join(ROOT, WINDOW))].map((file) => readFileSync(file, 'utf8'));
  const exceptions = JSON.parse(readFileSync(join(ROOT, EXCEPTIONS), 'utf8'));
  const found = problems(methods, calledMethods(sources, methods), exceptions);

  if (found.length === 0) {
    const gaps = Object.keys(exceptions.knownGaps ?? {}).length;
    console.log(`client surface: ok (${methods.length} methods, ${gaps} known gap(s) in the window)`);
    return;
  }

  for (const line of found) console.error(line);
  console.error(`client surface: ${found.length} problem(s)`);
  process.exit(1);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === join(process.argv[1])) {
  main();
}
