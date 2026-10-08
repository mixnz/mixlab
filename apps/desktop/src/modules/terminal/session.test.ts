import { describe, expect, it } from "vitest";
import {
  openingFor,
  openingKeystrokes,
  terminalBadgeMarks,
  terminalTarget,
  terminalTitle,
} from "./session";
import type { SshConfig, TerminalChoice } from "./types";

const bash: TerminalChoice = {
  kind: "local",
  shell: { name: "git-bash", path: "C:\\Program Files\\Git\\bin\\bash.exe", args: [] },
  cwd: null,
  targetId: null,
  runOnConnect: null,
  press: true,
  env: null,
  pathPrepend: null,
};

const ubuntu: TerminalChoice = {
  kind: "local",
  shell: { name: "wsl:Ubuntu", path: "C:\\Windows\\System32\\wsl.exe", args: ["-d", "Ubuntu"] },
  cwd: "D:\\work",
  targetId: null,
  runOnConnect: null,
  press: true,
  env: null,
  pathPrepend: null,
};

const config: SshConfig = {
  host: "example.com",
  port: 22,
  username: "deploy",
  auth: { type: "password", password: "hunter2" },
};

const remote: TerminalChoice = { kind: "ssh", config, targetId: null, runOnConnect: null, press: true };

describe("terminalTarget", () => {
  it("sends the path and the args, not the display name", () => {
    expect(terminalTarget(bash)).toEqual({
      type: "local",
      shell: "C:\\Program Files\\Git\\bin\\bash.exe",
      args: [],
      cwd: null,
    });
  });

  /* T205, D9: what a saved local target sets in its shell reaches Rust as it was saved. */
  it("carries a local target's variables and PATH prefix", () => {
    expect(
      terminalTarget({ ...bash, env: { MIXENGINE_HOME: "C:\\h" }, pathPrepend: ["C:\\h\\bin"] }),
    ).toMatchObject({ env: { MIXENGINE_HOME: "C:\\h" }, pathPrepend: ["C:\\h\\bin"] });
  });

  it("carries a WSL distribution through as arguments", () => {
    expect(terminalTarget(ubuntu)).toEqual({
      type: "local",
      shell: "C:\\Windows\\System32\\wsl.exe",
      args: ["-d", "Ubuntu"],
      cwd: "D:\\work",
    });
  });

  /* `Ssh(SshConfig)` on the Rust side is a newtype variant of an internally tagged enum, so its
     four fields sit flat beside `type` rather than nested under a key of their own. Getting this
     wrong makes serde refuse the payload at runtime, where nothing at build time would say so. */
  it("flattens the SSH config beside the target tag", () => {
    expect(terminalTarget(remote)).toEqual({
      type: "ssh",
      host: "example.com",
      port: 22,
      username: "deploy",
      auth: { type: "password", password: "hunter2" },
    });
  });
});

describe("terminalTitle", () => {
  it("names the tab after the shell, not after its path", () => {
    expect(terminalTitle(bash, null)).toBe("Git Bash");
    expect(terminalTitle(ubuntu, null)).toBe("WSL: Ubuntu");
  });

  /* `user@host`, not the saved host's name: a tab is a few characters wide, and what has to be
     readable there is which machine the keystrokes are going to. That is what the tab is called
     unless the user asks for the other one — see the setting below. */
  it("names an SSH session after user@host", () => {
    expect(terminalTitle(remote, null)).toBe("deploy@example.com");
  });

  /* The saved name beats both, and beats them for both kinds of session: someone who named a server
     "Prod DB" wants to read "Prod DB" on the tab, not read back what they typed into the form. */
  it("names the tab after the saved target when one is given", () => {
    expect(terminalTitle(remote, "Prod DB")).toBe("Prod DB");
    expect(terminalTitle(bash, "Dự án A")).toBe("Dự án A");
  });

  /* Nothing to show changes nothing. An empty name is an entry not yet named, not a request for the
     tab to have no name. */
  it("falls back when the saved name is empty", () => {
    expect(terminalTitle(remote, "")).toBe("deploy@example.com");
    expect(terminalTitle(remote, "   ")).toBe("deploy@example.com");
  });
});

describe("terminalBadgeMarks", () => {
  // With no session yet the form is showing, and the form may be for a different shell than the
  // one the tab will open.
  it("marks nothing while the tab is still on the form", () => {
    expect(terminalBadgeMarks(null, false)).toEqual([]);
  });

  it("marks the session while it is running", () => {
    expect(terminalBadgeMarks(bash, false)).toEqual([{ type: "local" }]);
  });

  it("tells an SSH session apart from a local one", () => {
    expect(terminalBadgeMarks(remote, false)).toEqual([{ type: "ssh" }]);
  });

  it("puts the ended mark after the kind, never before it", () => {
    expect(terminalBadgeMarks(remote, true)).toEqual([{ type: "ssh" }, { type: "ended" }]);
  });
});

describe("openingKeystrokes", () => {
  it("ends the line, because a command nobody pressed Enter on never runs", () => {
    expect(openingKeystrokes("cd ~/project-a/frontend")).toBe("cd ~/project-a/frontend\r");
  });

  /* Several lines are several commands, and the user types them into a multi-line field because
     they want each line to run — `\r` rather than `\n`: the pty reads the Enter key, not a newline
     character. */
  it("runs every line, however the box wrote its newlines", () => {
    expect(openingKeystrokes("cd ~/a\nnvm use\r\nnpm run dev")).toBe(
      "cd ~/a\rnvm use\rnpm run dev\r",
    );
  });

  it("drops blank lines and the spaces around each one", () => {
    expect(openingKeystrokes("  cd ~/a  \n\n\n  ls  \n")).toBe("cd ~/a\rls\r");
  });

  /* An empty field is perfectly normal, not an empty command to send down. */
  it("has nothing to send for an empty box", () => {
    expect(openingKeystrokes(null)).toBeNull();
    expect(openingKeystrokes("")).toBeNull();
    expect(openingKeystrokes("   \n\n  ")).toBeNull();
  });

  /* T205, D10: a restored tab or an untrusted project's step is typed and left for the person to
     run. Earlier lines still run; only the last one waits. */
  it("types without Enter when asked not to press it", () => {
    expect(openingKeystrokes("npm run dev", false)).toBe("npm run dev");
    expect(openingKeystrokes("a\nb", false)).toBe("a\rb");
  });
});

/* Measured by hand: *Just open* and *Type them* still ran the commands when the target was opened
   from the list, because the choice was only read when MixLab restored a tab. It is the choice for
   every opening of the target. */
describe("openingFor", () => {
  it("runs the lines for run, types them for type, and sends nothing for none", () => {
    expect(openingFor("npm run dev", "run")).toEqual({ runOnConnect: "npm run dev", press: true });
    expect(openingFor("npm run dev", "type")).toEqual({ runOnConnect: "npm run dev", press: false });
    expect(openingFor("npm run dev", "none")).toEqual({ runOnConnect: null, press: false });
  });

  it("reads an absent choice as run, and an empty box as nothing to send", () => {
    expect(openingFor("ls", undefined)).toEqual({ runOnConnect: "ls", press: true });
    expect(openingFor(undefined, "run")).toEqual({ runOnConnect: null, press: true });
  });
});
