import shared from "./en";
import sharedVi from "./vi";
import dbEn from "../modules/db/i18n/en";
import dbVi from "../modules/db/i18n/vi";
import mixengineEn from "../modules/mixengine/i18n/en";
import mixengineVi from "../modules/mixengine/i18n/vi";
import restEn from "../modules/rest/i18n/en";
import restVi from "../modules/rest/i18n/vi";
import terminalEn from "../modules/terminal/i18n/en";
import terminalVi from "../modules/terminal/i18n/vi";
import toolsEn from "../modules/tools/i18n/en";
import toolsVi from "../modules/tools/i18n/vi";
import tunnelEn from "../modules/tunnel/i18n/en";
import tunnelVi from "../modules/tunnel/i18n/vi";

/*
 * The dictionaries, assembled from the shared half and each module's own.
 *
 * Groups stay flat at the top level, which is the whole point: `t("connection.host")` resolves here
 * exactly as it did when one file held everything, so splitting 1039 lines in two changed no call
 * site at all.
 *
 * `error` is the one group more than one owner contributes to — its keys correspond 1-1 to what
 * `err!` emits on the Rust side, and Rust fails in the shared layer (`ssh/`, `secrets.rs`) as well
 * as inside a module. So it is merged by hand, one line, which keeps the type inferred. A flat
 * spread alone would let the later `error` swallow the earlier one and lose a dozen keys with
 * nobody the wiser.
 */
export const EN = {
  ...shared,
  ...dbEn,
  ...mixengineEn,
  ...restEn,
  ...terminalEn,
  ...toolsEn,
  ...tunnelEn,
  error: {
    ...shared.error,
    ...dbEn.error,
    ...mixengineEn.error,
    ...restEn.error,
    ...terminalEn.error,
    ...toolsEn.error,
    ...tunnelEn.error,
  },
};

export const VI = {
  ...sharedVi,
  ...dbVi,
  ...mixengineVi,
  ...restVi,
  ...terminalVi,
  ...toolsVi,
  ...tunnelVi,
  error: {
    ...sharedVi.error,
    ...dbVi.error,
    ...mixengineVi.error,
    ...restVi.error,
    ...terminalVi.error,
    ...toolsVi.error,
    ...tunnelVi.error,
  },
};

/* Outside `error`, no two dictionaries may name the same group: the second spread would silently
   replace the first and take every key of that group with it. This is the net — a clash stops the
   build rather than showing up as a raw key on screen months later. One term per pair of
   dictionaries, so a third module adds three. */
type Collision =
  | Exclude<Extract<keyof typeof shared, keyof typeof dbEn>, "error">
  | Exclude<Extract<keyof typeof shared, keyof typeof restEn>, "error">
  | Exclude<Extract<keyof typeof dbEn, keyof typeof restEn>, "error">
  | Exclude<Extract<keyof typeof shared, keyof typeof terminalEn>, "error">
  | Exclude<Extract<keyof typeof dbEn, keyof typeof terminalEn>, "error">
  | Exclude<Extract<keyof typeof restEn, keyof typeof terminalEn>, "error">
  | Exclude<Extract<keyof typeof shared, keyof typeof toolsEn>, "error">
  | Exclude<Extract<keyof typeof dbEn, keyof typeof toolsEn>, "error">
  | Exclude<Extract<keyof typeof restEn, keyof typeof toolsEn>, "error">
  | Exclude<Extract<keyof typeof terminalEn, keyof typeof toolsEn>, "error">;
const noCollision: [Collision] extends [never] ? true : never = true;
void noCollision;
