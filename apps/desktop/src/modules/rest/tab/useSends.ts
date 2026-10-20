import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../../../core/errors";
import { useTranslation } from "../../../i18n";
import { CANCELLED, decodeBase64, restCancel, restSend } from "../api";
import { buildRequest } from "../buildRequest";
import { IDLE_SEND, type SendState } from "../components/ResponsePane";
import { detectBody } from "../contentType";
import { historyVars, varMap, type Environment } from "../environments";
import { historyUrl, keptBody, type HistoryEntry } from "../history";
import { recordSend } from "../historyStore";
import { saveRequest } from "../requestsStore";
import { resolveRequest } from "../resolveRequest";
import { foldQuery } from "../syncUrlParams";
import type { RestRequest } from "../types";
import { currentWorkspace, sendSettings, type Workspace } from "../workspace";

interface SendsOptions {
  activeRequest: RestRequest | undefined;
  currentId: string | null;
  /** The request on screen resolved against the environment, or `null` with none open. */
  resolved: { missing: string[]; cyclic: boolean } | null;
  /** Whether the request on screen asks for a value nobody has. */
  blocked: boolean;
  env: Environment | null;
  workspace: Workspace;
}

export interface Sends {
  /** What the request on screen has been answered, or is waiting for. */
  sendState: SendState;
  send: () => Promise<void>;
  cancel: () => void;
  /** Puts away the banner a failed send left on `requestId`. */
  dismissError: (requestId: string) => void;
}

/** Sending the request on screen, cancelling it, and what came back for each request. */
export function useSends({ activeRequest, currentId, resolved, blocked, env, workspace }: SendsOptions): Sends {
  const { t } = useTranslation();
  const [sends, setSends] = useState<Record<string, SendState>>({});
  /**
   * The sends that have gone out and not yet come back, where the unmount cleanup can reach them.
   *
   * A ref and not `sends`, because the cleanup that matters runs once, on the way out, and an
   * effect cleanup closes over the render it was created in — `sends` there would be whatever was
   * on screen when the tab was opened.
   */
  const inFlight = useRef(new Set<string>());

  /**
   * Closing the tab stops whatever it was waiting for.
   *
   * Without this the request runs to its timeout — up to ten minutes — on a connection nobody is
   * watching, and then writes its answer into the history of a tab that is gone. Cancelling covers
   * both: the send comes back as cancelled, and a cancelled send is deliberately not recorded.
   *
   * Nothing is awaited. There is no one left to tell if the cancel itself fails, and the tab is
   * unmounting either way.
   */
  useEffect(
    () => () => {
      for (const id of inFlight.current) void restCancel(id);
      inFlight.current.clear();
    },
    [],
  );

  /**
   * Sends the request on screen.
   *
   * `lastUsedAt` is stamped here and nowhere else — opening a tab to look at a request does not
   * count as using it, which is what keeps Recent's ceiling honest from Phase 2 on.
   */
  async function send() {
    if (!activeRequest || resolved === null || blocked) return;
    // A query still in the URL box goes down into Params now, and is saved that way below.
    const request = foldQuery(activeRequest, () => crypto.randomUUID());
    const sendId = crypto.randomUUID();
    // Resolved again rather than taken from `resolved`: that one was folded with placeholder ids.
    // The answer is the same one the Send button was enabled on.
    const wire = buildRequest(
      resolveRequest(request, varMap(env)).request,
      sendId,
      sendSettings(workspace),
    );
    const startedAt = Date.now();
    /* The history's own URL, built from the request rather than from `wire`: what goes on the wire
       carries the secrets, and the Auth tab's query key is a credential whichever way it was
       typed. */
    const stub = {
      id: crypto.randomUUID(),
      requestId: request.id,
      envName: env?.name ?? "",
      method: request.method,
      url: historyUrl(request, historyVars(env)),
      startedAt,
    } satisfies Partial<HistoryEntry>;

    setSends((prev) => ({
      ...prev,
      [request.id]: {
        ...(prev[request.id] ?? IDLE_SEND),
        phase: "sending",
        sendId,
        sentUrl: wire.url,
        error: null,
      },
    }));
    // `request`, not `resolved.request`. What is stored keeps its variables — writing the resolved
    // copy back would strip a request of the thing that made it portable and, the first time a
    // secret variable was used, would put a credential into `rest-requests.json`.
    saveRequest({ ...request, lastUsedAt: Date.now() });

    inFlight.current.add(sendId);
    try {
      const response = await restSend(wire);
      const bytes = decodeBase64(response.body_base64);
      setSends((prev) => ({
        ...prev,
        [request.id]: {
          phase: "done",
          sendId: null,
          sentUrl: wire.url,
          response,
          bytes,
          detected: detectBody(response.headers, bytes),
          error: null,
        },
      }));
      recordSend({
        ...stub,
        durationMs: Date.now() - startedAt,
        status: response.status,
        statusText: response.status_text,
        size: response.body_size,
        error: null,
        // Read now rather than from the render this send started in: the switch may have been
        // turned off in the minute the server took to answer.
        responseBody: keptBody(
          response.body_base64,
          response.body_size,
          currentWorkspace().keepResponseBodies,
        ),
      });
    } catch (e) {
      // Cancelling is not a failure, and a failure leaves the previous response where it was —
      // the banner says what happened, and the pane still holds what is being compared against.
      const cancelled =
        typeof e === "object" && e !== null && (e as { code?: string }).code === CANCELLED;
      const message = errorMessage(t, e);
      setSends((prev) => ({
        ...prev,
        [request.id]: {
          ...(prev[request.id] ?? IDLE_SEND),
          phase: cancelled ? "cancelled" : "failed",
          sendId: null,
          error: cancelled ? null : message,
        },
      }));
      /* A cancelled send is not recorded: nothing came back and nothing was learned, and an entry
         with neither a status nor an error would be a blank row nobody could read. A timeout or a
         refused connection is an answer, and is kept. */
      if (!cancelled) {
        recordSend({
          ...stub,
          durationMs: Date.now() - startedAt,
          status: null,
          statusText: "",
          size: 0,
          error: message,
          responseBody: null,
        });
      }
    } finally {
      // However it ended, there is nothing left to cancel — and by the time the tab closes this id
      // would name a send that finished long ago.
      inFlight.current.delete(sendId);
    }
  }

  function cancel() {
    const sendId = currentId === null ? null : sends[currentId]?.sendId;
    if (sendId) void restCancel(sendId);
  }

  const sendState = currentId === null ? IDLE_SEND : (sends[currentId] ?? IDLE_SEND);

  function dismissError(requestId: string) {
    setSends((prev) => ({
      ...prev,
      [requestId]: { ...(prev[requestId] ?? IDLE_SEND), error: null },
    }));
  }

  return { sendState, send, cancel, dismissError };
}
