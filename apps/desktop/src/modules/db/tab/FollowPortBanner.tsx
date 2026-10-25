import { useState } from "react";

import Button from "../../../components/Button";
import NoticeBanner from "../../../components/NoticeBanner";
import { useTranslation } from "../../../i18n";
import { connectionsToFollow, followed } from "../saved/followPort";
import { updateConnection, useSavedConnections, useSavedConnectionsLoaded } from "../saved/savedConnectionsStore";
import type { ServiceMovedTabState } from "../tabState";

interface Props {
  moved: ServiceMovedTabState["serviceMoved"];
  onDone: () => void;
}

/**
 * The question a tab opens with after MixEngine moved one of its services to another port: the
 * saved connections made for that service still name the old one. Asked once, answered once, and
 * nothing changes unless the user says so. A connection typed by hand is never in the list.
 */
export default function FollowPortBanner({ moved, onDone }: Props) {
  const { t } = useTranslation();
  const saved = useSavedConnections();
  const loaded = useSavedConnectionsLoaded();
  const [updated, setUpdated] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  if (!loaded) return null;

  const { service, from, to } = moved;
  const stale = connectionsToFollow(saved, service, from);

  if (updated !== null || stale.length === 0) {
    const message =
      updated !== null
        ? t("followPort.updated", { count: String(updated), to: String(to) })
        : t("followPort.none", { service, from: String(from) });
    return (
      <div className="saved-elsewhere">
        <NoticeBanner message={message} />
        <Button size="small" onClick={onDone}>
          {t("followPort.ok")}
        </Button>
      </div>
    );
  }

  async function update() {
    setBusy(true);
    try {
      for (const entry of stale) await updateConnection(followed(entry, to));
      setUpdated(stale.length);
    } finally {
      setBusy(false);
    }
  }

  const names = stale.map((entry) => entry.name).join(", ");
  const question =
    stale.length === 1
      ? t("followPort.foundOne", { service, from: String(from), to: String(to), names })
      : t("followPort.found", { count: String(stale.length), service, from: String(from), to: String(to), names });

  return (
    <div className="saved-elsewhere">
      <NoticeBanner message={question} />
      <Button size="small" variant="primary" onClick={() => void update()} busy={busy ? t("followPort.update") : undefined}>
        {t("followPort.update")}
      </Button>
      <Button size="small" onClick={onDone} disabled={busy}>
        {t("followPort.leave")}
      </Button>
    </div>
  );
}
