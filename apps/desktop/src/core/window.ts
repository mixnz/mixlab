import { invoke } from "@tauri-apps/api/core";

/**
 * The application's own windows — the shell's, not any module's. `src-tauri/src/tray.rs`.
 */

/** Shows and focuses the main window, and hides the tray panel. */
export function openMainWindow(): Promise<void> {
  return invoke("tray_open_main");
}

/** Hides the tray panel's window now. Only the panel calls it, once its card has slid out. */
export function hideTrayPanel(): Promise<void> {
  return invoke("tray_hide_panel");
}

/** Quits MixLab. The daemon keeps running. */
export function quitApp(): Promise<void> {
  return invoke("app_quit");
}

/** The tray menu's words, which Rust does not keep a dictionary for. */
export interface TrayLabels {
  openPanel: string;
  openMain: string;
  quit: string;
}

/**
 * Puts the tray icon up, and says whether a module this window draws lends the panel a section
 * (ADR 0058). The backend still decides whether this session can show an icon at all.
 */
export function configureTray(panel: boolean, labels: TrayLabels): Promise<void> {
  return invoke("tray_configure", { panel, labels });
}

/** MixLab at login — ADR 0042, `src-tauri/src/login_item.rs`. What the operating system holds now. */
export interface LoginItem {
  /** `false` in a development build, which never registers an entry. */
  supported: boolean;
  enabled: boolean;
  /** Whether this session can show a tray icon; without one a login start opens the window. */
  trayHost: boolean;
}

export function loginItemStatus(): Promise<LoginItem> {
  return invoke<LoginItem>("login_item_status");
}

export function setLoginItem(enabled: boolean): Promise<LoginItem> {
  return invoke<LoginItem>("login_item_set", { enabled });
}
