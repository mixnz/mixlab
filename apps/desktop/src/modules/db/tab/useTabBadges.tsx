import { useEffect, useEffectEvent, useMemo } from "react";
import { LockIcon } from "../../../icons";
import { useTranslation } from "../../../i18n";
import type { TabBadge } from "../../../shell/module";
import { dbBadgeMarks } from "../badges";
import { kindLabel } from "../connectionForm";
import { DatabaseIcon } from "../icons";
import type { DbKind } from "../types";

interface Options {
  connectionId: string | null;
  kind: DbKind;
  /** Whether the connection behind the tab is marked read-only — `DbTab`'s `activeReadOnly`. */
  readOnly: boolean;
  onBadgesChange: (badges: TabBadge[]) => void;
}

export function useTabBadges({ connectionId, kind, readOnly, onBadgesChange }: Options) {
  const { t } = useTranslation();
  /**
   * The marks this tab asks the tab bar for: the engine's logo, and the lock when the connection
   * behind it is read-only.
   *
   * The shell is told what to draw rather than what this tab is — it has no `DbKind` and no notion
   * of read-only. Which marks a given state calls for is {@link dbBadgeMarks}; turning them into
   * icons and sentences is here, because this is the side that has a `t`.
   */
  const badges = useMemo<TabBadge[]>(
    () =>
      dbBadgeMarks(connectionId ? kind : undefined, readOnly).map((mark) =>
        mark.type === "kind"
          ? {
              id: "kind",
              // The same logo the sidebar row carries, without the tinted tile around it: a tab has
              // no room for a badge, and the mark alone is what has to be recognised.
              icon: <DatabaseIcon kind={mark.kind} size={14} />,
              label: t(kindLabel(mark.kind)),
              className: `tab-kind kind-${mark.kind}`,
            }
          : {
              id: "readOnly",
              icon: <LockIcon size={12} />,
              label: t("common.readOnly"),
              title: t("common.readOnlyConnection"),
              className: "tab-lock",
              // The bar along the top of the open tab goes amber, which is the one thing on screen
              // that says so no matter which pane of the workspace is showing.
              tabClassName: "tab-readonly",
            },
      ),
    [connectionId, kind, readOnly, t],
  );
  // An effect event: the shell hands down a fresh `onBadgesChange` every render, and depending on
  // it would send the badges up again on every one of them.
  const reportBadges = useEffectEvent((next: TabBadge[]) => onBadgesChange(next));
  useEffect(() => {
    reportBadges(badges);
  }, [badges]);
}
