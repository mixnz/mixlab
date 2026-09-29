import type { ComponentType } from "react";
import {
  AddOnIcon,
  BlueprintIcon,
  DashboardIcon,
  FolderIcon,
  GlobeIcon,
  LockIcon,
  LogIcon,
  PackageIcon,
  PulseIcon,
  PuzzleIcon,
  ServerIcon,
  SlidersIcon,
  type IconProps,
} from "../../../../icons";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import type { MixEngineScreen } from "../../tabState";
import styles from "./Sidebar.module.css";

/**
 * Mười hai màn hình, năm nhóm — T119, quyết định D11.
 *
 * **Thứ tự theo việc người ta làm, không theo hình dạng của API.** Mười một mục phẳng là thứ phải
 * đọc hết mới tìm được, và hai trong số đó từng tên gần giống nhau — T118 đổi nhãn *Extensions*
 * thành *Add-ons* để PHP Extensions có chỗ đứng tên nó.
 *
 * **Tiêu đề tĩnh, không gập lại được.** Một nhóm gập được là một chỗ để đúng thứ người ta đang tìm
 * trốn vào, và trạng thái gập là thứ phải lưu, migrate rồi làm sai. Danh sách có mười hai mục; nó
 * vừa màn hình.
 *
 * **`projects` và `metrics` không nằm trong 9 màn hình `client-surface.md` liệt kê**, và lý do giữ
 * nguyên như khi bảng này còn phẳng: `client-surface.md` giả định một client hỏi `project.list` cho
 * đúng một dropdown, trong khi `project.*` đã đủ method (`list, create, show, update, delete,
 * export`) cho một màn quản lý — và Sites không dùng được nếu chưa có project nào (Quyết định D4,
 * `docs/specs/2026-09-06-mixengine-runtimes-services-logs-design.md`). `metrics` thì là
 * dữ liệu khác hình dạng: biểu đồ 24 giờ, không phải một hàng trong bảng service (D1,
 * `docs/specs/2026-09-07-mixengine-metrics-settings-design.md`).
 *
 * **`phpExtensions` cũng không nằm trong đó** — `client-surface.md` đặt công tắc extension bên
 * trong màn Runtimes, và nó vẫn ở đó: cùng một component, vẽ ở hai nơi (T118). Nhóm *Môi trường* là
 * chỗ thứ hai, vì đó là nơi người ta đi tìm nó.
 *
 * Bảng này vẫn là chỗ **duy nhất** quyết định thứ tự, đúng như khi nó còn phẳng.
 */
const GROUPS: readonly {
  labelKey: TranslationKey | null;
  items: readonly { screen: MixEngineScreen; labelKey: TranslationKey; Icon: ComponentType<IconProps> }[];
}[] = [
  {
    labelKey: "mixengine.sidebar.groupOverview",
    items: [
      { screen: "dashboard", labelKey: "mixengine.sidebar.dashboard", Icon: DashboardIcon },
      { screen: "metrics", labelKey: "mixengine.sidebar.metrics", Icon: PulseIcon },
      { screen: "logs", labelKey: "mixengine.sidebar.logs", Icon: LogIcon },
    ],
  },
  {
    labelKey: "mixengine.sidebar.groupWebsites",
    items: [
      { screen: "projects", labelKey: "mixengine.sidebar.projects", Icon: FolderIcon },
      { screen: "sites", labelKey: "mixengine.sidebar.sites", Icon: GlobeIcon },
      { screen: "domains", labelKey: "mixengine.sidebar.domains", Icon: LockIcon },
    ],
  },
  {
    labelKey: "mixengine.sidebar.groupEnvironment",
    items: [
      { screen: "packages", labelKey: "mixengine.sidebar.packages", Icon: PackageIcon },
      { screen: "phpExtensions", labelKey: "mixengine.sidebar.phpExtensions", Icon: PuzzleIcon },
      { screen: "servicesDetail", labelKey: "mixengine.sidebar.servicesDetail", Icon: ServerIcon },
    ],
  },
  {
    labelKey: "mixengine.sidebar.groupLibrary",
    items: [
      { screen: "blueprints", labelKey: "mixengine.sidebar.blueprints", Icon: BlueprintIcon },
      { screen: "extensions", labelKey: "mixengine.sidebar.extensions", Icon: AddOnIcon },
    ],
  },
  // Không tiêu đề, và `margin-top: auto` trong CSS đẩy nó xuống đáy: Settings không thuộc nhóm nào
  // và là mục người ta tìm ở chỗ nó vẫn luôn ở.
  {
    labelKey: null,
    items: [{ screen: "settings", labelKey: "mixengine.sidebar.settings", Icon: SlidersIcon }],
  },
];

export default function Sidebar({
  screen,
  onSelect,
}: {
  screen: MixEngineScreen;
  onSelect: (screen: MixEngineScreen) => void;
}) {
  const { t } = useTranslation();

  return (
    <nav className={styles.sidebar} aria-label={t("mixengine.sidebar.label")}>
      {GROUPS.map((group, index) => (
        <div key={group.labelKey ?? `group-${index}`} className={styles.group}>
          {group.labelKey !== null && <h3 className={styles.heading}>{t(group.labelKey)}</h3>}
          {group.items.map((item) => (
            <button
              key={item.labelKey}
              type="button"
              className={styles.item}
              aria-current={item.screen === screen ? "page" : undefined}
              // The screen's id, for whatever has to reach one without reading its label: the tab
              // restores no screen, so the screenshot scenes click their way to one.
              data-screen={item.screen}
              onClick={() => onSelect(item.screen)}
            >
              <item.Icon size={16} className={styles.icon} />
              {t(item.labelKey)}
            </button>
          ))}
        </div>
      ))}
    </nav>
  );
}
