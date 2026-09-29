import { useEffect, useState, type ReactNode } from "react";

import PageHeader from "../../../../components/PageHeader";
import SegmentedControl, { type Segment } from "../../../../components/SegmentedControl";
import { DatabaseGenericIcon, EngineIcon, GlobeIcon, PackageIcon, PulseIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { peekPendingLanguageFilter } from "../../packagesNavigation";
import Languages from "./Languages";
import PackageList from "./PackageList";
import { PACKAGE_CATEGORY_ORDER, packageCategory, type PackageCategory } from "./packageCategories";
import { usePackages } from "./usePackages";
import styles from "./Packages.module.css";

type TabKey = "languages" | PackageCategory;

/**
 * One sidebar item, one tab strip — not two sidebar entries (D5), and not two levels of tabs:
 * Languages stands level with each package group, not level with a "Software" tab that has to be
 * opened before the groups show. `runtime.*` and `package.*` share the same RPC shape and the same
 * job shape, differing only in the namespace called and in the `force` capability.
 */
export default function Packages({ active }: { active: boolean }) {
  const [tab, setTab] = useState<TabKey>("languages");
  const { t } = useTranslation();

  // `package.*`'s state lives here, not in each group — see `usePackages`. Read even while on the
  // Languages tab: the tab strip below needs to know whether any package falls into "Other".
  const packages = usePackages(active);

  // Somebody sent the user here to install a runtime — "Install a PHP" on the PHP extensions
  // screen, via `packagesNavigation.ts`. This screen stays mounted between visits and keeps
  // whichever tab was last open, so a visit that arrives with a request pending has to be put back
  // on Languages; `Languages` itself takes the request and fills its search box.
  //
  // Peek, never take: consuming the token here would leave the search box empty. Running only on
  // an `active` edge is what keeps it from fighting the user — once they are on this screen the
  // token is already gone, and a manual switch to another tab stays switched.
  useEffect(() => {
    if (active && peekPendingLanguageFilter() !== null) setTab("languages");
  }, [active]);

  // Switching tabs must not unmount Languages: a job installing there still has to be followed
  // (its local `installingJob`/`jobs`) when the user visits a package group and comes back — see
  // `MixEngineTab.tsx`, which follows the same rule for the sidebar screens. The package groups do
  // not need to stay mounted: their state is already in `usePackages`, above the tabs.
  const [languagesMounted, setLanguagesMounted] = useState(tab === "languages");
  useEffect(() => {
    if (tab === "languages") setLanguagesMounted(true);
  }, [tab]);

  // The first three groups are always present — a tab's position must not jump just because the
  // user removed the last version in that group. "Other" is the opposite: it is not a group users
  // recognise, just a catch-all for packages from a registry newer than the running version (see
  // `packageCategories.ts`), so it is only drawn when something actually falls into it.
  const hasOther =
    packages.installed.some((row) => packageCategory(row.package) === "other") ||
    packages.available.some((release) => packageCategory(release.package) === "other");
  const categoryTabs = PACKAGE_CATEGORY_ORDER.filter((cat) => cat !== "other" || hasOther);

  // The last "Other" package just vanished while that tab was showing: go back to Languages
  // instead of keeping a tab that is no longer on the strip.
  useEffect(() => {
    if (tab === "other" && !hasOther) setTab("languages");
  }, [tab, hasOther]);

  const categoryLabel: Record<PackageCategory, string> = {
    web: t("mixengine.packages.categoryWeb"),
    database: t("mixengine.packages.categoryDatabase"),
    cache: t("mixengine.packages.categoryCache"),
    other: t("mixengine.packages.categoryOther"),
  };

  const tabIcon: Record<TabKey, ReactNode> = {
    languages: <EngineIcon size={15} />,
    web: <GlobeIcon size={15} />,
    database: <DatabaseGenericIcon size={15} />,
    cache: <PulseIcon size={15} />,
    other: <PackageIcon size={15} />,
  };

  const tabs: Segment<TabKey>[] = [
    { value: "languages", label: t("mixengine.packages.tabLanguages"), icon: tabIcon.languages },
    ...categoryTabs.map((cat) => ({ value: cat as TabKey, label: categoryLabel[cat], icon: tabIcon[cat] })),
  ];

  return (
    <div className={`mixengine-page ${styles.packages}`}>
      <PageHeader title={t("mixengine.sidebar.packages")} description={t("mixengine.packages.about")} />
      <div className={styles.tabs}>
        <SegmentedControl
          mode="tabs"
          aria-label={t("mixengine.sidebar.packages")}
          segments={tabs}
          value={tab}
          onChange={setTab}
        />
      </div>      {languagesMounted && (
        <div className={styles.pane} hidden={tab !== "languages"}>
          <Languages active={active && tab === "languages"} />
        </div>
      )}
      {tab !== "languages" && (
        // `key` follows the group: `PackageList`'s search box is local state, and a search typed
        // for Web servers must not follow along into Databases.
        <div className={styles.pane}>
          <PackageList key={tab} category={tab} state={packages} />
        </div>
      )}
    </div>
  );
}
