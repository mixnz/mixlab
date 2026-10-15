import { PowerIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";

interface Props {
  /** Closes the connection and puts the form back. `useConnection`'s `disconnect` is what this reaches. */
  onDisconnect: () => void;
}

/**
 * The way out of a connection, at the end of the row of tabs that only exists while there is one.
 *
 * One component for all three workspaces rather than a button written into each: the three strips
 * hold different tabs — Query is a SQL thing, the group pane a Redis one — but the way out of a
 * connection is the same act with the same consequence wherever it is pressed, and three copies of
 * it is three chances for one of them to drift.
 *
 * At the end of the tabs because that is where a row of tabs is finished with, and because
 * anywhere earlier would put it in the way of the panes it closes.
 *
 * Not a tab, and it does not pretend to be one: the tabs pick which pane is on screen, this takes
 * the whole workspace away and leaves the connection form behind. It says so with a rule down its
 * left, square corners where the tabs are rounded, and an icon none of them carry — and then stays
 * as quiet as they are until the pointer is on it, which is where the red belongs. See
 * `.method-tab-disconnect` in `db.css`.
 *
 * The tab bar above does not change: the connection this closes is still the one the form holds
 * and the one the sidebar has marked, so pressing this and pressing Connect again is a round trip
 * back to where it started.
 */
function DisconnectTab({ onDisconnect }: Props) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      className="method-tab method-tab-disconnect"
      title={t("connection.disconnectHint")}
      onClick={onDisconnect}
    >
      <PowerIcon size="1em" />
      {t("common.disconnect")}
    </button>
  );
}

export default DisconnectTab;
