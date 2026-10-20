import { useCallback, useEffect, useState } from "react";
import Button from "../../../../components/Button";
import Checkbox from "../../../../components/Checkbox";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ErrorBanner from "../../../../components/ErrorBanner";
import NoticeBanner from "../../../../components/NoticeBanner";
import { errorMessage } from "../../../../core/errors";
import type { SyncableCollection } from "../../../../core/syncCollection";
import { useTranslation } from "../../../../i18n";
import { SYNCABLE } from "../../../registry";
import { requestSync } from "../../../sync";
import {
  syncDevices,
  syncFreezeState,
  syncLogout,
  syncRevokeDevice,
  syncThaw,
  type SyncDevice,
  type SyncFreeze,
  type SyncStatus,
} from "../../../sync/api";
import { syncedAgo, useSyncActivity } from "../../../sync/activity";
import { readEnabled, toggleRow } from "../../../sync/enabled";
import { clearReplaced, onReplacedChange, replacedCounts } from "../../../sync/replaced";
import settings from "../SettingsModal.module.css";
import ChangePassword from "./ChangePassword";
import DeleteAccount from "./DeleteAccount";
import MoveAccount from "./MoveAccount";
import styles from "./SyncSection.module.css";

interface Props {
  status: SyncStatus;
  onChanged: () => void;
}

/** Codes that mean this machine is no longer signed in, whatever the screen still shows. */
const SIGNED_OUT = new Set(["error.syncSignedOut", "error.syncNotSignedIn"]);

function codeOf(error: unknown): string | undefined {
  return typeof error === "object" && error !== null ? (error as { code?: string }).code : undefined;
}

/**
 * Signed in: who, where, what follows this person to their other machines, and which machines
 * those are. **Every row starts off** (D5), and a replaced edit is reported here and asked about
 * nowhere (D4).
 */
function AccountView({ status, onChanged }: Props) {
  const { t, lang } = useTranslation();
  const activity = useSyncActivity();
  // "2 minutes ago" goes stale on its own, so the line is redrawn every half minute.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  const [enabled, setEnabledIds] = useState(() => readEnabled(localStorage));
  const [replaced, setReplaced] = useState(replacedCounts);
  const [devices, setDevices] = useState<SyncDevice[] | null>(null);
  const [revoking, setRevoking] = useState<SyncDevice | null>(null);
  /** A secret row waiting on its question (D5). */
  const [confirming, setConfirming] = useState<SyncableCollection | null>(null);
  const [busy, setBusy] = useState(false);
  /** The one form open under the account line, if any. */
  const [panel, setPanel] = useState<"none" | "password" | "delete" | "move">("none");
  const [freeze, setFreeze] = useState<SyncFreeze | null>(null);
  const [changed, setChanged] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => onReplacedChange(() => setReplaced(replacedCounts())), []);

  const fail = useCallback(
    (error: unknown) => {
      if (SIGNED_OUT.has(codeOf(error) ?? "")) onChanged();
      else setProblem(errorMessage(t, error));
    },
    [onChanged, t],
  );

  /* `closingOn` comes from the server's capabilities, which Rust reads when it opens a session —
     and the status this view was drawn from may predate that, if nothing has synced yet this run.
     The device list opens one, so once it has answered the status is read again, once. */
  const unknownClosing = status.closingOn === null;
  const loadDevices = useCallback(() => {
    syncDevices()
      .then((list) => {
        setDevices(list);
        if (unknownClosing) onChanged();
      })
      .catch(fail);
  }, [fail, onChanged, unknownClosing]);
  useEffect(loadDevices, [loadDevices]);

  const loadFreeze = useCallback(() => {
    syncFreezeState().then(setFreeze).catch(fail);
  }, [fail]);
  useEffect(loadFreeze, [loadFreeze]);

  async function thaw() {
    try {
      setFreeze(await syncThaw());
    } catch (error) {
      fail(error);
    }
  }

  const day = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(lang);
  const labelOf = (id: string) => {
    const collection = SYNCABLE.find((candidate) => candidate.id === id);
    return collection ? t(collection.labelKey) : id;
  };

  function toggle(collection: SyncableCollection, on: boolean) {
    // A secret row asks before it goes on (D5); every other change applies at once.
    if (on && collection.belongsTo !== undefined) {
      setConfirming(collection);
      return;
    }
    apply(collection.id, on);
  }

  function apply(id: string, on: boolean) {
    setEnabledIds(toggleRow(localStorage, SYNCABLE, id, on));
    if (on) requestSync();
  }

  async function signOut() {
    setBusy(true);
    try {
      await syncLogout();
      onChanged();
    } catch (error) {
      fail(error);
    } finally {
      setBusy(false);
    }
  }

  async function revoke(device: SyncDevice) {
    setRevoking(null);
    try {
      await syncRevokeDevice(device.id);
      loadDevices();
    } catch (error) {
      fail(error);
    }
  }

  let lastSync: string | null = null;
  if (activity.lastError !== undefined) {
    lastSync = t("sync.syncFailed", { message: errorMessage(t, activity.lastError) });
  } else if (activity.lastSyncedAt !== null) {
    const ago = syncedAgo(activity.lastSyncedAt, now, lang);
    lastSync = ago === null ? t("sync.syncedJustNow") : t("sync.syncedAgo", { when: ago });
  }

  return (
    <>
      <div className={settings.section}>
        {/* Who and where on the left, one line each, and signing out on the right. The rarer
            actions sit on a row of their own below: beside the account line they squeezed it
            until the address broke mid-word. */}
        <div className={settings.updateRow}>
          <div className={styles.account}>
            <span className={`${settings.updateVersion} ${styles.oneLine}`} title={status.email ?? undefined}>
              {t("sync.signedInAs", { email: status.email ?? "" })}
            </span>
            <span className={`${settings.updateStatus} ${styles.oneLine}`} title={status.server ?? undefined}>
              {status.server}
            </span>
            {lastSync !== null && (
              <span className={`${settings.updateStatus} ${styles.oneLine}`} title={lastSync}>
                {lastSync}
              </span>
            )}
          </div>
          <div className={styles.row}>
            <Button size="small" busy={activity.direction !== null ? t("sync.syncing") : undefined} onClick={requestSync}>
              {t("sync.syncNow")}
            </Button>
            <Button size="small" busy={busy ? t("sync.signingOut") : undefined} onClick={() => void signOut()}>
              {t("sync.signOut")}
            </Button>
          </div>
        </div>
        {panel === "none" && (
          <div className={styles.actions}>
            <Button size="small" variant="ghost" onClick={() => setPanel("password")}>
              {t("sync.changePassword")}
            </Button>
            <Button size="small" variant="ghost" onClick={() => setPanel("move")}>
              {t("sync.moveTitle")}
            </Button>
            <Button size="small" variant="ghost" onClick={() => setPanel("delete")}>
              {t("sync.deleteAccount")}
            </Button>
          </div>
        )}
        <p className={settings.hint}>{t("sync.signOutHint")}</p>
        {changed && <NoticeBanner message={t("sync.passwordChanged")} onDismiss={() => setChanged(false)} />}
        {status.closingOn !== null && <NoticeBanner message={t("sync.closing", { date: day(status.closingOn) })} />}
        {freeze?.state === "frozen" && panel !== "move" && (
          <div className={styles.row}>
            <NoticeBanner
              message={t("sync.frozen", { date: freeze.frozenAt === null ? "" : day(freeze.frozenAt) })}
            />
            <Button size="small" onClick={() => void thaw()}>
              {t("sync.thaw")}
            </Button>
          </div>
        )}
        {[...replaced].map(([id, count]) => (
          <NoticeBanner
            key={id}
            message={t("sync.replaced", { count, collection: labelOf(id) })}
            onDismiss={() => clearReplaced(id)}
          />
        ))}
        {problem && <ErrorBanner message={problem} onDismiss={() => setProblem(null)} />}
      </div>

      {panel === "password" && (
        <ChangePassword
          onDone={() => {
            setPanel("none");
            setChanged(true);
            loadDevices();
          }}
          onCancel={() => setPanel("none")}
        />
      )}
      {panel === "delete" && status.server && (
        <DeleteAccount server={status.server} onDeleted={onChanged} onCancel={() => setPanel("none")} />
      )}
      {panel === "move" && status.server && (
        <MoveAccount
          from={status.server}
          deviceName={devices?.find((device) => device.current)?.name ?? "MixLab"}
          onMoved={() => {
            onChanged();
            // The new account's store starts empty; a full run pulls it before anything is
            // pushed, and re-reads the new server's closing date (`useSyncClosing`) on the way.
            requestSync();
          }}
          onCancel={() => {
            setPanel("none");
            loadFreeze();
          }}
        />
      )}

      <div className={settings.section}>
        <span className={settings.sectionLabel}>{t("sync.collections")}</span>
        {SYNCABLE.map((collection) => {
          const owner = collection.belongsTo;
          const blocked = owner !== undefined && !enabled.has(owner);
          return (
            <Checkbox
              key={collection.id}
              className={owner === undefined ? settings.moduleRow : `${settings.moduleRow} ${styles.secretRow}`}
              label={t(collection.labelKey)}
              checked={enabled.has(collection.id)}
              disabled={blocked}
              title={
                blocked && owner !== undefined ? t("sync.secretNeedsOwner", { collection: labelOf(owner) }) : undefined
              }
              onChange={(event) => toggle(collection, event.target.checked)}
            />
          );
        })}
        <p className={settings.hint}>{t("sync.collectionsHint")}</p>
      </div>

      <div className={settings.section}>
        <span className={settings.sectionLabel}>{t("sync.devices")}</span>
        {devices?.map((device) => (
          <div key={device.id} className={styles.device}>
            <div className={settings.updateText}>
              <span>
                {device.name}
                {device.current && <span className={styles.tag}>{t("sync.thisDevice")}</span>}
              </span>
              <span className={settings.updateStatus}>{t("sync.lastSeen", { when: day(device.lastSeenAt) })}</span>
            </div>
            {/* This machine leaves by signing out, above, which also forgets it here. */}
            {!device.current && (
              <Button size="small" variant="ghost" onClick={() => setRevoking(device)}>
                {t("sync.revoke")}
              </Button>
            )}
          </div>
        ))}
      </div>

      {confirming && (
        <ConfirmDialog
          title={t("sync.secretConfirmTitle", { collection: t(confirming.labelKey) })}
          message={t("sync.secretConfirmMessage")}
          confirmLabel={t("sync.secretConfirmAction")}
          onConfirm={() => {
            apply(confirming.id, true);
            setConfirming(null);
          }}
          onCancel={() => setConfirming(null)}
        />
      )}
      {revoking && (
        <ConfirmDialog
          title={t("sync.revokeTitle", { name: revoking.name })}
          message={t("sync.revokeMessage")}
          confirmLabel={t("sync.revoke")}
          danger
          onConfirm={() => void revoke(revoking)}
          onCancel={() => setRevoking(null)}
        />
      )}
    </>
  );
}

export default AccountView;
