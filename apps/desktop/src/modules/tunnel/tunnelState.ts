import type { StatusTone } from "../../components/StatusPill";
import type { TranslationKey } from "../../i18n";

/** Which program a tunnel runs — `Provider` in `src-tauri/src/modules/tunnel/provider.rs`. */
export type Provider = "cloudflare" | "ngrok";

export const PROVIDERS: readonly Provider[] = ["cloudflare", "ngrok"];

/** One tunnel as the backend reports it — `TunnelInfo` in `src-tauri/src/modules/tunnel/state.rs`. */
export interface TunnelInfo {
  id: number;
  provider: Provider;
  target: string;
  url: string | null;
  state: "connecting" | "open" | "failed";
  detail: string | null;
  hint: "allowedHosts" | "nothingListening" | "needsAuthtoken" | null;
}

/** The pill's tone: in transition while connecting, good once open, bad once failed. */
export function rowTone(info: TunnelInfo): StatusTone {
  if (info.state === "open") return "success";
  if (info.state === "failed") return "danger";
  return "warning";
}

/** A download's size, as a person reads it before agreeing to it. */
export function formatSize(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** A stored provider, or Cloudflare for nothing stored or a value this build does not know. */
export function parseProvider(stored: string | null): Provider {
  return PROVIDERS.find((provider) => provider === stored) ?? "cloudflare";
}

/** Where a program's path is kept. cloudflared's key predates ngrok and keeps its name. */
export function programPathKey(provider: Provider): string {
  return provider === "cloudflare" ? "mixlab-tunnel-cloudflared" : "mixlab-tunnel-ngrok";
}

/** Whether ngrok has an authtoken, and where from — `Status` in `src-tauri/src/modules/tunnel/authtoken.rs`. */
export interface AuthtokenStatus {
  set: boolean;
  source: "config" | "environment" | null;
  path: string | null;
}

/** What Settings says about the token: where it comes from, never what it is. */
export function tokenStateKey(status: AuthtokenStatus): TranslationKey {
  if (status.source === "environment") return "tunnelTab.tokenInEnvironment";
  if (status.set) return "tunnelTab.tokenInConfig";
  return "tunnelTab.tokenNone";
}

/** Whether the tab asks for a token before Start: ngrok chosen, ngrok found, and no token known. */
export function needsTokenCard(provider: Provider, ngrokFound: boolean, status: AuthtokenStatus | null): boolean {
  return provider === "ngrok" && ngrokFound && status !== null && !status.set;
}

/** The string a row's hint is said with, the same in the tab and in the tray. */
export function hintKey(info: TunnelInfo): { key: TranslationKey; params?: Record<string, string> } | null {
  if (info.hint === "allowedHosts") return { key: "tunnelTab.hintAllowedHosts" };
  if (info.hint === "nothingListening") {
    return { key: "tunnelTab.hintNothingListening", params: { target: info.target } };
  }
  if (info.hint === "needsAuthtoken") return { key: "tunnelTab.hintNeedsAuthtoken" };
  return null;
}
