import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import LoadingState from "../../../../components/LoadingState";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import { CheckIcon, LockIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { Browsers } from "@mixengine/api";
import type { CaStatus } from "@mixengine/api";
import type { Trust } from "@mixengine/api";
import ElevationDialog from "../../components/ElevationDialog";
import styles from "./CaBlock.module.css";

type Translate = ReturnType<typeof useTranslation>["t"];

/** A sentence describing `trust` — four explicit branches, no dynamically built key strings. */
function trustLine(trust: Trust, t: Translate): string {
  switch (trust.state) {
    case "installed":
      return t("mixengine.domains.ca.trust.installed", { store: trust.store });
    case "not_installed":
      return t("mixengine.domains.ca.trust.notInstalled", { reason: trust.because });
    case "no_store":
      return t("mixengine.domains.ca.trust.noStore", { reason: trust.because });
    case "unknown":
      return t("mixengine.domains.ca.trust.unknown", { reason: trust.because });
  }
}

/** A sentence describing `browsers` when no branch was reached — the `reached` branch draws its own
 *  list. */
function browsersLine(browsers: Exclude<Browsers, { state: "reached" }>, t: Translate): string {
  switch (browsers.state) {
    case "no_tool":
      return t("mixengine.domains.ca.browsers.noTool", { reason: browsers.because });
    case "not_searched":
      return t("mixengine.domains.ca.browsers.notSearched", { reason: browsers.because });
    case "unknown":
      return t("mixengine.domains.ca.browsers.unknown", { reason: browsers.because });
  }
}

interface Pill {
  tone: StatusTone;
  word: string;
}

/** The system store's state in a word, for the pill beside the sentence `trustLine` writes. */
function trustPill(trust: Trust, t: Translate): Pill {
  switch (trust.state) {
    case "installed":
      return { tone: "success", word: t("mixengine.domains.ca.pill.trusted") };
    case "not_installed":
      return { tone: "danger", word: t("mixengine.domains.ca.pill.notTrusted") };
    case "no_store":
      return { tone: "neutral", word: t("mixengine.domains.ca.pill.noStore") };
    case "unknown":
      return { tone: "warning", word: t("mixengine.domains.ca.pill.unknown") };
  }
}

/** The browsers' state in a word: every database found trusts the CA, some do, or none do. */
function browsersPill(browsers: Browsers, t: Translate): Pill {
  switch (browsers.state) {
    case "reached": {
      const trusted = browsers.databases.filter((db) => db.installed).length;
      if (browsers.databases.length === 0) return { tone: "neutral", word: t("mixengine.domains.ca.pill.noneFound") };
      if (trusted === browsers.databases.length) return { tone: "success", word: t("mixengine.domains.ca.pill.trusted") };
      if (trusted === 0) return { tone: "danger", word: t("mixengine.domains.ca.pill.notTrusted") };
      return { tone: "warning", word: t("mixengine.domains.ca.pill.partly") };
    }
    case "no_tool":
    case "not_searched":
      return { tone: "neutral", word: t("mixengine.domains.ca.pill.notSearched") };
    case "unknown":
      return { tone: "warning", word: t("mixengine.domains.ca.pill.unknown") };
  }
}

/**
 * The CA's state — T2.6.
 *
 * **Two independent rows, not one combined green tick.** `trust` is the system store, `browsers`
 * is the Firefox/Chrome NSS database — a machine can hold the CA in the system store without any
 * browser knowing about it; that is a normal state, not a contradiction.
 */
export default function CaBlock({
  revision,
  onError,
}: {
  /** A change means reread — `Domains` bumps it when a job ends or when the screen is reopened. */
  revision: number;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<CaStatus | null>(null);
  /** False until the first read has answered, failed or not — a failure is the banner's to say. */
  const [loaded, setLoaded] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [pending, setPending] = useState<unknown[] | null>(null);
  const [canPrompt, setCanPrompt] = useState(true);
  const [reason, setReason] = useState<string | null | undefined>(null);

  const reload = useCallback(async () => {
    try {
      setStatus(await api.caStatus());
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [onError, t]);

  useEffect(() => {
    void reload();
  }, [reload, revision]);

  /**
   * The two-pass T64 flow: enqueue first with `grant: false`, then read `elevation.status` — if
   * something is waiting, show `ElevationDialog` for the user to see before the OS prompt comes up;
   * if nothing is (repairing the NSS database needs no rights on this machine), just reread the
   * state.
   */
  async function repair() {
    setRepairing(true);
    try {
      await api.caRepair({ grant: false });
      const queue = await api.elevationStatus();
      if (queue.pending.length > 0) {
        setCanPrompt(queue.can_prompt);
        setReason(queue.reason);
        setPending(queue.pending);
      } else {
        await reload();
      }
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setRepairing(false);
    }
  }

  if (status === null) {
    return loaded ? null : (
      <Card title={t("mixengine.domains.ca.title")}>
        <LoadingState compact />
      </Card>
    );
  }

  return (
    <Card
      title={t("mixengine.domains.ca.title")}
      count={
        <StatusPill tone={status.state === "present" ? "success" : "danger"}>
          {status.state === "present" ? t("mixengine.domains.ca.ready") : t("mixengine.domains.ca.missing")}
        </StatusPill>
      }
      description={t("mixengine.domains.ca.about")}
    >
      {status.state !== "present" && (
        <p className={styles.warning}>
          {status.state === "absent"
            ? t("mixengine.domains.ca.absent")
            : t("mixengine.domains.ca.unusable", { reason: status.because })}
        </p>
      )}

      {/* Two rows that fail independently, each with its own pill — never one tick for both. */}
      <div className={styles.facts}>
        <div className={styles.fact}>
          <span className={styles.label}>{t("mixengine.domains.ca.systemStore")}</span>
          <StatusPill tone={trustPill(status.trust, t).tone}>{trustPill(status.trust, t).word}</StatusPill>
          <span className={styles.explain}>{trustLine(status.trust, t)}</span>
        </div>

        <div className={styles.fact}>
          <span className={styles.label}>{t("mixengine.domains.ca.browsersLabel")}</span>
          <StatusPill tone={browsersPill(status.browsers, t).tone}>
            {browsersPill(status.browsers, t).word}
          </StatusPill>
          <span className={styles.explain}>
            {status.browsers.state === "reached" ? (
              <ul className={styles.databases}>
                {status.browsers.databases.map((db) => (
                  <li key={db.path}>
                    <span className={styles.owner}>{db.owner}</span>
                    {db.installed ? (
                      <CheckIcon size={14} className={styles.good} />
                    ) : (
                      <span>{db.because ?? "—"}</span>
                    )}
                    <span className={styles.path}>{db.path}</span>
                  </li>
                ))}
              </ul>
            ) : (
              browsersLine(status.browsers, t)
            )}
          </span>
          <Button
            variant="soft"
            className={styles.repair}
            onClick={() => void repair()}
            busy={repairing ? t("mixengine.domains.ca.repairing") : undefined}
          >
            <LockIcon size={14} />
            {t("mixengine.domains.ca.repair")}
          </Button>
        </div>
      </div>

      {pending && (
        <ElevationDialog
          pending={pending}
          canPrompt={canPrompt}
          reason={reason}
          onClose={() => {
            setPending(null);
            void reload();
          }}
        />
      )}
    </Card>
  );
}
