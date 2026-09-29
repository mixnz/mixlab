/** The prefix Rust uses for a WSL distribution: `wsl:Ubuntu`. */
const WSL_PREFIX = "wsl:";

/** Proper names, so not translated — what gets translated is the picker's label, not its
 *  contents. */
const LABELS: Record<string, string> = {
  powershell: "Windows PowerShell",
  pwsh: "PowerShell 7",
  cmd: "Command Prompt",
  "git-bash": "Git Bash",
};

/** The display label for a detected shell. An unknown name returns itself: this table is for
 *  prettifying, not for filtering. */
export function shellLabel(name: string): string {
  if (name.startsWith(WSL_PREFIX)) return `WSL: ${name.slice(WSL_PREFIX.length)}`;
  return LABELS[name] ?? name;
}

/** The brands that can be drawn for a shell — see `icons.tsx`. */
export type ShellBrand = "powershell" | "git" | "bash" | "zsh" | "fish" | "linux";

/**
 * Which logo stands in front of a shell's name, or no logo at all.
 *
 * Split from `icons.tsx` because this is the part worth testing: that other file is just path data.
 * `null` is a real answer rather than an omission — `cmd` and `sh` have no logo, and a generic
 * terminal icon is the right answer for them.
 */
export function shellBrand(name: string): ShellBrand | null {
  // Every distribution is Linux; the penguin states the only thing certainly true here.
  if (name.startsWith(WSL_PREFIX)) return "linux";
  switch (name) {
    case "powershell":
    case "pwsh":
      return "powershell";
    // Git Bash is bash, but what sets it apart from WSL's or macOS's bash is Git.
    case "git-bash":
      return "git";
    case "bash":
      return "bash";
    case "zsh":
      return "zsh";
    case "fish":
      return "fish";
    default:
      return null;
  }
}
