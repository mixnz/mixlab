import type { CatalogueGap } from "@mixengine/api";
import StatusPill from "../../../../components/StatusPill";
import { useTranslation } from "../../../../i18n";
import { describeGaps } from "./gaps";

/**
 * "Missing from this list: node" — for `RuntimeCatalogue.unavailable` and
 * `PackageCatalogue.unavailable` (T196). Drawn beside `StaleBadge` for its reason: a list with a
 * kind missing and no word about it reads as an index that publishes nothing for that kind.
 */
export default function UnavailableNote({ gaps }: { gaps: CatalogueGap[] | null | undefined }) {
  const { t } = useTranslation();
  const said = describeGaps(gaps);
  if (!said) return null;
  return (
    <StatusPill tone="warning" title={said.title}>
      {t("mixengine.packages.unavailable", { names: said.names })}
    </StatusPill>
  );
}
