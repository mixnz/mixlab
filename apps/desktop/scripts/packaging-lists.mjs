/**
 * The binary list `packaging/common.sh` declares, read from the shell without running it.
 *
 * `MIX_BINARIES` and `MIX_CRATES` are two bash arrays paired by index, and `MIX_WINDOW` is the one
 * entry of the first that is not a headless binary. `crates/mixengine-core/tests/packaging.rs`
 * holds every Rust reader to that file; this is the one reader written in JavaScript, and it
 * carries no list of its own so that a sixth binary added there is staged here with no edit.
 *
 * Bash is not parsed: two shapes of assignment, `NAME=(a b c)` and `NAME=value`, are matched at
 * the start of a line, and anything else in the file is ignored.
 */

/** The words of a bash array assignment `name=(…)`, in order. */
export function bashArray(text, name) {
  const match = text.match(new RegExp(`^${name}=\\(([^)]*)\\)[ \\t]*$`, "m"));
  if (match === null) {
    throw new Error(`packaging/common.sh has no ${name}=(…) line`);
  }
  return match[1]
    .trim()
    .split(/\s+/)
    .filter((word) => word !== "");
}

/** The value of a bash scalar assignment `name=value`. */
export function bashScalar(text, name) {
  const match = text.match(new RegExp(`^${name}=(\\S+)[ \\t]*$`, "m"));
  if (match === null) {
    throw new Error(`packaging/common.sh has no ${name}= line`);
  }
  return match[1];
}

/**
 * Every headless binary and the crate that builds it: `MIX_BINARIES` minus `MIX_WINDOW`, each
 * paired with the `MIX_CRATES` entry at the same index.
 */
export function headlessBinaries(text) {
  const binaries = bashArray(text, "MIX_BINARIES");
  const crates = bashArray(text, "MIX_CRATES");
  const window = bashScalar(text, "MIX_WINDOW");
  if (binaries.length !== crates.length) {
    throw new Error(
      `packaging/common.sh: MIX_BINARIES has ${binaries.length} entries and MIX_CRATES has ` +
        `${crates.length}; they are paired by index`,
    );
  }
  return binaries
    .map((binary, index) => ({ binary, crate: crates[index] }))
    .filter(({ binary }) => binary !== window);
}
