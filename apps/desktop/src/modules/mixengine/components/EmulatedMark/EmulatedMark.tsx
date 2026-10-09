import type { Execution } from "@mixengine/api";
import StatusPill from "../../../../components/StatusPill";
import { useTranslation } from "../../../../i18n";
import { isEmulated } from "./emulated";

/**
 * "emulated" on a release row, with the reason as its tooltip — what `mix runtime available` and
 * `mix package available` say in their RUNS column (ADR 0023). Neutral, because an emulated build
 * works: it is slower, not broken. Nothing is drawn for a native row, so five of the six targets
 * never see it.
 */
export default function EmulatedMark({
  release,
  className,
}: {
  release: { execution?: Execution | null };
  className?: string;
}) {
  const { t } = useTranslation();
  if (!isEmulated(release)) return null;
  return (
    <StatusPill tone="neutral" className={className} title={t("mixengine.packages.emulated.reason")}>
      {t("mixengine.packages.emulated.mark")}
    </StatusPill>
  );
}
