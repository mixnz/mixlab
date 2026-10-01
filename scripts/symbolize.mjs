#!/usr/bin/env node
// Turn a crash report's offsets back into function names — roadmap task T91a.
//
//   node scripts/symbolize.mjs <crash-….json | crashes.json> [--symbols <dir | .sym.tar.gz>]
//
// A release `mixengined` ships without its symbol table, and its crash reports record each frame
// as an offset into the executable (docs/specs/2026-10-01-a-crash-report-names-its-frames-after-the-
// fact-design.md, ADR 0060). The release keeps the symbols as `mixengined-<version>-<target>.sym.tar.gz`
// beside the download. This fetches the one a report names, checks that its build identifier is the
// report's, and prints each frame's function.
//
// **No symboliser is needed.** The symbol file is read here: `.symtab` on Linux, `LC_SYMTAB` on
// macOS, the public symbols of the `.pdb` on Windows, with the executable beside it for its
// sections. Rust's legacy mangling is undone here too. So it runs wherever Node does, and CI needs
// nothing installed.
//
// Without `--symbols` the archive and its `.minisig` are downloaded with `gh` from the release
// `v<version>`, and the signature is checked with `minisign` against `packaging/updates.pub`.
//
// Exit status: 0 when every report was symbolised, 1 when one could not be (no symbols, a build
// identifier that does not match, a signature that does not verify), 64 for a misuse.

import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REPOSITORY = 'mixnz/mixlab';

// ---------------------------------------------------------------------------------------------
// Pure helpers, exported for scripts/symbolize.test.mjs.

/** The target triple a report's `os` and `arch` were built for. */
export function targetOf(os, arch) {
  const triples = {
    'linux x86_64': 'x86_64-unknown-linux-gnu',
    'linux aarch64': 'aarch64-unknown-linux-gnu',
    'macos x86_64': 'x86_64-apple-darwin',
    'macos aarch64': 'aarch64-apple-darwin',
    'windows x86_64': 'x86_64-pc-windows-msvc',
    'windows aarch64': 'aarch64-pc-windows-msvc',
  };
  const triple = triples[`${os} ${arch}`];
  if (!triple) throw new Error(`no release is built for ${os} ${arch}`);
  return triple;
}

/** The release asset `packaging/symbols.sh` writes for one target. */
export function assetName(version, target) {
  return `mixengined-${version}-${target}.sym.tar.gz`;
}

const ESCAPES = [
  ['$SP$', '@'], ['$BP$', '*'], ['$RF$', '&'], ['$LT$', '<'], ['$GT$', '>'], ['$LP$', '('],
  ['$RP$', ')'], ['$C$', ','], ['$u20$', ' '], ['$u22$', '"'], ['$u27$', "'"], ['$u2b$', '+'],
  ['$u3b$', ';'], ['$u5b$', '['], ['$u5d$', ']'], ['$u7b$', '{'], ['$u7d$', '}'], ['$u7e$', '~'],
];

/** A Rust symbol as a path: v0 (`_R…`, what the toolchain this repository pins emits)
 *  or legacy (`_ZN…E`), either with macOS's extra leading underscore. Anything else unchanged. */
export function demangle(name) {
  const symbol = name.startsWith('__') ? name.slice(1) : name;
  if (symbol.startsWith('_R')) {
    try {
      return demangleV0(symbol.slice(2));
    } catch {
      return name;
    }
  }
  return demangleLegacy(symbol) ?? name;
}

const BASIC_TYPES = {
  a: 'i8', b: 'bool', c: 'char', d: 'f64', e: 'str', f: 'f32', h: 'u8', i: 'isize', j: 'usize',
  l: 'i32', m: 'u32', n: 'i128', o: 'u128', p: '_', s: 'i16', t: 'u16', u: '()', v: '...',
  x: 'i64', y: 'u64', z: '!',
};

/** The v0 mangling (RFC 2603), after its `_R`. Lifetimes are left out of what is printed, and an
 *  impl is printed as `<Type>` without the module it sits in, as `rustfilt` prints them. */
function demangleV0(text) {
  let at = 0;
  let depth = 0;
  const bad = () => {
    throw new Error(`not a v0 symbol at ${at}`);
  };
  const eat = (character) => (text[at] === character ? (at++, true) : false);
  const nested = (parse) => () => {
    if (++depth > 256) bad();
    try {
      return parse();
    } finally {
      depth--;
    }
  };
  const base62 = () => {
    if (eat('_')) return 0;
    let value = 0;
    while (text[at] !== '_') {
      const digit = '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ'.indexOf(text[at++] ?? '');
      if (digit < 0) bad();
      value = value * 62 + digit;
    }
    at++;
    return value + 1;
  };
  const decimal = () => {
    const digits = /^\d+/.exec(text.slice(at));
    if (!digits) bad();
    at += digits[0].length;
    return Number(digits[0]);
  };
  // Absent is 0 and `s_` is 1: one more than the base-62 number it carries.
  const disambiguator = () => (eat('s') ? base62() + 1 : 0);
  const undisambiguated = () => {
    const punycode = eat('u');
    const length = decimal();
    eat('_');
    const name = text.slice(at, at + length);
    if (name.length !== length) bad();
    at += length;
    return punycode ? `{punycode ${name}}` : name;
  };
  const identifier = () => {
    const index = disambiguator();
    return { name: undisambiguated(), index };
  };
  const backref = (parse) => {
    const target = base62();
    if (target >= at) bad();
    const saved = at;
    at = target;
    const value = parse();
    at = saved;
    return value;
  };

  const path = nested(() => {
    switch (text[at++]) {
      case 'C':
        return identifier().name;
      case 'M':
        disambiguator();
        path();
        return `<${type()}>`;
      case 'X': {
        disambiguator();
        path();
        const self = type();
        return `<${self} as ${path()}>`;
      }
      case 'Y': {
        const self = type();
        return `<${self} as ${path()}>`;
      }
      case 'N': {
        const namespace = text[at++];
        const parent = path();
        const { name, index } = identifier();
        if (namespace === 'C') return `${parent}::{closure${name ? `:${name}` : ''}#${index}}`;
        if (namespace === 'S') return `${parent}::{shim${name ? `:${name}` : ''}#${index}}`;
        if (/[A-Z]/.test(namespace ?? '')) return `${parent}::{${namespace}${name ? `:${name}` : ''}#${index}}`;
        return name ? `${parent}::${name}` : parent;
      }
      case 'I': {
        const generic = path();
        const args = [];
        while (!eat('E')) {
          const arg = genericArg();
          if (arg !== null) args.push(arg);
        }
        return args.length ? `${generic}::<${args.join(', ')}>` : generic;
      }
      case 'B':
        return backref(path);
      default:
        return bad();
    }
  });

  const genericArg = () => {
    if (eat('L')) {
      base62();
      return null;
    }
    if (eat('K')) return constant();
    return type();
  };

  const constant = nested(() => {
    if (eat('p')) return '_';
    if (eat('B')) return backref(constant);
    type();
    const negative = eat('n');
    let hex = '';
    while (text[at] !== '_') {
      if (at >= text.length) bad();
      hex += text[at++];
    }
    at++;
    return `${negative ? '-' : ''}${hex ? BigInt(`0x${hex}`).toString() : '0'}`;
  });

  const signature = () => {
    if (eat('G')) base62();
    const unsafe = eat('U') ? 'unsafe ' : '';
    let abi = '';
    if (eat('K')) abi = `extern "${eat('C') ? 'C' : undisambiguated().replaceAll('_', '-')}" `;
    const args = [];
    while (!eat('E')) args.push(type());
    const output = type();
    return `${unsafe}${abi}fn(${args.join(', ')})${output === '()' ? '' : ` -> ${output}`}`;
  };

  const bounds = () => {
    if (eat('G')) base62();
    const traits = [];
    while (!eat('E')) {
      const trait = path();
      const bindings = [];
      while (eat('p')) {
        const name = undisambiguated();
        bindings.push(`${name} = ${type()}`);
      }
      traits.push(bindings.length ? `${trait}<${bindings.join(', ')}>` : trait);
    }
    return traits.join(' + ');
  };

  const type = nested(() => {
    const tag = text[at];
    if (tag !== undefined && Object.hasOwn(BASIC_TYPES, tag)) {
      at++;
      return BASIC_TYPES[tag];
    }
    at++;
    switch (tag) {
      case 'A': {
        const element = type();
        return `[${element}; ${constant()}]`;
      }
      case 'S':
        return `[${type()}]`;
      case 'T': {
        const items = [];
        while (!eat('E')) items.push(type());
        return `(${items.join(', ')}${items.length === 1 ? ',' : ''})`;
      }
      case 'R':
      case 'Q':
        if (eat('L')) base62();
        return `&${tag === 'Q' ? 'mut ' : ''}${type()}`;
      case 'P':
        return `*const ${type()}`;
      case 'O':
        return `*mut ${type()}`;
      case 'F':
        return signature();
      case 'D': {
        const traits = bounds();
        if (eat('L')) base62();
        return `dyn ${traits}`;
      }
      case 'B':
        return backref(type);
      default:
        at--;
        return path();
    }
  });

  if (/\d/.test(text[at] ?? '')) decimal(); // an encoding version, which only 0 has ever been
  return path();
}

function demangleLegacy(mangled) {
  if (!mangled.startsWith('_ZN') || !mangled.endsWith('E')) return null;

  const parts = [];
  let at = 3;
  while (at < mangled.length - 1) {
    const length = /^\d+/.exec(mangled.slice(at));
    if (!length) return null;
    at += length[0].length;
    parts.push(mangled.slice(at, at + Number(length[0])));
    at += Number(length[0]);
  }
  if (parts.length > 1 && /^h[0-9a-f]{16}$/.test(parts.at(-1))) parts.pop();

  return parts
    .map((part) => {
      let text = part.startsWith('_$') ? part.slice(1) : part;
      text = text.replaceAll('..', '::');
      for (const [from, to] of ESCAPES) text = text.replaceAll(from, to);
      return text;
    })
    .join('::');
}

/** Which object format a buffer is, by its magic. */
export function formatOf(bytes) {
  if (bytes.length >= 4 && bytes.readUInt32BE(0) === 0x7f454c46) return 'elf';
  if (bytes.length >= 4 && bytes.readUInt32LE(0) === 0xfeedfacf) return 'macho';
  if (bytes.length >= 2 && bytes.readUInt16LE(0) === 0x5a4d) return 'pe';
  return null;
}

/** The build identifier, spelled as the daemon's report spells it, or null. */
export function buildIdOf(bytes) {
  switch (formatOf(bytes)) {
    case 'elf': return elfBuildId(bytes);
    case 'macho': return machoUuid(bytes);
    case 'pe': return peCodeView(bytes);
    default: return null;
  }
}

function elfBuildId(bytes) {
  const programs = bytes.readBigUInt64LE(0x20);
  const size = bytes.readUInt16LE(0x36);
  const count = bytes.readUInt16LE(0x38);
  for (let index = 0; index < count; index++) {
    const header = Number(programs) + index * size;
    if (bytes.readUInt32LE(header) !== 4) continue; // PT_NOTE
    const start = Number(bytes.readBigUInt64LE(header + 8));
    const end = start + Number(bytes.readBigUInt64LE(header + 32));
    let at = start;
    while (at + 12 <= end) {
      const nameSize = bytes.readUInt32LE(at);
      const descriptorSize = bytes.readUInt32LE(at + 4);
      const kind = bytes.readUInt32LE(at + 8);
      const nameAt = at + 12;
      const descriptorAt = nameAt + Math.ceil(nameSize / 4) * 4;
      if (kind === 3 && bytes.toString('latin1', nameAt, nameAt + nameSize) === 'GNU\0') {
        return bytes.subarray(descriptorAt, descriptorAt + descriptorSize).toString('hex');
      }
      at = descriptorAt + Math.ceil(descriptorSize / 4) * 4;
    }
  }
  return null;
}

function machoCommands(bytes) {
  const count = bytes.readUInt32LE(16);
  const commands = [];
  let at = 32;
  for (let index = 0; index < count; index++) {
    commands.push({ kind: bytes.readUInt32LE(at), at });
    at += bytes.readUInt32LE(at + 4);
  }
  return commands;
}

function machoUuid(bytes) {
  const uuid = machoCommands(bytes).find((command) => command.kind === 0x1b);
  if (!uuid) return null;
  const hex = bytes.subarray(uuid.at + 8, uuid.at + 24).toString('hex').toUpperCase();
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function peSections(bytes) {
  const pe = bytes.readUInt32LE(0x3c);
  const count = bytes.readUInt16LE(pe + 6);
  const optionalSize = bytes.readUInt16LE(pe + 20);
  const sections = [];
  for (let index = 0; index < count; index++) {
    const at = pe + 24 + optionalSize + index * 40;
    sections.push({
      address: bytes.readUInt32LE(at + 12),
      size: bytes.readUInt32LE(at + 8),
      raw: bytes.readUInt32LE(at + 20),
    });
  }
  return sections;
}

function peCodeView(bytes) {
  const pe = bytes.readUInt32LE(0x3c);
  const optional = pe + 24;
  if (bytes.readUInt16LE(optional) !== 0x20b) return null; // PE32+
  const directory = optional + 112 + 6 * 8;
  const rva = bytes.readUInt32LE(directory);
  const size = bytes.readUInt32LE(directory + 4);
  const section = peSections(bytes).find((s) => rva >= s.address && rva < s.address + s.size);
  if (!section) return null;
  const start = rva - section.address + section.raw;
  for (let entry = start; entry < start + size; entry += 28) {
    if (bytes.readUInt32LE(entry + 12) !== 2) continue; // IMAGE_DEBUG_TYPE_CODEVIEW
    const record = bytes.readUInt32LE(entry + 24); // PointerToRawData, a file offset
    if (bytes.toString('latin1', record, record + 4) !== 'RSDS') continue;
    return guidAndAge(bytes.subarray(record + 4, record + 20), bytes.readUInt32LE(record + 20));
  }
  return null;
}

/** A GUID and age as symbol servers spell them: the GUID's fields in order, the age unpadded. */
export function guidAndAge(guid, age) {
  const hex = (value, width) => value.toString(16).toUpperCase().padStart(width, '0');
  return (
    hex(guid.readUInt32LE(0), 8) + hex(guid.readUInt16LE(4), 4) + hex(guid.readUInt16LE(6), 4) +
    guid.subarray(8, 16).toString('hex').toUpperCase() + age.toString(16).toUpperCase()
  );
}

/** Every function symbol: `{ address, size, name }`, sorted by address. */
export function symbolsOf(bytes, pdb) {
  switch (formatOf(bytes)) {
    case 'elf': return sorted(elfSymbols(bytes));
    case 'macho': return sorted(machoSymbols(bytes));
    case 'pe': return sorted(pdbSymbols(pdb, peSections(bytes)));
    default: throw new Error('not an executable this script reads');
  }
}

function sorted(symbols) {
  return symbols.sort((a, b) => a.address - b.address);
}

function elfSymbols(bytes) {
  const sections = Number(bytes.readBigUInt64LE(0x28));
  const size = bytes.readUInt16LE(0x3a);
  const count = bytes.readUInt16LE(0x3c);
  const header = (index) => sections + index * size;
  const symbols = [];
  for (let index = 0; index < count; index++) {
    if (bytes.readUInt32LE(header(index) + 4) !== 2) continue; // SHT_SYMTAB
    const offset = Number(bytes.readBigUInt64LE(header(index) + 24));
    const length = Number(bytes.readBigUInt64LE(header(index) + 32));
    const strings = header(bytes.readUInt32LE(header(index) + 40));
    const stringsAt = Number(bytes.readBigUInt64LE(strings + 24));
    for (let entry = offset; entry + 24 <= offset + length; entry += 24) {
      if ((bytes.readUInt8(entry + 4) & 0xf) !== 2) continue; // STT_FUNC
      const address = Number(bytes.readBigUInt64LE(entry + 8));
      if (address === 0) continue;
      const nameAt = stringsAt + bytes.readUInt32LE(entry);
      symbols.push({
        address,
        size: Number(bytes.readBigUInt64LE(entry + 16)),
        name: bytes.toString('latin1', nameAt, bytes.indexOf(0, nameAt)),
      });
    }
  }
  return symbols;
}

function machoSymbols(bytes) {
  const table = machoCommands(bytes).find((command) => command.kind === 0x2);
  if (!table) return [];
  const symbolsAt = bytes.readUInt32LE(table.at + 8);
  const count = bytes.readUInt32LE(table.at + 12);
  const stringsAt = bytes.readUInt32LE(table.at + 16);
  const symbols = [];
  for (let index = 0; index < count; index++) {
    const entry = symbolsAt + index * 16;
    const type = bytes.readUInt8(entry + 4);
    if ((type & 0xe0) !== 0 || (type & 0x0e) !== 0x0e) continue; // a debug entry, or not in a section
    const nameAt = stringsAt + bytes.readUInt32LE(entry);
    symbols.push({
      address: Number(bytes.readBigUInt64LE(entry + 8)),
      size: null,
      name: bytes.toString('latin1', nameAt, bytes.indexOf(0, nameAt)),
    });
  }
  return symbols;
}

/** The streams of an MSF file (a `.pdb`), as buffers by index. */
function msfStreams(pdb) {
  if (!pdb.toString('latin1', 0, 24).startsWith('Microsoft C/C++ MSF 7.00')) {
    throw new Error('not a PDB');
  }
  const blockSize = pdb.readUInt32LE(32);
  const directoryBytes = pdb.readUInt32LE(44);
  const blockMap = pdb.readUInt32LE(52);
  const block = (index) => pdb.subarray(index * blockSize, (index + 1) * blockSize);
  const directoryBlocks = Math.ceil(directoryBytes / blockSize);
  const directory = Buffer.concat(
    Array.from({ length: directoryBlocks }, (_, i) => block(block(blockMap).readUInt32LE(i * 4))),
  ).subarray(0, directoryBytes);

  const count = directory.readUInt32LE(0);
  const sizes = Array.from({ length: count }, (_, i) => directory.readUInt32LE(4 + i * 4));
  let at = 4 + count * 4;
  return sizes.map((size) => {
    if (size === 0xffffffff) return Buffer.alloc(0);
    const blocks = Math.ceil(size / blockSize);
    const parts = Array.from({ length: blocks }, (_, i) => block(directory.readUInt32LE(at + i * 4)));
    at += blocks * 4;
    return Buffer.concat(parts).subarray(0, size);
  });
}

/** The GUID and age of a `.pdb`, from its info stream, spelled as `guidAndAge`. */
export function pdbIdOf(pdb) {
  const info = msfStreams(pdb)[1];
  return guidAndAge(info.subarray(12, 28), info.readUInt32LE(8));
}

/** A `.pdb`'s functions. **The procedures each module records, when there are any**: they are every
 *  function, private ones included, with their sizes, and they are there because `stage.sh` builds
 *  Windows releases with line tables (T91a). The public symbols are the fallback, and they are only
 *  the functions that stayed external, which after LTO is a minority: measured on 2026-10-01, a
 *  release daemon read through its publics alone put most frames in the wrong function. */
function pdbSymbols(pdb, sections) {
  if (!pdb) throw new Error('a Windows executable is read with its .pdb');
  const streams = msfStreams(pdb);
  const procedures = pdbProcedures(streams, sections);
  return procedures.length ? procedures : pdbPublics(streams, sections);
}

/** `S_GPROC32`, `S_LPROC32` and their `_ID` forms, from every module's symbol stream. */
function pdbProcedures(streams, sections) {
  const dbi = streams[3];
  const modulesEnd = 64 + dbi.readInt32LE(24); // the header, then `ModInfoSize` bytes of modules
  const procedures = [];
  for (let module = 64; module < modulesEnd; ) {
    const stream = dbi.readUInt16LE(module + 34);
    const symbolBytes = dbi.readUInt32LE(module + 36);
    // Two names follow the fixed 64 bytes, and the entry is padded to four.
    const nameEnd = dbi.indexOf(0, dbi.indexOf(0, module + 64) + 1);
    module = Math.ceil((nameEnd + 1) / 4) * 4;

    const records = streams[stream];
    if (stream === 0xffff || !records) continue;
    for (let at = 4; at + 4 <= Math.min(symbolBytes, records.length); ) {
      const length = records.readUInt16LE(at);
      const kind = records.readUInt16LE(at + 2);
      if (kind === 0x1110 || kind === 0x110f || kind === 0x1147 || kind === 0x1146) {
        const section = sections[records.readUInt16LE(at + 36) - 1];
        if (section) {
          const nameAt = at + 39;
          procedures.push({
            address: section.address + records.readUInt32LE(at + 32),
            size: records.readUInt32LE(at + 16),
            name: records.toString('utf8', nameAt, records.indexOf(0, nameAt)),
          });
        }
      }
      at += 2 + length;
    }
  }
  return procedures;
}

function pdbPublics(streams, sections) {
  const records = streams[streams[3].readUInt16LE(20)]; // the DBI header's symbol-record stream
  const symbols = [];
  for (let at = 0; at + 4 <= records.length; ) {
    const length = records.readUInt16LE(at);
    if (records.readUInt16LE(at + 2) === 0x110e) { // S_PUB32
      const offset = records.readUInt32LE(at + 8);
      const section = sections[records.readUInt16LE(at + 12) - 1];
      if (section) {
        const nameAt = at + 14;
        symbols.push({
          address: section.address + offset,
          size: null,
          name: records.toString('latin1', nameAt, records.indexOf(0, nameAt)),
        });
      }
    }
    at += 2 + length;
  }
  return symbols;
}

/** The regular files of a tar, by base name. Read here rather than by `tar`, because Git Bash's
 *  GNU tar takes `C:` in a path for a remote host, and Windows' own takes other flags. */
export function untar(bytes) {
  const files = [];
  for (let at = 0; at + 512 <= bytes.length; ) {
    const header = bytes.subarray(at, at + 512);
    if (header.every((byte) => byte === 0)) break;
    const field = (start, length) => header.toString('latin1', start, start + length).replace(/\0.*$/s, '');
    const size = parseInt(field(124, 12).trim() || '0', 8);
    const type = field(156, 1) || '0';
    const name = path.posix.basename(field(0, 100));
    if (type === '0' && name) files.push([name, bytes.subarray(at + 512, at + 512 + size)]);
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return files;
}

/** The symbol an address falls in: the last at or below it, and inside its size when it has one. */
export function lookup(symbols, address) {
  let low = 0;
  let high = symbols.length - 1;
  let found = null;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (symbols[middle].address <= address) {
      found = symbols[middle];
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  if (!found || (found.size && address >= found.address + found.size)) return null;
  return found;
}

/** One report's frames, as printable lines. A return address is looked up one byte earlier, in
 *  the call that made it rather than the instruction after. */
export function symbolise(report, symbols) {
  return report.frames.map((frame, index) => {
    if (!frame.offset) return `${String(index).padStart(3)}  (outside the executable)`;
    const symbol = lookup(symbols, Number(BigInt(frame.offset)) - 1);
    return `${String(index).padStart(3)}  ${frame.offset.padEnd(12)}${symbol ? demangle(symbol.name) : '??'}`;
  });
}

// ---------------------------------------------------------------------------------------------
// The command.

function fail(code, message) {
  process.stderr.write(`${message}\n`);
  process.exit(code);
}

function run(command, args) {
  try {
    return execFileSync(command, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
  } catch (error) {
    fail(1, `${command} ${args.join(' ')} failed:\n${error.stderr ?? error.message}`);
  }
}

/** The directory the symbols are in, unpacked: `--symbols` as given, or downloaded and verified. */
function symbolsFor(report, given, scratch) {
  const unpack = (archive) => {
    for (const [name, bytes] of untar(gunzipSync(readFileSync(archive)))) {
      writeFileSync(path.join(scratch, name), bytes);
    }
    return scratch;
  };
  if (given) {
    return statSync(given).isDirectory() ? given : unpack(given);
  }

  const name = assetName(report.daemon.version, targetOf(report.os, report.arch));
  run('gh', [
    'release', 'download', `v${report.daemon.version}`, '--repo', REPOSITORY,
    '--pattern', name, '--pattern', `${name}.minisig`, '--dir', scratch,
  ]);
  const key = readFileSync(path.join(ROOT, 'packaging', 'updates.pub'), 'utf8').split(/\r?\n/)[1];
  run('minisign', ['-V', '-H', '-P', key, '-m', path.join(scratch, name), '-q']);
  return unpack(path.join(scratch, name));
}

function main(argv) {
  let input = null;
  let given = null;
  for (let index = 0; index < argv.length; index++) {
    if (argv[index] === '--symbols') given = argv[++index];
    else if (!input) input = argv[index];
    else fail(64, `unexpected argument: ${argv[index]}`);
  }
  if (!input) fail(64, 'usage: node scripts/symbolize.mjs <report.json | crashes.json> [--symbols <dir | archive>]');

  const parsed = JSON.parse(readFileSync(input, 'utf8'));
  const reports = Array.isArray(parsed) ? parsed : [parsed];
  let failed = false;

  for (const report of reports) {
    const at = report.location ? `${report.location.file}:${report.location.line}:${report.location.column}` : 'an unknown location';
    console.log(`panic at ${at}, ${report.daemon.version} on ${report.os} ${report.arch}, format ${report.format}`);

    if (report.format === 1 || !report.frames?.length) {
      for (const name of report.symbols ?? report.frames ?? []) console.log(`       ${name}`);
      continue;
    }

    const scratch = mkdtempSync(path.join(tmpdir(), 'symbolize-'));
    try {
      const directory = symbolsFor(report, given, scratch);
      const files = readdirSync(directory);
      const executable = files.find((file) => file === 'mixengined.sym' || file === 'mixengined.exe');
      if (!executable) {
        fail(1, `no mixengined.sym or mixengined.exe in ${directory}`);
      }
      const bytes = readFileSync(path.join(directory, executable));
      const pdbPath = path.join(directory, 'mixengined.pdb');
      const pdb = existsSync(pdbPath) ? readFileSync(pdbPath) : null;

      const id = buildIdOf(bytes);
      if (!report.build_id || id !== report.build_id) {
        console.log(`  the symbols are build ${id ?? 'without an identifier'}, the report ${report.build_id ?? 'names none'}: not this build's symbols`);
        failed = true;
        continue;
      }
      if (pdb && pdbIdOf(pdb) !== id) {
        console.log(`  the .pdb is ${pdbIdOf(pdb)}, its executable ${id}: they are not one build`);
        failed = true;
        continue;
      }

      for (const line of symbolise(report, symbolsOf(bytes, pdb))) console.log(line);
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  }

  process.exit(failed ? 1 : 0);
}

if (path.basename(process.argv[1] ?? '') === 'symbolize.mjs') main(process.argv.slice(2));
