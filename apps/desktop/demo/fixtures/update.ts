import type { UpdateStatus } from "../../src/shell/update/api";
import { returns, type Handlers } from "../ipc/dispatch";

/** MixLab's own updater (T187), read by the workspace at launch. Up to date and nothing offered:
 *  a demo draws no dot on the Settings button and no update notice. */
const upToDate: UpdateStatus = {
  current: "0.0.8",
  placement: { kind: "swap", directory: "C:\Users\demo\AppData\Local\Programs\MixEngine" },
  feed: null,
  skipped: null,
  automatic: true,
  installing: false,
  downloaded: null,
  checkedAt: null,
  failure: null,
};

export const updateHandlers: Handlers = {
  update_status: returns(upToDate),
  update_check: returns(upToDate),
};
