import { useCallback, useRef, useState } from "react";
import { useTranslation } from "../../i18n";

/**
 * Whether this sentence may fall through to ErrorBanner.
 *
 * `lostMessage` is the "connection lost" sentence in the active language, or `null` for a
 * connection not going through a tunnel — nothing is swallowed there.
 */
export function reachesErrorBanner(message: string, lostMessage: string | null): boolean {
  return message !== lostMessage;
}

/**
 * A workspace's ErrorBanner line, in place of `useState("")`.
 *
 * It wraps `useState` for one reason only: when the connection goes through an SSH tunnel,
 * "connection lost" must not fall through to here. TunnelBanner is telling exactly that story and
 * telling it better — it says the tunnel is being reopened, disappears once it is, and has a retry
 * button when it is not. ErrorBanner would only leave a dead sentence the user has to dismiss by
 * hand, sitting right under the one saying everything has healed.
 *
 * A direct connection swallows nothing: there is no TunnelBanner there to speak instead, and
 * swallowing would leave only a silent action where nothing happens.
 *
 * Returns exactly the shape of `useState`, so `setError("")` to dismiss the banner still works as
 * before: only that one sentence is swallowed, and the empty string is not that sentence.
 *
 * The setter is one function for the life of the workspace, as `useState`'s is. It asks which
 * sentence to swallow at the moment it is called rather than when it was made: a setter rebuilt
 * whenever the language or the tunnel changed had to be listed in every effect that reports an
 * error, and listing it would re-run them — re-reading a collection list and dropping the
 * selection — for no more than a change of language.
 */
export function useWorkspaceError(tunnelled: boolean): [string, (message: string) => void] {
  const { t } = useTranslation();
  const [error, setError] = useState("");
  const tunnelledRef = useRef(tunnelled);
  tunnelledRef.current = tunnelled;
  const report = useCallback(
    (message: string) => {
      const lostMessage = tunnelledRef.current ? t("error.connectionLost") : null;
      if (reachesErrorBanner(message, lostMessage)) setError(message);
    },
    [t]
  );
  return [error, report];
}
