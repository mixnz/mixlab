/**
 * What version this is, and where a release can be downloaded by hand.
 *
 * MixLab updates itself (ADR 0056): the updater is `src/shell/update/` and
 * `src-tauri/src/updater/`. What is left here is the running version and the release page, for a
 * copy the updater cannot replace (a development build, or one something else installed).
 */

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";

const REPO = "mixnz/mixlab";

/** Where a user is sent when the automatic path is not available to them — a `.deb` install, a
 *  locked-down machine, a copy something else installed. */
export const RELEASES_PAGE = `https://github.com/${REPO}/releases/latest`;

/** Opens the releases page in the user's browser. */
export function openReleasesPage(): Promise<void> {
  return openUrl(RELEASES_PAGE);
}

/**
 * The running version, empty until the bundle has answered.
 *
 * Taken from the bundle rather than from `package.json` so it is the one the installed app actually
 * reports — which is the workspace's.
 */
export function useAppVersion(): string {
  const [version, setVersion] = useState("");

  useEffect(() => {
    let alive = true;
    void getVersion().then((answer) => {
      if (alive) setVersion(answer);
    });
    return () => {
      alive = false;
    };
  }, []);

  return version;
}
