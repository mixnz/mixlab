import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import type { StorageReport } from "@mixengine/api";

import Button from "../../components/Button";
import ErrorBanner from "../../components/ErrorBanner";
import { errorMessage } from "../../core/errors";
import { useTranslation, type Language } from "../../i18n";
import type { ModuleTabProps } from "../../shell/module";
import * as api from "./api";
import { isDisconnected } from "./daemonState";
import { subscribeDaemonWatch } from "./daemonWatch";
import Sidebar from "./components/Sidebar";
import Blueprints from "./screens/Blueprints";
import Dashboard from "./screens/Dashboard";
import Domains from "./screens/Domains";
import Extensions from "./screens/Extensions";
import Logs from "./screens/Logs";
import Metrics from "./screens/Metrics";
import Projects from "./screens/Projects";
import PhpExtensions from "./screens/PhpExtensions";
import Packages from "./screens/Packages";
import ServicesDetail from "./screens/ServicesDetail";
import Settings from "./screens/Settings";
import Sites from "./screens/Sites";
import { requestLanguageFilter } from "./packagesNavigation";
import { requestLogsService } from "./logsNavigation";
import { requestSitesFilter } from "./sitesNavigation";
import {
  chosenFrom,
  explanationOf,
  isFree,
  oneFolderFor,
  pick,
  rowsFrom,
  type StorageKey,
  type StorageRow,
} from "./storagePicker";
import type { MixEngineScreen } from "./tabState";
import "./mixengine.css";

/** How often the gate asks whether a daemon has come up somewhere else — the tray, `mix`. */
const GATE_POLL_MS = 2000;

/** Trang cài đặt của MixEngine, cho một máy chưa có nó. */
const INSTALL_PAGE_EN = "https://mixnz.github.io/mixlab/en/install/";
/** Only languages with a translated install page go here; everything else falls back to English. */
const INSTALL_PAGE_BY_LANG: Partial<Record<Language, string>> = {
  vi: "https://mixnz.github.io/mixlab/vi/install/",
};

/**
 * Cổng vào module, rồi màn hình.
 *
 * **Ba trạng thái, không phải hai.** *Không chạy* (không dial được nhưng chương trình có trên máy),
 * *không trả lời* (dial được, `/health` không xong), *không có MixEngine*. Gộp cả ba thành một
 * thông báo lỗi là bắt người dùng đoán xem họ phải cài, phải khởi động, hay phải chờ.
 *
 * **Không tự khởi động daemon khi mở tab.** Mở một tab là một cử chỉ rẻ và người dùng có thể chỉ
 * đang tìm nhầm tab; khởi động một daemon đang giám sát database thì không rẻ như vậy. Nút nói rõ
 * nó sắp làm gì.
 */
export default function MixEngineTab({
  isModuleVisible,
  onTitleChange,
  onStateChange,
}: ModuleTabProps) {
  // **Always Dashboard, never the screen open last.** A tab — restored from the last session or
  // opened now — lands where a person sees the state of everything first; the screen they left
  // belonged to a daemon that may since have stopped, changed or been rebuilt. `restored` is not
  // read, and `selectScreen` writes nothing back.
  const [screen, setScreen] = useState<MixEngineScreen>("dashboard");
  const [report, setReport] = useState<api.PresenceReport | null>(null);
  const presence = report === null ? null : report.presence;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { t, lang } = useTranslation();

  /* Chỗ đặt bốn thư mục phình to, và bốn dòng người dùng đang sửa — T146.
     `null` là "chưa hỏi xong", phân biệt với "đã hỏi và quyền chọn đã đóng".

     **Chỉ hỏi ở `notRunning`, không hỏi ở `notInstalled`.** Thiết kế nói picker thuộc về cả hai
     cổng, và điều đó không làm được: câu trả lời tới từ `mixengined --storage`, mà `notInstalled`
     nghĩa là đúng chương trình ấy không có trên máy. Hỏi ở đó là chạy một tiến trình chắc chắn
     hỏng để vẽ một màn hình chắc chắn không vẽ được. Một máy chưa cài MixEngine sẽ thấy picker ở
     lần mở đầu tiên *sau khi* cài — vẫn trước lần cài runtime đầu tiên, nên cửa sổ chọn còn
     nguyên. */
  const [storage, setStorage] = useState<StorageReport | null>(null);
  const [rows, setRows] = useState<StorageRow[]>([]);
  /* Whether the storage question is still out. While it is, Start waits: a click before the four
     rows arrive starts the daemon on its defaults, and once it has run the choice is gone. */
  const [storageAsked, setStorageAsked] = useState(false);
  useEffect(() => {
    if (presence !== "notRunning") return;

    let live = true;
    setStorageAsked(false);
    void api
      .storage()
      .then((answer) => {
        if (!live) return;
        setStorage(answer);
        setRows(rowsFrom(answer));
      })
      // Không hỏi được chỗ đặt file thì cổng vẫn phải vẽ được cái nút của nó: đây là một màn hình
      // thêm vào, không phải điều kiện để khởi động daemon.
      .catch(() => {})
      .finally(() => {
        if (live) setStorageAsked(true);
      });
    return () => {
      live = false;
    };
  }, [presence]);

  async function choose(key: StorageKey) {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setRows((prev) => pick(prev, key, picked));
  }

  async function chooseOneFolder() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setRows((prev) => oneFolderFor(prev, picked));
  }

  /* Mỗi màn hình sidebar tự quản lý watch/reload riêng của nó (qua `subscribeDaemonWatch`) và có
     thể đang giữa một việc dài hơi (một job cài đặt ở Packages) khi người dùng đổi sang màn khác —
     đổi màn không được unmount nó, nếu không state cục bộ đang theo dõi việc đó mất sạch. Nên
     render mỗi màn đã từng xem qua đúng một lần, chỉ ẩn/hiện bằng `hidden`; màn chưa xem qua thì
     chưa vào DOM (mở tất cả chín màn ngay từ đầu là chín lượt gọi API cho những màn có thể không
     bao giờ được xem).
     Khai báo trước mọi `return` sớm bên dưới (cổng "chưa hỏi xong"/"daemon không chạy") — Hook
     phải chạy đều ở mọi lần render, không được đứng sau một nhánh return. */
  const [mountedScreens, setMountedScreens] = useState<MixEngineScreen[]>([screen]);
  useEffect(() => {
    setMountedScreens((prev) => (prev.includes(screen) ? prev : [...prev, screen]));
  }, [screen]);

  /* Every time the daemon comes up — Start, Retry, or a daemon that came back by itself — the tab
     starts again at Dashboard, and the screens that stayed mounted behind the gate are dropped: they
     hold state read from the daemon that went away. `null` until the first answer, so the first
     look at a daemon already running changes nothing. */
  const wasRunning = useRef<boolean | null>(null);
  useEffect(() => {
    if (presence === null) return;
    const running = presence === "running";
    if (running && wasRunning.current === false) {
      setScreen("dashboard");
      setMountedScreens(["dashboard"]);
    }
    wasRunning.current = running;
  }, [presence]);

  const look = useCallback(async () => {
    setReport(await api.presence());
  }, []);

  useEffect(() => {
    let live = true;
    void api.presence().then((answer) => {
      if (live) setReport(answer);
    });
    return () => {
      live = false;
    };
  }, []);

  /* The tray panel starts and stops the same daemon (T168), so this tab cannot assume it is the
     only one that does. Stopped here and started there: ask again every couple of seconds while
     the gate is up — one `/health` dial that fails at once when nobody is listening. Running here
     and stopped there: the event stream ending says so, and the gate comes back. */
  useEffect(() => {
    if (presence === null || presence === "running") return;
    const timer = window.setInterval(() => void look(), GATE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [presence, look]);
  useEffect(
    () =>
      subscribeDaemonWatch((raw) => {
        if (isDisconnected(raw)) void look();
      }),
    [look],
  );

  useEffect(() => {
    onTitleChange(t("mixengine.newTabTitle"));
  }, [onTitleChange, t]);

  /* Khởi động một daemon hỏng được vì nhiều lý do người dùng sửa được — chương trình không ở chỗ
     đoán, một daemon khác đang giữ lock. Nuốt cái đó đi là để họ bấm một cái nút không làm gì. */
  async function run(work: () => Promise<unknown>) {
    setBusy(true);
    setError("");
    try {
      await work();
      await look();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  // Chưa hỏi xong: một khung trống, không phải một thông báo. Câu trả lời tới trong vài mili giây
  // và một dòng "đang kiểm tra" nhấp nháy thì tệ hơn là không có gì.
  if (report === null || presence === null) return <div className="mixengine-root" />;

  if (presence !== "running") {
    return (
      <div className="mixengine-root mixengine-gate">
        {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
        <p>{t(`mixengine.gate.${presence}`)}</p>
        {presence === "notInstalled" && report.searched.length > 0 && (
          <>
            <p className="mixengine-gate-looked">{t("mixengine.gate.lookedIn")}</p>
            {/* Khoá theo cả chỉ số: một `PATH` thật hay có cùng một thư mục hai lần, và hai `li`
                cùng khoá là một cảnh báo React cho thứ vốn là dữ liệu hợp lệ. */}
            <ul className="mixengine-gate-searched">
              {report.searched.map((dir, index) => (
                <li key={`${index}-${dir}`}>{dir}</li>
              ))}
            </ul>
          </>
        )}
        {storage !== null && (
          <div className="mixengine-gate-storage">
            <p className="mixengine-gate-looked">
              {isFree(storage)
                ? t("mixengine.storage.free")
                : t("mixengine.storage.taken", { what: explanationOf(storage) ?? "" })}
            </p>

            <ul className="mixengine-gate-searched">
              {rows.map((row) => (
                <li key={row.key} className="mixengine-gate-storage-row">
                  <span className="mixengine-gate-storage-name">
                    {t(`mixengine.storage.${row.key}`)}
                  </span>
                  <span className="mixengine-gate-storage-path">{row.picked ?? row.current}</span>
                  {isFree(storage) && (
                    <Button onClick={() => void choose(row.key)} disabled={busy}>
                      {t("mixengine.storage.choose")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>

            {isFree(storage) && (
              <Button onClick={() => void chooseOneFolder()} disabled={busy}>
                {t("mixengine.storage.oneFolder")}
              </Button>
            )}
          </div>
        )}
        {presence === "notRunning" && (
          <Button
            onClick={() => void run(() => api.startDaemon(chosenFrom(rows)))}
            busy={
              busy
                ? t("mixengine.gate.starting")
                : storageAsked
                  ? undefined
                  : t("mixengine.gate.readingStorage")
            }
          >
            {t("mixengine.gate.start")}
          </Button>
        )}
        {presence === "notAnswering" && (
          <Button onClick={() => void run(() => Promise.resolve())} disabled={busy}>
            {t("mixengine.gate.retry")}
          </Button>
        )}
        {presence === "notInstalled" && (
          <Button onClick={() => void openUrl(INSTALL_PAGE_BY_LANG[lang] ?? INSTALL_PAGE_EN)}>
            {t("mixengine.gate.getIt")}
          </Button>
        )}
      </div>
    );
  }

  function selectScreen(next: MixEngineScreen) {
    setScreen(next);
    // Forget whatever an older build kept for this tab — this one never reads it.
    onStateChange(undefined);
  }

  /* `render` nhận `active` thay vì nhận thẳng một node dựng sẵn — mỗi màn tự quyết định làm gì với
     nó (đọc lại danh sách khi vừa quay lại, xem `Dashboard.tsx`/`ServicesDetail.tsx`...). Giữ mount
     không kéo theo tự đọc lại: một sự kiện live-update không phải lúc nào cũng phủ hết những gì đổi
     ở màn khác trong lúc màn này bị ẩn (gỡ/cài PHP không sinh `service_state_changed`, nhưng vẫn có
     thể là lý do người dùng quay lại Dashboard/Services để nhìn), nên mỗi màn tự đọc lại lúc `active`
     chuyển sang `true` — đúng câu spec đã viết: "tự đọc lại khi focus quay lại tab". */
  function pane(key: MixEngineScreen, render: (active: boolean) => ReactNode) {
    if (!mountedScreens.includes(key)) return null;
    const active = screen === key;
    return (
      <div key={key} className="mixengine-screen-pane" hidden={!active}>
        {render(active)}
      </div>
    );
  }

  return (
    <div className="mixengine-root mixengine-layout">
      <Sidebar screen={screen} onSelect={selectScreen} />
      <div className="mixengine-screen">
        {pane("dashboard", (active) => (
          <Dashboard
            active={active}
            isModuleVisible={isModuleVisible}
            onViewLogs={(service) => {
              requestLogsService(service);
              selectScreen("logs");
            }}
          />
        ))}
        {pane("projects", (active) => (
          <Projects
            active={active}
            onOpenSites={(project) => {
              requestSitesFilter(project);
              selectScreen("sites");
            }}
          />
        ))}
        {pane("sites", (active) => <Sites active={active} />)}
        {pane("domains", (active) => <Domains active={active} />)}
        {pane("packages", (active) => <Packages active={active} />)}
        {/* A callback rather than a link: a home with no PHP on it has nothing for this screen to
            draw, and the place to install one is Packages right above. The request carried along
            with the jump is what lands it on Languages with `php` already typed — same shape as
            Projects → Sites above, and `selectScreen` so the jump is remembered like any other. */}
        {pane("phpExtensions", (active) => (
          <PhpExtensions
            active={active}
            onInstallPhp={() => {
              requestLanguageFilter("php");
              selectScreen("packages");
            }}
          />
        ))}
        {pane("servicesDetail", (active) => <ServicesDetail active={active} />)}
        {pane("logs", (active) => <Logs active={active} />)}
        {pane("blueprints", (active) => <Blueprints active={active} />)}
        {pane("extensions", (active) => <Extensions active={active} />)}
        {pane("metrics", (active) => <Metrics active={active} />)}
        {pane("settings", (active) => (
          <Settings active={active} />
        ))}
      </div>
    </div>
  );
}
