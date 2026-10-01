// Tests for scripts/symbolize.mjs:  node --test scripts/symbolize.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  targetOf, assetName, demangle, formatOf, buildIdOf, guidAndAge, lookup, symbolise, untar,
} from './symbolize.mjs';

test('a report names its target', () => {
  assert.equal(targetOf('linux', 'x86_64'), 'x86_64-unknown-linux-gnu');
  assert.equal(targetOf('macos', 'aarch64'), 'aarch64-apple-darwin');
  assert.equal(targetOf('windows', 'aarch64'), 'aarch64-pc-windows-msvc');
  assert.throws(() => targetOf('freebsd', 'x86_64'), /no release/);
});

test('the asset is the name packaging/symbols.sh writes, and no feed glob matches it', () => {
  const name = assetName('0.0.14', 'x86_64-apple-darwin');
  assert.equal(name, 'mixengined-0.0.14-x86_64-apple-darwin.sym.tar.gz');
  assert.ok(!name.startsWith('mixlab-'));
  assert.ok(!name.startsWith('mixengine-elevate-'));
});

test('v0 names, as the pinned toolchain emits them, read as paths', () => {
  assert.equal(
    demangle('_RNvNtNtCsdJstzbIjHNu_10mixengined5crash5probe14raise_if_asked'),
    'mixengined::crash::probe::raise_if_asked',
  );
  // Two closures inside an inherent method, the type's path a backref into the impl's own.
  assert.equal(
    demangle('_RNCNCNvMNtCsdJstzbIjHNu_10mixengined5crashNtB6_7Reports7install0s_0B8_'),
    '<mixengined::crash::Reports>::install::{closure#0}::{closure#1}',
  );
  // Generic arguments, a basic type as the impl's self, and backrefs to a crate root.
  assert.equal(
    demangle('_RINvMNtCs9OCfJ4KTcUD_4core4boolb4thenINtNtB5_6result6ResultNtNtCsbSQC797JefK_3std4path7PathBufNtNtNtB5_2io5error5ErrorEE'),
    '<bool>::then::<core::result::Result::<std::path::PathBuf, core::io::error::Error>>',
  );
  // macOS's extra underscore.
  assert.equal(demangle('__RNvCs6rREvFdRhLb_7___rustc17rust_begin_unwind'), '__rustc::rust_begin_unwind');
});

test('legacy names read as paths without their hash', () => {
  assert.equal(demangle('_ZN4core3fmt5write17h0123456789abcdefE'), 'core::fmt::write');
  assert.equal(demangle('__ZN3std2rt10lang_start17h0123456789abcdefE'), 'std::rt::lang_start');
  assert.equal(demangle('_ZN5alloc3vec12Vec$LT$T$GT$4push17h0123456789abcdefE'), 'alloc::vec::Vec<T>::push');
});

test('anything else, and a v0 name that does not parse, is left as it is', () => {
  assert.equal(demangle('memcpy'), 'memcpy');
  assert.equal(demangle('_RNvZZ'), '_RNvZZ');
});

/** A little-endian ELF64 with one PT_NOTE holding a GNU build-id. */
function elfWithBuildId(id) {
  const elf = Buffer.alloc(0x200);
  elf.writeUInt32BE(0x7f454c46, 0);
  elf.writeBigUInt64LE(0x40n, 0x20); // e_phoff
  elf.writeUInt16LE(56, 0x36); // e_phentsize
  elf.writeUInt16LE(1, 0x38); // e_phnum
  elf.writeUInt32LE(4, 0x40); // PT_NOTE
  elf.writeBigUInt64LE(0x100n, 0x48); // p_offset
  const note = Buffer.concat([
    Buffer.from([4, 0, 0, 0, id.length, 0, 0, 0, 3, 0, 0, 0]),
    Buffer.from('GNU\0', 'latin1'),
    id,
  ]);
  note.copy(elf, 0x100);
  elf.writeBigUInt64LE(BigInt(note.length), 0x40 + 32); // p_filesz
  return elf;
}

test('the build identifier is read as the daemon spells it', () => {
  const elf = elfWithBuildId(Buffer.from('deadbeef01', 'hex'));
  assert.equal(formatOf(elf), 'elf');
  assert.equal(buildIdOf(elf), 'deadbeef01');

  const macho = Buffer.alloc(0x100);
  macho.writeUInt32LE(0xfeedfacf, 0);
  macho.writeUInt32LE(1, 16); // ncmds
  macho.writeUInt32LE(0x1b, 32); // LC_UUID
  macho.writeUInt32LE(24, 36);
  Buffer.from([...Array(16).keys()]).copy(macho, 40);
  assert.equal(buildIdOf(macho), '00010203-0405-0607-0809-0A0B0C0D0E0F');

  const guid = Buffer.from([0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0x78, 0x56, 1, 2, 3, 4, 5, 6, 7, 8]);
  assert.equal(guidAndAge(guid, 3), '123456781234567801020304050607083');
});

test('an address is the symbol at or below it, and inside its size when it has one', () => {
  const symbols = [
    { address: 0x1000, size: 0x10, name: 'a' },
    { address: 0x2000, size: null, name: 'b' },
  ];
  assert.equal(lookup(symbols, 0x1008)?.name, 'a');
  assert.equal(lookup(symbols, 0x1010), null);
  assert.equal(lookup(symbols, 0x2fff)?.name, 'b');
  assert.equal(lookup(symbols, 0x0fff), null);
});

test('a return address is looked up one byte earlier, and a frame outside says so', () => {
  const symbols = [
    { address: 0x1000, size: 0x10, name: '_ZN1a1f17h0123456789abcdefE' },
    { address: 0x1010, size: 0x10, name: '_ZN1a1g17h0123456789abcdefE' },
  ];
  const lines = symbolise({ frames: [{ offset: '0x1010' }, {}] }, symbols);
  assert.match(lines[0], /0x1010\s+a::f$/);
  assert.match(lines[1], /outside the executable/);
});

test('a tar is read without tar, regular files by base name', () => {
  const entry = (name, body, type = '0') => {
    const header = Buffer.alloc(512);
    header.write(name, 0, 'latin1');
    header.write(body.length.toString(8).padStart(11, '0'), 124, 'latin1');
    header.write(type, 156, 'latin1');
    const padded = Buffer.alloc(Math.ceil(body.length / 512) * 512);
    body.copy(padded);
    return Buffer.concat([header, padded]);
  };
  const archive = Buffer.concat([
    entry('./', Buffer.alloc(0), '5'),
    entry('./mixengined.sym', Buffer.from('symbols')),
    Buffer.alloc(1024),
  ]);
  const files = untar(archive);
  assert.deepEqual(files.map(([name, bytes]) => [name, bytes.toString()]), [['mixengined.sym', 'symbols']]);
});
