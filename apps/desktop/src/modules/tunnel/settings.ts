import { parseProvider, programPathKey, type Provider } from "./tunnelState";

/**
 * What this module remembers: which provider the form starts with, and a program the person
 * chose, by path, for each.
 *
 * In `localStorage` because it is about this machine and nothing else: it is not synced, and it is
 * not in a tab's session slot, which holds ids only. An empty path means *use one on PATH, or the
 * one MixLab downloads*. No authtoken is ever kept here: ngrok keeps its own.
 */
const PROVIDER_KEY = "mixlab-tunnel-provider";

export function readProvider(): Provider {
  try {
    return parseProvider(localStorage.getItem(PROVIDER_KEY));
  } catch {
    return "cloudflare";
  }
}

export function writeProvider(provider: Provider): void {
  try {
    localStorage.setItem(PROVIDER_KEY, provider);
  } catch {
    // A full or blocked storage forgets the choice, and the form starts with Cloudflare.
  }
}

export function readProgramPath(provider: Provider): string {
  try {
    return localStorage.getItem(programPathKey(provider)) ?? "";
  } catch {
    return "";
  }
}

export function writeProgramPath(provider: Provider, path: string): void {
  try {
    if (path.trim() === "") localStorage.removeItem(programPathKey(provider));
    else localStorage.setItem(programPathKey(provider), path.trim());
  } catch {
    // A full or blocked storage loses the setting, and the module falls back to PATH.
  }
}
