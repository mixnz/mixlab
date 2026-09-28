import { useCallback, useEffect, useRef, useState } from "react";

import ActionBar from "../../../../components/ActionBar";
import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ContextMenu from "../../../../components/ContextMenu";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import MonogramBadge from "../../../../components/MonogramBadge";
import PageHeader from "../../../../components/PageHeader";
import SegmentedControl from "../../../../components/SegmentedControl";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import Switch from "../../../../components/Switch";
import Table from "../../../../components/Table";
import {
  CopyIcon,
  DatabaseGenericIcon,
  FolderIcon,
  LockIcon,
  LogIcon,
  MoreIcon,
  PlayIcon,
  PlusIcon,
  PowerIcon,
  ReloadIcon,
  StopIcon,
} from "../../../../icons";
import { copyText } from "../../../../core/clipboard";
import { serialQueue } from "../../../../core/serialQueue";
import { useWindowFocused } from "../../../../core/windowFocus";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DaemonStatus } from "@mixengine/api";
import type { DatabaseClientReport } from "@mixengine/api";
import type { DatabaseCredentials } from "@mixengine/api";
import type { StoppedBy } from "@mixengine/api";
import type { DiskUsage } from "@mixengine/api";
import CredentialDialog from "../../components/CredentialDialog";
import ElevationDialog from "../../components/ElevationDialog";
import ServiceForm from "../../components/ServiceForm";
import {
  applyEvent,
  applyJob,
  isJobFinished,
  movesARow,
  needsResync,
  rowsFrom,
  type JobRow,
  type ServiceRow,
} from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import DaemonUsage from "../../components/DaemonUsage";
import type { MetricsFrame } from "@mixengine/api";
import {
  DAEMON_SUBJECT,
  formatBytes,
  formatCpu,
  metricsSubjectFor,
  parseMetricsFrame,
  readingFor,
} from "../../metricsState";
import { pendingFrom } from "../../pendingOps";
import {
  DATABASE_MODULE_ID,
  openChoices,
  opensADatabase,
} from "../ServicesDetail/openChoices";
import { eventArrived, noReadsYet, readBegan, readLanded } from "../../readOrder";
import { serviceStateHint, serviceStateKey, serviceStateTone, toggleMode } from "../../serviceStateLabel";
import CleanupDialog from "./CleanupDialog";
import DiskUsagePanel from "./DiskUsagePanel";
import FoundServices from "./FoundServices";
import RestorePrevious from "./RestorePrevious";
import PathNudge from "./PathNudge";
import QuickStart from "./QuickStart";
import { shouldOfferQuickStart } from "../../quickStart";
import type { SiteSummary } from "@mixengine/api";
import styles from "./Dashboard.module.css";

/* Bảng tra tường minh chứ không ghép `${action}ing`: "stop" + "ing" ra "stoping", và một khoá dịch
   dựng bằng phép nối chuỗi là một khoá không ai grep ra được. */
const PENDING_LABEL = {
  start: "mixengine.dashboard.starting",
  stop: "mixengine.dashboard.stopping",
  restart: "mixengine.dashboard.restarting",
} as const;

type ServiceFilter = "all" | "running" | "stopped";

/**
 * Daemon, và mọi thứ nó đang giám sát.
 *
 * **Trạng thái đến từ stream, không từ suy đoán.** Bấm Start thì hàng đó chuyển sang `starting` khi
 * `service_state_changed` nói vậy, không phải ngay lúc bấm — một công tắc nói dối về việc MariaDB
 * có đang chạy hay không tệ hơn một công tắc chậm.
 */
export default function Dashboard({
  active,
  isModuleVisible,
  onViewLogs,
}: {
  active: boolean;
  /** Opens the Logs screen on one service — the row menu's *View logs*. */
  onViewLogs: (serviceId: string) => void;
  /** Whether this window draws the built-in database client — T110. Straight through to the row
   *  menu, which is where *open* lives now. */
  isModuleVisible: (moduleId: string) => boolean;
}) {
  const [status, setStatus] = useState<DaemonStatus | null>(null);
  const [rows, setRows] = useState<ServiceRow[]>([]);
  const [pending, setPending] = useState<unknown[] | null>(null);
  /** `ElevationStatus.can_prompt`/`reason` — "còn helper để bật prompt không, và tại sao không khi
   *  không". Mặc định `true` vì đa số máy bật prompt được; chỉ đổi khi `elevation.status` nói khác. */
  const [canPrompt, setCanPrompt] = useState(true);
  const [reason, setReason] = useState<string | null | undefined>(null);
  /** Có bao nhiêu thao tác chờ quyền, theo `daemon.status`. Chỉ là con số; danh sách ở `elevation.status`. */
  const [waiting, setWaiting] = useState(0);
  const [jobs, setJobs] = useState<JobRow[]>([]);
  /** Frame mới nhất của `/metrics`, hoặc `null` khi chưa có (stream chưa mở, hay chưa nhận frame nào). */
  const [frame, setFrame] = useState<MetricsFrame | null>(null);
  const focused = useWindowFocused();
  const [disk, setDisk] = useState<DiskUsage | null>(null);
  /** `site.list`, hay `null` khi chưa đọc xong — điều kiện vẽ thẻ Quick Start (T117). */
  const [sites, setSites] = useState<SiteSummary[] | null>(null);
  const [refreshingDisk, setRefreshingDisk] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  /** Service nào đang có một hành động bay, và là hành động nào. Khoá theo id. */
  const [busy, setBusy] = useState<Record<string, api.ServiceAction>>({});
  /** Menu của một hàng, và chỗ nó được mở ra. `null` là không có menu nào đang mở. */
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  /**
   * `database.client` cho từng service, tra **một lần cho mỗi id** rồi nhớ.
   *
   * **Phải biết trước khi bấm, nên không thể tra lúc mở menu.** Nút ⋮ của một service không phải
   * database phải xám ngay từ lúc vẽ — mở ra một menu rỗng rồi mới biết là tệ hơn không mời bấm.
   *
   * Tra một lượt cho mỗi id là chấp nhận được vì **câu trả lời không bao giờ đổi**: nó là protocol
   * của recipe, tức một thuộc tính của package service này chạy ra. Nên lần đọc `service.list` đầu
   * tiên trả giá N lượt, và mọi lần `reload()` sau đó — mỗi lần quay lại tab, mỗi `resync` — trả
   * giá 0.
   *
   * Vắng mặt một id nghĩa là chưa hỏi xong *hoặc* đã hỏi hỏng, và cả hai đều vẽ ra một nút xám.
   */
  const [databases, setDatabases] = useState<Record<string, DatabaseClientReport>>({});
  /** Những id đã gửi câu hỏi đi, để `rows` đổi theo stream không biến thành một tràng RPC. */
  const asked = useRef(new Set<string>());
  /** Mật khẩu đang hiện trong hộp thoại, hoặc `null`. Không bao giờ nằm trong `rows`. */
  const [credentials, setCredentials] = useState<DatabaseCredentials | null>(null);
  /** Service đang được hỏi "đặt lại mật khẩu?", hoặc `null`. */
  const [resetTarget, setResetTarget] = useState<string | null>(null);
  /** Which rows the Services card shows. */
  const [filter, setFilter] = useState<ServiceFilter>("all");
  /** The home path was just copied, for the button to say so for a moment. */
  const [homeCopied, setHomeCopied] = useState(false);
  const { t } = useTranslation();

  /**
   * Thứ tự giữa các lần đọc và các sự kiện — xem `readOrder.ts`.
   *
   * **`rows` có hai người ghi và không cái nào biết cái kia.** Một snapshot `service.list` và một
   * `service_state_changed` cùng gọi `setRows`, và React ghi theo thứ tự **tới**, không theo thứ
   * tự *đúng*. Bấm Stop all là lúc điều đó lộ ra: nhiều thao tác xong gần nhau, nhiều lượt đọc và
   * nhiều sự kiện chen nhau trên đường về, nên một snapshot cũ ghi đè một snapshot mới — và vì
   * `stopped` là chuyển trạng thái cuối cùng, không còn sự kiện nào sửa lại. Hàng đứng ở "Đang
   * tắt" cho tới khi có người bấm Làm mới.
   *
   * `useRef` chứ không phải `useState`: đây là sổ ghi thứ tự, không phải thứ được vẽ, và một
   * `setState` ở đây sẽ render lại mỗi lần một message đi qua.
   */
  const order = useRef(noReadsYet());

  /* Mọi lỗi đi qua đây thành một câu người đọc được. `errorMessage` dịch `code` và điền `params`,
     nên `hint` của MixEngine tới người dùng nguyên vẹn thay vì rơi vào một promise không ai bắt —
     một tab đứng im, rỗng, không nói gì là kết cục tệ hơn bất kỳ thông báo nào. */
  const reload = useCallback(async () => {
    /* Vòng lặp chứ không đệ quy: một `useCallback` không gọi được chính nó. Nó quay thêm một vòng
       đúng khi có sự kiện chen vào giữa lượt đọc vừa rồi, và dừng ngay lượt đầu tiên không bị
       chen — mỗi vòng là một round trip thật, nên nó tự giới hạn nhịp. */
    for (;;) {
      const began = readBegan(order.current);
      order.current = began.order;
      let landed;
      try {
        const [next, list, usage] = await Promise.all([
          api.status(),
          api.services(),
          api.diskUsage(false),
        ]);
        landed = readLanded(order.current, began.seq);
        order.current = landed.order;
        // Một snapshot khởi hành trước một snapshot đã vẽ rồi thì không được vẽ: nó mang tin cũ
        // hơn thứ đang trên màn hình, dù nó về sau.
        if (landed.apply) {
          setStatus(next);
          setRows(rowsFrom(list.services));
          setWaiting(next.elevation?.pending ?? 0);
          setDisk(usage);
        }
        setError("");
      } catch (e) {
        // Một lượt đọc hỏng vẫn phải hạ cánh, nếu không `inFlight` không bao giờ về 0 và mọi sự
        // kiện sau đó đều bị coi là đang đua.
        landed = readLanded(order.current, began.seq);
        order.current = landed.order;
        setError(errorMessage(t, e));
      }
      if (!landed.readAgain) return;
    }
  }, [t]);

  /**
   * Home này đã có site nào chưa — điều kiện vẽ thẻ Quick Start (T117).
   *
   * Đọc riêng khỏi `reload()` chứ không gộp vào `Promise.all` của nó: `reload()` chạy lại mỗi lần
   * quay lại tab và mỗi lần stream nói có gì đổi, còn câu hỏi này chỉ đổi khi một site được tạo
   * hoặc xoá. Thất bại thì để nguyên giá trị cũ và không dựng banner lỗi: một Dashboard đỏ vì
   * không hỏi được "đã có site chưa" là một Dashboard đỏ vì một câu trang trí.
   */
  const readSites = useCallback(async () => {
    try {
      const listed = await api.sites();
      setSites(listed.sites);
    } catch {
      // Để nguyên: `null` vẫn là "chưa biết", và `shouldOfferQuickStart` không mời trên `null`.
    }
  }, []);

  /** Nút "Làm mới" của bảng disk usage — `refresh: true` đi bộ đĩa lại, khác `reload()` ở trên vốn
   *  đọc bản daemon giữ sẵn (tới một phút) để không biến mỗi lần quay lại tab thành một lần đi bộ. */
  const refreshDisk = useCallback(async () => {
    setRefreshingDisk(true);
    try {
      setDisk(await api.diskUsage(true));
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setRefreshingDisk(false);
    }
  }, [t]);

  /**
   * Mở danh sách thao tác đang chờ quyền quản trị.
   *
   * Một tab mở ra khi hàng đợi đã có sẵn thứ gì đó **không** nhận `elevation_required` — sự kiện đó
   * chỉ bắn lúc hàng đợi đổi. Nên con số ở `daemon.status` là thứ duy nhất nói rằng có gì đó đang
   * chờ, và `elevation.status` là chỗ lấy danh sách để hiện ra.
   */
  const showWaiting = useCallback(async () => {
    try {
      const answer = await api.elevationStatus();
      setCanPrompt(answer.can_prompt);
      setReason(answer.reason);
      setPending(answer.pending);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  /**
   * Gửi một hành động và chờ nó xong. **Không đọc lại** — ai gọi mới quyết định lúc nào đọc.
   *
   * Hàng vẫn đổi theo stream suốt lúc đó, đúng luật "trạng thái được thông báo". Việc đọc lại tách
   * ra khỏi đây vì một lần bấm Stop all là *một* câu hỏi chứ không phải N: xem [`stopAll`].
   */
  const run = useCallback(
    async (id: string, action: api.ServiceAction) => {
      setBusy((current) => ({ ...current, [id]: action }));
      try {
        await api.serviceAction(id, action);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        setBusy((current) => {
          const next = { ...current };
          delete next[id];
          return next;
        });
      }
    },
    [t],
  );

  /**
   * Một hành động trên một hàng, rồi đọc lại.
   *
   * **Sự kiện là best-effort và không bao giờ là đường duy nhất biết trạng thái**, nên tin mỗi
   * stream là để lại một bảng đứng im khi một sự kiện rơi. Đọc lại không phải là suy đoán, nó là
   * đọc — và `order` ở trên là thứ giữ cho lần đọc ấy không bị một lần đọc cũ hơn ghi đè.
   */
  const act = useCallback(
    async (id: string, action: api.ServiceAction) => {
      await run(id, action);
      await reload();
    },
    [reload, run],
  );

  /**
   * Tắt mọi thứ đang chạy, rồi đọc lại **một** lần.
   *
   * Không phải `Promise.all` của `act`: cách đó bắn N lượt `reload` song song cho một lần bấm, mỗi
   * lượt ba RPC, và chúng đua nhau — đúng thứ `order` ở trên tồn tại để chặn. Chặn được không có
   * nghĩa là nên gây ra: một lần bấm là một câu hỏi, nên hỏi một lần.
   */
  const stopAll = useCallback(async () => {
    await Promise.all(
      rows.filter((row) => row.state === "running").map((row) => run(row.id, "stop")),
    );
    await reload();
  }, [reload, rows, run]);

  // Đọc lại lúc mount và mỗi lần vừa quay lại màn này — sự kiện service_state_changed không bao
  // giờ báo tin một service khác được tạo/xoá ở màn Services, và không method-kiểu-runtime nào
  // (cài/gỡ PHP...) sinh sự kiện gì cho bảng này biết cả; quay lại tab vẫn là đường dự phòng.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  // Cùng nhịp, riêng call: xem `readSites`.
  useEffect(() => {
    if (active) void readSites();
  }, [active, readSites]);

  /**
   * `/metrics` khoá vòng đời theo `active`, không theo mount/unmount như `/events`.
   *
   * **Mở kết nối này chính là subscribe** — MixEngine lấy mẫu 1 Hz trong lúc còn ai giữ stream, 1
   * lần/phút khi không. `MixEngineTab.tsx` giữ mọi màn đã-xem-qua ở trong DOM thay vì unmount lúc
   * đổi tab, nên nếu khoá theo unmount, rời Dashboard sang màn khác sẽ không đóng được gì — daemon
   * kẹt ở lấy mẫu nhanh vĩnh viễn dù không còn ai nhìn. Effect cleanup chạy cho cả hai trường hợp
   * (`active` chuyển `false`, và unmount thật), nên khoá theo `active` là đủ cho cả hai.
   *
   * **And only while the window has focus**, the tray panel's rule. `active` says Dashboard is the
   * tab in front, not that anybody is looking: a window left on Dashboard behind another
   * application held the stream open for hours and kept the daemon sampling every second.
   *
   * **Opening and closing go through one queue.** Focus can leave and come back while a stream is
   * still connecting, and the window keeps one `/metrics` slot: a close sent beside the next
   * opening could land after it and end the stream the Dashboard now wants. Queued, each close
   * follows the opening it belongs to, and the next opening follows that close.
   */
  const measuring = active && focused;
  const [metricsQueue] = useState(serialQueue);
  useEffect(() => {
    if (!measuring) return;
    let live = true;
    metricsQueue(async () => {
      // Put away before its turn came: nothing to open.
      if (!live) return;
      await api.metricsWatch((raw) => {
        if (!live) return;
        const next = parseMetricsFrame(raw);
        if (next !== null) setFrame(next);
      });
    });
    return () => {
      live = false;
      setFrame(null);
      metricsQueue(() => api.metricsUnwatch());
    };
  }, [measuring, metricsQueue]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      // Một lô rỗng nghĩa là không còn gì chờ — đóng hộp thoại thay vì để nó đứng đó rỗng không.
      // `elevation_required` mang cả số mới nhất: cập nhật `waiting` thẳng từ đây, không đợi một
      // `reload()` khác — nếu không, nút "N thao tác đang chờ" đứng yên với số cũ sau khi Cho phép,
      // vì bản thân sự kiện này chưa từng được xem là một lý do resync.
      const ops = pendingFrom(raw);
      if (ops !== null) {
        setWaiting(ops.length);
        if (ops.length === 0) {
          setPending(null);
        } else if (active) {
          // `elevation_required` chỉ mang `pending` (đúng hình `{"type":"elevation_required",
          // "pending":[…]}`), không mang `can_prompt`/`reason` — đọc lại qua `elevation.status`
          // trước khi tự mở dialog, để biết máy này còn bật prompt được không (vd. một `hosts-apply`
          // cũ kẹt trong hàng đợi từ trước, nhưng helper vừa bị một lệnh Uninstall xoá).
          //
          // **Chỉ khi màn này đang hiện.** `MixEngineTab` giữ Dashboard trong DOM khi người dùng ở
          // màn khác, và `Modal` vẽ qua portal nên một dialog mở từ đây vẫn nổi lên trên màn đó —
          // trong khi màn tự khởi phát thao tác (CaBlock "Fix browser trust", Doctor "Repair") đã
          // mở dialog của riêng nó cho đúng hàng đợi này: hai modal y hệt, cùng một job grant.
          // Khi ẩn, Dashboard chỉ giữ con số cho nút "N đang chờ"; ai mở tab sẽ thấy nút đó.
          void showWaiting();
        }
      }
      setJobs((current) => applyJob(current, raw));
      // Sự kiện là best-effort: khi bus bên kia tràn hay kết nối đứt, đọc lại thay vì tin cái đang
      // có trên màn hình. Ngoài updater, vì updater chạy hai lần trong StrictMode.
      // `job_finished` cũng là một lý do đọc lại: một `elevation.grant` xong đổi số "N đang chờ"
      // mà không có sự kiện nào riêng nói vậy (xem `isJobFinished`).
      if (needsResync(raw) || isJobFinished(raw)) void reload();
      // Một sự kiện đổi hàng, tới trong lúc một `service.list` đang trên đường về, nghĩa là
      // snapshot đó có thể đã đọc *trước* sự kiện này — client không phân biệt được. Ghi lại ở đây
      // để lượt đọc ấy xin thêm một lượt nữa khi hạ cánh. Ngoài updater, cùng lý do hai dòng trên.
      if (movesARow(raw)) order.current = eventArrived(order.current);
      setRows((current) => applyEvent(current, raw).rows);
    });
  }, [active, reload, showWaiting]);



  /**
   * Hỏi `database.client` cho những id chưa từng hỏi.
   *
   * **`ServiceSummary` không trả lời được câu này.** `ServiceRole` chỉ phân biệt front end với
   * phần còn lại, và [ADR 0026] cấm client suy ra vai trò từ tên package — nên `database.client`
   * là đường duy nhất. Chạy theo `rows` vì đó là nơi một service mới xuất hiện.
   *
   * Một câu hỏi hỏng thì **bỏ id ra khỏi `asked`**: lần `reload()` sau hỏi lại. Giữ nó lại là để
   * một trục trặc thoáng qua khoá nút ⋮ của hàng đó cho tới khi đóng cửa sổ.
   *
   * [ADR 0026]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md
   */
  useEffect(() => {
    const missing = rows.map((row) => row.id).filter((id) => !asked.current.has(id));
    if (missing.length === 0) return;
    for (const id of missing) asked.current.add(id);

    void (async () => {
      const answers = await Promise.all(
        missing.map(async (id) => {
          try {
            return [id, await api.databaseClient(id)] as const;
          } catch {
            asked.current.delete(id);
            return [id, null] as const;
          }
        }),
      );
      setDatabases((current) => {
        const next = { ...current };
        for (const [id, report] of answers) if (report !== null) next[id] = report;
        return next;
      });
    })();
  }, [rows]);

  /** Lấy mật khẩu quản trị viên của một service và mở hộp thoại. */
  const showCredentials = useCallback(
    async (id: string) => {
      try {
        setCredentials(await api.databaseCredentials(id));
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [t],
  );

  /** Chạy `service.reset_credential`, rồi đọc lại — nó dừng và bật lại nhiều service. */
  const resetCredential = useCallback(
    async (id: string) => {
      try {
        await api.serviceResetCredential(id);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        await reload();
      }
    },
    [reload, t],
  );


  /**
   * Bật hay tắt autostart cho một service — cùng cột mà `AutostartPanel` ở màn Services đổi.
   *
   * **Ghi lại thứ daemon trả về, không phải thứ vừa bấm.** Một hàng nói "có" trong khi cột trong
   * database vẫn là "không" tệ hơn một hàng đổi chậm — cùng luật cả bảng này đang theo.
   *
   * Không khởi động và không dừng gì cả, nên không đi qua `busy`: thứ nó đổi là walk ở lần daemon
   * khởi động sau (T112/T113).
   */
  const setAutostart = useCallback(
    async (id: string, autostart: boolean) => {
      try {
        const summary = await api.serviceSetAutostart({ service: id, autostart });
        setRows((current) =>
          current.map((row) => (row.id === id ? { ...row, autostart: summary.autostart } : row)),
        );
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [t],
  );

  /** Hàng đang mở menu. Menu chỉ sống cùng một `menu.id`, nhưng bảng thì cập nhật từ stream, nên
   *  đọc lại từ `rows` thay vì chụp ảnh hàng lúc mở — nhãn autostart phải theo cột bên cạnh. */
  const menuRow = menu === null ? undefined : rows.find((row) => row.id === menu.id);

  /** Câu trả lời cho hàng đang mở menu, buộc vào một tên: `databases[menu.id]` đọc hai lần thì
   *  TypeScript mất luôn phần thu hẹp kiểu trên `client`.
   *
   *  `opensADatabase` lọc ngay ở đây chứ không còn ở nút ⋮: `database.client` cũng trả lời cho
   *  nginx và cho một php-fpm pool, chỉ là với `protocol: null`, nên có mặt trong `databases`
   *  không có nghĩa là có gì để mở. */
  const menuReport =
    menuRow === undefined ? undefined : databases[menuRow.id];
  const menuDatabase =
    menuReport !== undefined && opensADatabase(menuReport) ? menuReport : undefined;

  /** The state as a pill tone: whether it is serving, not which of the seven states it is in. */
  function pillTone(state: string | null | undefined, stoppedBy?: StoppedBy | null): StatusTone {
    const tone = serviceStateTone(state, stoppedBy);
    if (tone === "ok") return "success";
    if (tone === "bad") return "danger";
    if (tone === "busy") return "warning";
    return "neutral";
  }

  /** Trạng thái đã dịch; một trạng thái daemon mới hơn build này hiện nguyên văn. */
  function stateLabel(state: string | null | undefined, stoppedBy?: StoppedBy | null): string {
    const key = serviceStateKey(state, stoppedBy);
    return key === null ? (state ?? "—") : t(key);
  }

  /** Câu mà nhãn ngắn bỏ bớt, cho tooltip của pill. */
  function stateHint(state: string | null | undefined, stoppedBy?: StoppedBy | null): string | undefined {
    const key = serviceStateHint(state, stoppedBy);
    return key === null ? undefined : t(key);
  }

  const runningCount = rows.filter((row) => isServing(row.state)).length;
  const movingCount = rows.filter(
    (row) => busy[row.id] !== undefined || toggleMode(row.state, false) === "moving",
  ).length;
  const shown = rows.filter((row) =>
    filter === "all" ? true : filter === "running" ? isServing(row.state) : !isServing(row.state),
  );
  const daemon = readingFor(frame, DAEMON_SUBJECT);

  function copyHome(home: string) {
    void copyText(home).then(() => {
      setHomeCopied(true);
      window.setTimeout(() => setHomeCopied(false), 1600);
    });
  }

  return (
    <div className={`mixengine-page ${styles.dashboard}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.dashboard")}
        badges={
          status && (
            <>
              <StatusPill tone="neutral" className={styles.version}>
                {t("mixengine.dashboard.versionBadge", { version: status.version })}
              </StatusPill>
              {movingCount > 0 ? (
                <StatusPill tone="warning" pulse>
                  {t("mixengine.dashboard.summaryChanging", { count: movingCount })}
                </StatusPill>
              ) : runningCount > 0 ? (
                <StatusPill tone="success">
                  {t("mixengine.dashboard.summaryRunning", { running: runningCount, total: rows.length })}
                </StatusPill>
              ) : (
                rows.length > 0 && (
                  <StatusPill tone="neutral">{t("mixengine.dashboard.summaryStopped")}</StatusPill>
                )
              )}
            </>
          )
        }
        meta={
          status && (
            <div className={styles.metaRow}>
              <div className={styles.home}>
                <FolderIcon size={14} className={styles.homeIcon} />
                <span className={styles.homePath} title={status.home}>
                  {status.home}
                </span>
                <Button
                  size="small"
                  variant="ghost"
                  className={styles.copy}
                  onClick={() => copyHome(status.home)}
                  aria-label={t("mixengine.dashboard.copyHome")}
                >
                  <CopyIcon size={13} />
                  {homeCopied ? t("mixengine.dashboard.copied") : t("mixengine.dashboard.copy")}
                </Button>
              </div>
              {/* Daemon không có `ServiceRow` — vẽ riêng khỏi bảng service, không chèn vào `rows`.
                  Luôn vẽ, kể cả trước frame đầu tiên: khung đứng sẵn với "—" thay vì hiện ra sau và
                  đẩy cả màn xuống — `frame` về `null` mỗi lần rời tab, nên cú nhảy đó lặp lại mỗi
                  lần quay lại. */}
              <DaemonUsage reading={daemon} cores={frame?.cores ?? 1} />
            </div>
          )
        }
        actions={
          <div className={styles.headerButtons}>
            {/* Không tự bật hộp thoại lúc mở tab: một lô có thể nằm chờ nhiều ngày, và một modal bật
                lên mỗi lần mở tab là thứ người ta học cách bấm bỏ mà không đọc. */}
            {waiting > 0 && pending === null && (
              <Button size="large" className={styles.waiting} onClick={() => void showWaiting()}>
                {t("mixengine.dashboard.elevationWaiting", { count: waiting })}
              </Button>
            )}
            {/* Đường dự phòng thủ công: một service được tạo/xoá từ nơi khác không sinh sự kiện nào
                cho bảng này biết. */}
            <Button size="large" onClick={() => void reload()}>
              <ReloadIcon size={15} />
              {t("mixengine.dashboard.reload")}
            </Button>
            {/* Không đổi hàng nào ở đây: bảng đổi khi `service_state_changed` tới, không khi bấm. */}
            <Button
              size="large"
              variant="danger"
              onClick={() => void stopAll()}
              disabled={rows.every((row) => row.state !== "running") || Object.keys(busy).length > 0}
            >
              <StopIcon size={13} />
              {t("mixengine.dashboard.stopAll")}
            </Button>
            <Button size="large" variant="primary" onClick={() => setCreating(true)}>
              <PlusIcon size={15} />
              {t("mixengine.dashboard.newService")}
            </Button>
          </div>
        }
      />

      {/* Trên bảng service, và chỉ khi home này chưa có site nào — T117. */}
      {shouldOfferQuickStart(sites) && <QuickStart onCreated={() => void readSites()} />}
      <PathNudge active={active} />
      {/* T182h: a copy of an earlier install's state, while this home has nothing of its own. */}
      <RestorePrevious active={active} onRestored={() => void reload()} />
      {/* T182g: service data an earlier install left, until nothing is left to adopt. */}
      <FoundServices active={active} onAdopted={() => void reload()} />

      <Card
        flush
        title={t("mixengine.dashboard.servicesTitle")}
        description={t("mixengine.dashboard.servicesAbout")}
        actions={
          <SegmentedControl<ServiceFilter>
            aria-label={t("mixengine.dashboard.servicesTitle")}
            value={filter}
            onChange={setFilter}
            segments={[
              { value: "all", label: t("mixengine.dashboard.filterAll"), count: rows.length },
              { value: "running", label: t("mixengine.dashboard.filterRunning"), count: runningCount },
              {
                value: "stopped",
                label: t("mixengine.dashboard.filterStopped"),
                count: rows.length - runningCount,
              },
            ]}
          />
        }
      >
        {/* Tiến độ vẽ ngay tại chỗ, không phủ spinner lên cả màn hình. */}
        {jobs.length > 0 && (
          <ul className={styles.jobs}>
            {jobs.map((job) => (
              <li key={job.id}>
                <span>{job.kind || t("mixengine.dashboard.job")}</span>
                <progress value={job.percent} max={100} />
                <span className={styles.jobMessage}>{job.message}</span>
              </li>
            ))}
          </ul>
        )}

        {rows.length === 0 ? (
          <EmptyState title={t("mixengine.dashboard.noServices")} />
        ) : shown.length === 0 ? (
          <EmptyState
            title={
              filter === "running"
                ? t("mixengine.dashboard.filterEmptyRunning")
                : t("mixengine.dashboard.filterEmptyStopped")
            }
            action={
              <Button size="small" onClick={() => setFilter("all")}>
                {t("mixengine.dashboard.showAll")}
              </Button>
            }
          />
        ) : (
          <Table aria-label={t("mixengine.dashboard.servicesTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.dashboard.service")}</th>
                <th>{t("mixengine.dashboard.state")}</th>
                {/* Cạnh State — T114: cái gì đang chạy, và cái gì sẽ chạy sau lần đăng nhập tới. */}
                <th>{t("mixengine.dashboard.autostart")}</th>
                <th>{t("mixengine.dashboard.port")}</th>
                <th>{t("mixengine.dashboard.cpu")}</th>
                <th>{t("mixengine.dashboard.memory")}</th>
                <th data-align="end">{t("mixengine.dashboard.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((row) => {
                const reading = readingFor(frame, metricsSubjectFor(row.id));
                const mode = toggleMode(row.state, busy[row.id] !== undefined);
                return (
                  <tr key={row.id}>
                    <td>
                      <span className={styles.service}>
                        <MonogramBadge name={row.id} size={34} />
                        <span className={styles.serviceName} title={row.id}>
                          {row.id}
                        </span>
                        {row.version !== null && <span className={styles.serviceVersion}>{row.version}</span>}
                      </span>
                    </td>
                    <td data-nowrap>
                      {busy[row.id] ? (
                        /* Một hành động vừa gửi đi và chưa có sự kiện nào xác nhận: cũng là "đang
                           chuyển", nên cùng tông với `starting`/`stopping`. */
                        <StatusPill tone="warning" pulse>
                          {t(PENDING_LABEL[busy[row.id]])}
                        </StatusPill>
                      ) : (
                        <StatusPill
                          tone={pillTone(row.state, row.stoppedBy)}
                          pulse={mode === "moving"}
                          title={stateHint(row.state, row.stoppedBy)}
                        >
                          {stateLabel(row.state, row.stoppedBy)}
                        </StatusPill>
                      )}
                    </td>
                    <td>
                      <Switch
                        small
                        checked={row.autostart}
                        onChange={(next) => void setAutostart(row.id, next)}
                        aria-label={t(
                          row.autostart
                            ? "mixengine.dashboard.autostartOff"
                            : "mixengine.dashboard.autostartOn",
                        )}
                      />
                    </td>
                    <td className={row.port === null ? styles.none : styles.mono} data-nowrap>{row.port ?? "—"}</td>
                    {/* Vắng mặt trong frame là "—", không phải 0%: một service rảnh và một service không
                        đo được là hai câu khác nhau. */}
                    <td className={styles.mono} data-nowrap>
                      {formatCpu(reading?.cpu_percent ?? null, frame?.cores ?? 1)}
                    </td>
                    <td className={styles.mono} data-nowrap>
                      {reading === null ? "—" : formatBytes(reading.rss_bytes)}
                    </td>
                    <td data-align="end" data-nowrap>
                      <span className={styles.actions}>
                        {mode === "moving" ? (
                          <Button
                            size="small"
                            className={styles.toggle}
                            busy={busy[row.id] ? t(PENDING_LABEL[busy[row.id]]) : stateLabel(row.state, row.stoppedBy)}
                            aria-label={t("mixengine.dashboard.moving", { service: row.id })}
                          />
                        ) : mode === "up" ? (
                          <Button
                            size="small"
                            className={`${styles.toggle} ${styles.stop}`}
                            aria-label={t("mixengine.dashboard.stopService", { service: row.id })}
                            onClick={() => void act(row.id, "stop")}
                          >
                            <StopIcon size={11} className={styles.stopMark} />
                            {t("mixengine.dashboard.stop")}
                          </Button>
                        ) : (
                          <Button
                            size="small"
                            variant="positive"
                            className={styles.toggle}
                            aria-label={t("mixengine.dashboard.startService", { service: row.id })}
                            onClick={() => void act(row.id, "start")}
                          >
                            <PlayIcon size={11} />
                            {t("mixengine.dashboard.start")}
                          </Button>
                        )}
                        <Button
                          size="small"
                          onClick={() => void act(row.id, "restart")}
                          disabled={busy[row.id] !== undefined || mode !== "up"}
                        >
                          <ReloadIcon size={13} />
                          {t("mixengine.dashboard.restart")}
                        </Button>
                        {/* Never greyed: autostart and the logs are there for *every* service. */}
                        <ActionBar
                          actions={[
                            {
                              key: "menu",
                              icon: MoreIcon,
                              label: t("mixengine.dashboard.rowMenu"),
                              onClick: (event) => {
                                const at = event.currentTarget.getBoundingClientRect();
                                setMenu({ id: row.id, x: at.left, y: at.bottom });
                              },
                            },
                          ]}
                        />
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      <DiskUsagePanel
        disk={disk}
        refreshing={refreshingDisk}
        onRefresh={() => void refreshDisk()}
        onCleanup={() => setCleaning(true)}
      />

      {cleaning && disk && (
        <CleanupDialog
          disk={disk}
          onCancel={() => setCleaning(false)}
          onStarted={() => setCleaning(false)}
        />
      )}

      {/* Menu này không bao giờ rỗng: autostart và log có mặt cho *mọi* service. */}
      {menu !== null && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          <button
            type="button"
            onClick={() => {
              const id = menu.id;
              setMenu(null);
              onViewLogs(id);
            }}
          >
            <LogIcon size={14} />
            {t("mixengine.dashboard.viewLogs")}
          </button>

          {menuRow !== undefined && (
            <button
              type="button"
              disabled={menuRow.port === null}
              onClick={() => {
                const port = menuRow.port;
                setMenu(null);
                if (port !== null) void copyText(String(port));
              }}
            >
              <CopyIcon size={14} />
              {t("mixengine.dashboard.copyPort")}
            </button>
          )}

          {/* Nhãn lật theo cột Autostart của chính hàng đó, chứ không phải một dấu tick. */}
          {menuRow !== undefined && (
            <button
              type="button"
              onClick={() => {
                const id = menuRow.id;
                const wanted = !menuRow.autostart;
                setMenu(null);
                void setAutostart(id, wanted);
              }}
            >
              <PowerIcon size={14} />
              {t(
                menuRow.autostart
                  ? "mixengine.dashboard.autostartOff"
                  : "mixengine.dashboard.autostartOn",
              )}
            </button>
          )}

          {menuDatabase !== undefined && (
            <>
              <div className="context-menu-separator" />
              {openChoices(menuDatabase.client, isModuleVisible(DATABASE_MODULE_ID)).map((choice) => (
                <button
                  key={choice}
                  type="button"
                  onClick={() => {
                    const id = menu.id;
                    setMenu(null);
                    // A `db` tab in this window; the password travels neither way through here (T83).
                    void api.databaseOpenInMixDB(id).catch((e: unknown) => setError(errorMessage(t, e)));
                  }}
                >
                  <DatabaseGenericIcon size={14} />
                  {t(
                    choice === "builtIn"
                      ? "mixengine.dashboard.exploreData"
                      : "mixengine.dashboard.exploreDataEnabling",
                  )}
                </button>
              ))}

              <button
                type="button"
                onClick={() => {
                  const id = menu.id;
                  setMenu(null);
                  void showCredentials(id);
                }}
              >
                <LockIcon size={14} />
                {t("mixengine.dashboard.credentials")}
              </button>

              {/* Dấu ba chấm là lời hứa: bấm vào mở một câu hỏi, không chạy ngay. */}
              <button
                type="button"
                onClick={() => {
                  const id = menu.id;
                  setMenu(null);
                  setResetTarget(id);
                }}
              >
                <ReloadIcon size={14} />
                {t("mixengine.dashboard.resetCredential")}
              </button>
            </>
          )}
        </ContextMenu>
      )}

      {credentials !== null && (
        <CredentialDialog credentials={credentials} onClose={() => setCredentials(null)} />
      )}

      {/* Không `danger`: `ConfirmDialog` dành màu đó cho thao tác **mất dữ liệu**, và đây giữ
          nguyên mọi database. */}
      {resetTarget !== null && (
        <ConfirmDialog
          title={t("mixengine.dashboard.resetTitle")}
          message={t("mixengine.dashboard.resetMessage", { service: resetTarget })}
          confirmLabel={t("mixengine.dashboard.resetConfirm")}
          onConfirm={() => {
            const id = resetTarget;
            setResetTarget(null);
            void resetCredential(id);
          }}
          onCancel={() => setResetTarget(null)}
        />
      )}

      {creating && (
        <ServiceForm
          onCancel={() => setCreating(false)}
          onCreated={() => {
            setCreating(false);
            void reload();
          }}
        />
      )}

      {pending && (
        <ElevationDialog
          pending={pending}
          canPrompt={canPrompt}
          reason={reason}
          onClose={() => {
            setPending(null);
            // Sau grant hoặc drop, hàng đợi đã khác: đọc lại con số thay vì giữ cái cũ.
            void reload();
          }}
        />
      )}
    </div>
  );
}

/** Serving, for the filter and the summary: `degraded` still answers. */
function isServing(state: string | null | undefined): boolean {
  return state === "running" || state === "degraded";
}
