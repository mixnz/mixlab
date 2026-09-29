import { useCallback, useState } from "react";
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
 */
export function useWorkspaceError(tunnelled: boolean): [string, (message: string) => void] {
  const { t } = useTranslation();
  const [error, setError] = useState("");
  const lostMessage = tunnelled ? t("error.connectionLost") : null;
  const report = useCallback(
    (message: string) => {
      if (reachesErrorBanner(message, lostMessage)) setError(message);
    },
    [lostMessage]
  );
  return [error, report];
}
