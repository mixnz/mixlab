// Tests for scripts/check-strings.mjs:  node --test scripts/check-strings.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { lostContinuation } from './check-strings.mjs';

const indent = (text) => `            ${text}`;

test('two source lines joined where a `\\` went missing are found', () => {
  const line = indent(
    '"nothing routes this name: its TLD is not wired to the DNS server, and it has no line              in the hosts file"',
  );
  assert.equal(lostContinuation(line), true);
});

test('a continuation that turned into a literal \\n is found too', () => {
  const line = indent(
    'reason = "Windows refuses both directions before it would ever need a lock, so on that \\n              system this is compiled"',
  );
  assert.equal(lostContinuation(line), true);
});

test('a join before or after a format placeholder is found', () => {
  // The second line of a continued string: no opening quote of its own.
  const line = indent(
    ' give it a number of seconds, or remove the key for the default of              {DEFAULT_IDLE_CHECK_SECONDS}"',
  );
  assert.equal(lostContinuation(line, indent('"an idle check every 0 seconds is a loop; \\')), true);
  assert.equal(
    lostContinuation(
      indent('"{} is the default, which every project on this machine inherits, and {}              is ours"'),
    ),
    true,
  );
});

test('a string that continues properly is not', () => {
  assert.equal(
    lostContinuation(indent('"nothing routes this name: its TLD is not wired to the DNS server, and it has no line \\')),
    false,
  );
});

test('columns aligned on purpose in a short line are not', () => {
  assert.equal(lostContinuation(indent('"mariadb@main  11.4.3   running  no         yes         4123  —"')), false);
  assert.equal(lostContinuation(indent('"local   all   all                scram-sha-256",')), false);
});

test('spaces outside a string literal are not', () => {
  const line = indent(
    'let long_enough_to_pass_one_hundred_columns = something_else();                     // a comment that is aligned',
  );
  assert.equal(lostContinuation(line), false);
});

test('a line marked as aligned on purpose is not', () => {
  const line = indent(
    '"  public_id            TEXT    NOT NULL UNIQUE, and enough more text to pass one hundred columns", // aligned on purpose',
  );
  assert.equal(lostContinuation(line), false);
});

test('a line inside a continued string can be marked on the line above it', () => {
  const line = indent(
    '"{} — {answer}\\n  note        anything a service that does start depends on is started too, \\',
  );
  assert.equal(lostContinuation(line), true);
  assert.equal(lostContinuation(line, '        // aligned on purpose: the `note` column'), false);
});
