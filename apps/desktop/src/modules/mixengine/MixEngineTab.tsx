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
import { TERMINAL_MODULE_ID } from "./nextSteps";
import "./mixengine.css";

/** How often the gate asks whether a daemon has come up somewhere else — the tray, `mix`. */
const GATE_POLL_MS = 2000;

/** MixEngine's install page, for a machine that does not have it yet. */
const INSTALL_PAGE_EN = "https://mixnz.github.io/mixlab/en/install/";
/** Only languages with a translated install page go here; everything else falls back to English. */
const INSTALL_PAGE_BY_LANG: Partial<Record<Language, string>> = {
  vi: "https://mixnz.github.io/mixlab/vi/install/",
};

/**
 * The module's gate, then the screens.
 *
 * **Three states, not two.** *Not running* (cannot dial, but the program is on the machine), *not
 * responding* (dials, but `/health` does not finish), *no MixEngine*. Folding all three into one
 * error message makes the user guess whether they have to install, start or wait.
 *
 * **Opening the tab does not start the daemon.** Opening a tab is a cheap gesture and the user may
 * just have picked the wrong tab; starting a daemon that supervises databases is not that cheap.
 * The button says plainly what it is about to do.
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

  /* Where the four growing directories go, and the four rows the user is editing — T146.
     `null` is "not finished asking", as distinct from "asked, and the choice is closed".

     **Only asked at `notRunning`, not at `notInstalled`.** The design says the picker belongs to
     both gates, and that cannot be done: the answer comes from `mixengined --storage`, and
     `notInstalled` means that very program is not on the machine. Asking there would run a process
     certain to fail in order to draw a screen certain not to draw. A machine without MixEngine sees
     the picker on the first open *after* installing — still before the first runtime install, so
     the window for choosing is still intact. */
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
      // If we cannot ask where the files go, the gate must still be able to draw its button: this
      // is an extra screen, not a condition for starting the daemon.
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

  /* Each sidebar screen manages its own watch/reload (through `subscribeDaemonWatch`) and may be in
     the middle of something long-running (an install job in Packages) when the user switches to
     another screen — switching must not unmount it, or the local state tracking that work is lost.
     So each screen ever visited is rendered exactly once and only shown/hidden with `hidden`; a
     screen not yet visited is not in the DOM (opening all nine screens up front would be nine API
     calls for screens that may never be looked at).
     Declared before every early `return` below (the "not finished asking"/"daemon not running"
     gates) — Hooks must run on every render, never after a return branch. */
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

  /* Starting a daemon can fail for many reasons the user can fix — the program is not where we
     guessed, another daemon holds the lock. Swallowing that leaves them pressing a button that does
     nothing. */
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

  // Not finished asking: an empty frame, not a message. The answer arrives within milliseconds, and
  // a flickering "checking" line is worse than nothing.
  if (report === null || presence === null) return <div className="mixengine-root" />;

  if (presence !== "running") {
    return (
      <div className="mixengine-root mixengine-gate">
        {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
        <p>{t(`mixengine.gate.${presence}`)}</p>
        {presence === "notInstalled" && report.searched.length > 0 && (
          <>
            <p className="mixengine-gate-looked">{t("mixengine.gate.lookedIn")}</p>
            {/* Keyed by the index too: a real `PATH` often has the same directory twice, and two
                `li` with the same key are a React warning for what is valid data. */}
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

  /* `render` takes `active` instead of a ready-built node — each screen decides for itself what to
     do with it (read its list again when it has just come back, see
     `Dashboard.tsx`/`ServicesDetail.tsx`...). Staying mounted does not bring rereading with it: a
     live-update event does not always cover everything that changed on other screens while this
     one was hidden (removing/installing PHP produces no `service_state_changed`, yet may be why the
     user comes back to Dashboard/Services to look), so each screen rereads itself when `active`
     turns `true` — exactly what the spec says: "reread when focus returns to the tab". */
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
            onOpenDashboard={() => selectScreen("dashboard")}
            terminalVisible={isModuleVisible(TERMINAL_MODULE_ID)}
          />
        ))}
        {pane("sites", (active) => (
          <Sites active={active} terminalVisible={isModuleVisible(TERMINAL_MODULE_ID)} />
        ))}
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
        {pane("blueprints", (active) => (
          <Blueprints active={active} terminalVisible={isModuleVisible(TERMINAL_MODULE_ID)} />
        ))}
        {pane("extensions", (active) => <Extensions active={active} />)}
        {pane("metrics", (active) => <Metrics active={active} />)}
        {pane("settings", (active) => (
          <Settings active={active} />
        ))}
      </div>
    </div>
  );
}
