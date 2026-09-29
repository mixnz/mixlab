import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import ActionBar from "../../components/ActionBar";
import Button from "../../components/Button";
import ErrorBoundary from "../../components/ErrorBoundary";
import { IS_MAC, IS_WINDOWS } from "../../core/platform";
import { hideTrayPanel, openMainWindow, quitApp } from "../../core/window";
import { PowerIcon } from "../../icons";
import { useTranslation } from "../../i18n";
import type { ModuleDefinition } from "../module";
import styles from "./TrayFrame.module.css";

/* The popover slides in from the right on the two systems where it is one. On Linux it is an
   ordinary window the window manager placed, and content sliding about inside it would look broken. */
const SLIDES = IS_MAC || IS_WINDOWS;

/** The slide out, to the millisecond of `TrayFrame.module.css`'s exit transition. */
const SLIDE_OUT_MS = 420;

/**
 * The tray panel's frame — T192, ADR 0058. The window's part of what T168 built: the card, its
 * slide, every way it goes away, and a header that is MixLab's — the mark, the slogan, Open MixLab
 * and Quit MixLab. Under it, every section a visible module lends, in the registry's order, each
 * behind its own error boundary so that one that throws never takes the ways out with it.
 */
export default function TrayFrame({ sections }: { sections: ModuleDefinition[] }) {
  const { t } = useTranslation();
  /* Off to the right until the window is focused, and back there the moment it is not — so that
     the next show starts from the edge instead of flashing where it was. */
  const [shown, setShown] = useState(!SLIDES);
  /* On Linux the window is never slid away, so "shown" is always true there; what a section needs
     to know about attention is focus. */
  const [focused, setFocused] = useState(false);
  /* The hide waiting for the slide out to finish — cancelled when the panel is shown again before
     it has, so a quick re-open is not hidden from under the person. */
  const leaving = useRef<ReturnType<typeof setTimeout> | null>(null);

  /**
   * Put the panel away: slide the card out, and only then hide the window. Hiding first would
   * leave the card where it was in the window's last frame, and the next show would flash it in
   * place before sliding it in. On Linux there is no slide, so the window goes at once.
   */
  const dismiss = useCallback(() => {
    if (!SLIDES) {
      void hideTrayPanel();
      return;
    }
    if (leaving.current !== null) return;
    setShown(false);
    leaving.current = setTimeout(() => {
      leaving.current = null;
      void hideTrayPanel();
    }, SLIDE_OUT_MS);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let live = true;
    void getCurrentWindow()
      .onFocusChanged(({ payload: now }) => {
        setFocused(now);
        if (!now) return;
        if (leaving.current !== null) {
          clearTimeout(leaving.current);
          leaving.current = null;
        }
        // One frame at the edge first, or the browser skips straight to the end of the slide.
        if (SLIDES) requestAnimationFrame(() => setShown(true));
      })
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      });
    return () => {
      live = false;
      unlisten?.();
    };
  }, []);

  /* Every way the panel goes — a click elsewhere, a click on the icon, Open MixLab — arrives from
     `src-tauri/src/tray.rs` as this one event, so all of them slide out the same way. */
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let live = true;
    void getCurrentWindow()
      .listen("tray://dismiss", dismiss)
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [dismiss]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") dismiss();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dismiss]);

  useEffect(
    () => () => {
      if (leaving.current !== null) clearTimeout(leaving.current);
    },
    [],
  );

  return (
    <div className={styles.stage} data-slides={SLIDES || undefined}>
      <div className={`${styles.panel} ${shown ? styles.shown : ""}`}>
        <header className={styles.header}>
          <img className={styles.logo} src="/logo.svg" alt="" width={38} height={38} />
          <div className={styles.brand}>
            <span className={styles.name}>MixLab</span>
            <span className={styles.slogan}>{t("tray.slogan")}</span>
          </div>
          <div className={styles.actions} data-density="compact">
            <Button size="small" variant="primary" onClick={() => void openMainWindow()}>
              {t("tray.openMain")}
            </Button>
            <ActionBar
              actions={[{ key: "quit", icon: PowerIcon, label: t("tray.quit"), onClick: () => void quitApp() }]}
            />
          </div>
        </header>

        <div className={styles.body}>
          {sections.map(({ id, TraySection }) =>
            TraySection === undefined ? null : (
              <ErrorBoundary key={id}>
                <TraySection shown={shown} focused={focused} dismiss={dismiss} />
              </ErrorBoundary>
            ),
          )}
        </div>
      </div>
    </div>
  );
}
