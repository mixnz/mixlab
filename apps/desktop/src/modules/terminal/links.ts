/**
 * What on the terminal screen may be opened outside, and what may not.
 *
 * Every string passing through here was printed by a server. Handing it to the operating system's
 * opener hands that server a way to launch something on this machine, so only two schemes get
 * through: `http` and `https`. `file:`, `data:` and every scheme registered by an installed
 * application — `vscode:`, `ms-msdt:` — stop here, whether or not the addon's regex picks them up.
 */

/** The address as written, if it can be opened; `null` for anything else. */
export function openableUrl(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return null;
  }
  // `URL` has already lowercased the scheme; what is returned is the original string, because that
  // is what the user sees.
  return url.protocol === "http:" || url.protocol === "https:" ? text : null;
}
