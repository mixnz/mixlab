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
 * Một sidebar item, một dải tab — không hai mục sidebar (D5), và không hai tầng tab: Ngôn ngữ
 * đứng ngang hàng với từng nhóm package, không phải ngang hàng với một tab "Phần mềm" còn phải
 * mở ra mới thấy nhóm. `runtime.*` và `package.*` cùng hình dạng RPC và cùng hình dạng job, khác
 * đúng namespace gọi và đúng khả năng `force`.
 */
export default function Packages({ active }: { active: boolean }) {
  const [tab, setTab] = useState<TabKey>("languages");
  const { t } = useTranslation();

  // State của `package.*` sống ở đây, không trong từng nhóm — xem `usePackages`. Đọc kể cả khi
  // đang ở tab Ngôn ngữ: dải tab dưới đây cần biết có package nào rơi vào nhóm "Khác" không.
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

  // Đổi tab không được unmount Ngôn ngữ: một job đang cài ở đó vẫn phải còn được theo dõi
  // (`installingJob`/`jobs` cục bộ của nó) khi người dùng ghé qua một nhóm package rồi quay lại —
  // xem `MixEngineTab.tsx`, chỗ đã theo cùng luật này cho các màn sidebar. Các nhóm package thì
  // không cần giữ mount: state của chúng đã ở `usePackages`, cao hơn tab.
  const [languagesMounted, setLanguagesMounted] = useState(tab === "languages");
  useEffect(() => {
    if (tab === "languages") setLanguagesMounted(true);
  }, [tab]);

  // Ba nhóm đầu luôn có mặt — vị trí một tab không được nhảy chỉ vì người dùng vừa gỡ bản cuối
  // cùng trong nhóm đó. "Khác" thì ngược lại: nó không phải một nhóm người dùng nhận ra, chỉ là
  // chốt cho package registry mới hơn bản đang chạy (xem `packageCategories.ts`), nên chỉ vẽ khi
  // thật sự có gì rơi vào đó.
  const hasOther =
    packages.installed.some((row) => packageCategory(row.package) === "other") ||
    packages.available.some((release) => packageCategory(release.package) === "other");
  const categoryTabs = PACKAGE_CATEGORY_ORDER.filter((cat) => cat !== "other" || hasOther);

  // Package "Khác" cuối cùng vừa biến mất trong lúc đang đứng ở tab đó: quay về Ngôn ngữ thay vì
  // giữ một tab không còn trên dải.
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
        // `key` gắn theo nhóm: ô tìm của `PackageList` là state cục bộ, và một câu tìm gõ cho Máy chủ
        // web không được đi theo sang Cơ sở dữ liệu.
        <div className={styles.pane}>
          <PackageList key={tab} category={tab} state={packages} />
        </div>
      )}
    </div>
  );
}
