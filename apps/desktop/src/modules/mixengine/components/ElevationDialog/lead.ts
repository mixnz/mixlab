import type { TranslationKey } from "../../../../i18n";

/** What the dialog knows when it chooses its opening sentence. */
export type LeadState = {
  /** The daemon can raise a system prompt from here. */
  canPrompt: boolean;
  /** A terminal's grant is waiting for its command to be run. */
  terminalWaits: boolean;
  /** At least one row below is left for this machine's own screen. */
  anyMarked: boolean;
};

/**
 * The i18n keys of the dialog's opening sentences, in order.
 *
 * Each sentence says only what is true of the dialog it opens: "one prompt" only where one can be
 * raised, and "anything marked below" only when something is.
 */
export function leadKeys({ canPrompt, terminalWaits, anyMarked }: LeadState): TranslationKey[] {
  if (terminalWaits) {
    return anyMarked
      ? ["mixengine.elevation.leadWhileTerminalWaits", "mixengine.elevation.leadMarkedWaitForScreen"]
      : ["mixengine.elevation.leadWhileTerminalWaits"];
  }

  return [canPrompt ? "mixengine.elevation.lead" : "mixengine.elevation.leadCannotPrompt"];
}
