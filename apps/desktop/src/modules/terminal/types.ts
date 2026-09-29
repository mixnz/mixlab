/** A shell detected on this machine. `name` is a stable identifier — `shells.ts` turns it into a
 *  label. */
export interface LocalShell {
  /** `powershell`, `pwsh`, `cmd`, `git-bash`, `wsl:<distro>`, `zsh`, `bash`, `sh`. */
  name: string;
  path: string;
  /** That shell's fixed arguments; empty for most, `["-d", "<distro>"]` for WSL. */
  args: string[];
}

export interface TerminalSize {
  cols: number;
  rows: number;
}

/* The SSH server belongs to `core/ssh.ts`, not to this module: on the Rust side there is a single
   `SshConfig` shared by the db tunnel and the terminal session, and this used to be one of its two
   copies. Re-exported rather than making every place change its import. */
import type { SshConfig } from "../../core/ssh";
export type { SshAuth, SshConfig } from "../../core/ssh";

/**
 * A target the user has saved: a shell on this machine, or an SSH server.
 *
 * The left column of `TargetForm` is this list, and it deliberately mixes the two kinds: it is
 * *the places I open often*, not a server list — so it still shows while the form is on "This
 * machine", just as it always has.
 *
 * Entries written by the old version have no `kind`. `parseSavedTarget` reads them as `ssh`,
 * because there was no other kind back then — there is no migration step, and the file only
 * changes shape when the user saves again.
 */
export type SavedTarget = SavedLocalTarget | SavedSshTarget;

interface SavedTargetBase {
  id: string;
  name: string;
  /**
   * Commands typed on the user's behalf as soon as the shell speaks — `cd ~/project-a/frontend`,
   * `nvm use`, the first few lines you would otherwise retype on every visit. Each line is one
   * command; `openingKeystrokes` in `session.ts` turns this field into keys.
   *
   * Rust never sees it: it goes down to the pty like keys the user pressed. And it sits verbatim
   * in `terminal-hosts.json`, so it is not a place for passwords; see the top of
   * `savedTargets.ts`.
   */
  runOnConnect?: string;
}

/** A shell on this machine. `shellName` rather than a path — `powershell`, `wsl:Ubuntu`: the name
 *  is a stable identifier while the path changes from machine to machine, just as
 *  `TerminalTabState` decided. */
export interface SavedLocalTarget extends SavedTargetBase {
  kind: "local";
  shellName: string;
  cwd: string | null;
}

/** An SSH server. `config` here is always complete — `savedTargets.ts` merges the secret part in
 *  from the credential store before handing it to anyone. */
export interface SavedSshTarget extends SavedTargetBase {
  kind: "ssh";
  config: SshConfig;
}

/** A session's target, exactly the shape of the Rust-side `TerminalTarget`. The `ssh` branch
 *  flattens `SshConfig`'s four fields because on the Rust side it is a newtype variant in an enum
 *  with a `tag`. */
export type TerminalTarget =
  | { type: "local"; shell: string; args: string[]; cwd: string | null }
  | ({ type: "ssh" } & SshConfig);

/**
 * What the user chose in the form. Wider than `TerminalTarget`: it also keeps the `LocalShell` to
 * name the tab, `targetId` to know which saved target this session came from, and `runOnConnect`
 * to type the first few lines on the user's behalf — three things Rust does not need to know.
 */
export type TerminalChoice =
  | {
      kind: "local";
      shell: LocalShell;
      cwd: string | null;
      targetId: string | null;
      runOnConnect: string | null;
    }
  | {
      kind: "ssh";
      config: SshConfig;
      targetId: string | null;
      runOnConnect: string | null;
    };
