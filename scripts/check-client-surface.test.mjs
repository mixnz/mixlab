// Tests for scripts/check-client-surface.mjs:  node --test scripts/check-client-surface.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { daemonMethods, calledMethods, problems } from './check-client-surface.mjs';

const RPC = `
pub mod method {
    /// Record a version that is on disk.
    pub const RUNTIME_ADOPT: &str = "runtime.adopt";
    pub const SERVICE_LIST: &str = "service.list";
    pub const UPDATE_CHECK: &str = "update.check";
}
pub const PROTOCOL: &str = "v1";
`;

test('the daemon methods are the dotted constants in rpc.rs', () => {
  assert.deepEqual(daemonMethods(RPC), ['runtime.adopt', 'service.list', 'update.check']);
});

test('a method the window calls is any of them quoted in its source', () => {
  const called = calledMethods(['rpc::call("service.list", json!({})).await', 'let x = "runtime.adopt";'], [
    'runtime.adopt',
    'service.list',
    'update.check',
  ]);
  assert.deepEqual([...called].sort(), ['runtime.adopt', 'service.list']);
});

const methods = ['runtime.adopt', 'service.list', 'update.check'];

test('every method is called, or listed with a reason', () => {
  const found = problems(methods, new Set(['service.list']), {
    cliOnly: { 'update.check': 'ADR 0056: MixLab updates itself' },
    knownGaps: { 'runtime.adopt': 'T182f: the Runtimes screen has no Adopt yet' },
  });
  assert.deepEqual(found, []);
});

test('a new method the window does not call and nobody listed is a problem', () => {
  const found = problems(methods, new Set(['service.list']), {
    cliOnly: { 'update.check': 'ADR 0056' },
    knownGaps: {},
  });
  assert.equal(found.length, 1);
  assert.match(found[0], /runtime\.adopt/);
});

test('a listed method with no reason is a problem', () => {
  const found = problems(methods, new Set(['service.list', 'runtime.adopt']), {
    cliOnly: { 'update.check': '  ' },
    knownGaps: {},
  });
  assert.equal(found.length, 1);
  assert.match(found[0], /update\.check.*reason/);
});

test('a listed method the window now calls is a stale entry', () => {
  const found = problems(methods, new Set(['service.list', 'runtime.adopt', 'update.check']), {
    cliOnly: { 'update.check': 'ADR 0056' },
    knownGaps: { 'runtime.adopt': 'T182f' },
  });
  assert.equal(found.length, 2);
  assert.ok(found.every((line) => /now calls/.test(line)), found.join('\n'));
});

test('a listed name that is not a daemon method is a stale entry', () => {
  const found = problems(methods, new Set(['service.list', 'runtime.adopt', 'update.check']), {
    cliOnly: { 'update.gone': 'renamed' },
    knownGaps: {},
  });
  assert.equal(found.length, 1);
  assert.match(found[0], /update\.gone/);
});
