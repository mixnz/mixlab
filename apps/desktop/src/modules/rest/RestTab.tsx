import { useEffect, useEffectEvent, useMemo, useRef, useState } from "react";
import ConfirmDialog from "../../components/ConfirmDialog";
import Splitter from "../../components/Splitter";
import { Tab, TabStrip, tabKeyDown } from "../../components/TabStrip";
import { errorMessage } from "../../core/errors";
import { modalDepth, useShortcut } from "../../core/shortcuts";
import type { ModuleTabProps } from "../../shell/module";
import { useTranslation } from "../../i18n";
import { CANCELLED, decodeBase64, restCancel, restSend } from "./api";
import { buildRequest } from "./buildRequest";
import { detectBody, type ViewMode } from "./contentType";
import { SECRET_MASK, historyVars, previewVars, varMap } from "./environments";
import { addVariables, useEnvironments } from "./environmentsStore";
import { historyUrl, keptBody, type HistoryEntry } from "./history";
import { recordSend } from "./historyStore";
import { interpolate } from "./interpolate";
import { resolveRequest } from "./resolveRequest";
import { findSubstitutions, substitute, type Substitution } from "./substitute";
import AuthPane from "./components/AuthPane";
import BodyEditor from "./components/BodyEditor";
import EnvironmentDialog from "./components/EnvironmentDialog";
import EnvironmentSelect from "./components/EnvironmentSelect";
import HistoryDialog from "./components/HistoryDialog";
import ResponsePane, { IDLE_SEND, type SendState } from "./components/ResponsePane";
import KeyValueTable from "./components/KeyValueTable";
import RequestList from "./components/RequestList";
import RequestTabs from "./components/RequestTabs";
import UrlBar from "./components/UrlBar";
import UrlPreview from "./components/UrlPreview";
import { shortUrl } from "./format";
import { parsePaste } from "./parsePaste";
import { isBlank } from "./requests";
import {
  addRequest,
  createRequest,
  deleteRequest,
  pasteOverBlank,
  pasteRequest,
  pinRequest,
  saveRequest,
  useRequestLists,
} from "./requestsStore";
import { foldQuery, replaceQuery, urlWithParams } from "./syncUrlParams";
import type { RestRequest } from "./types";
import { currentWorkspace, sendSettings, useWorkspace } from "./workspace";
import { useEnvironmentChoice } from "./tab/useEnvironmentChoice";
import { useOpenRequests } from "./tab/useOpenRequests";
import { usePaneSizes } from "./tab/usePaneSizes";
import "./rest.css";

type RequestTabKey = "params" | "body" | "headers" | "auth";

/**
 * The REST client's workspace: the request list, the requests open on it, and the pane each one
 * is edited and answered in.
 *
 * **Nothing here is a draft.** Every edit is written straight through to the request in the shared
 * store, so closing a tab loses nothing, two REST tabs cannot overwrite each other, and there is
 * no Save button, no dirty mark and no dialog asking whether to keep anything. Which requests are
 * open is the only state that lives in this component, and it is the only state the app does not
 * remember — the shell keeps no tabs either.
 */
function RestTab({ active, onTitleChange, restored, onStateChange }: ModuleTabProps) {
  const { t } = useTranslation();
  const lists = useRequestLists();
  const workspace = useWorkspace();
  const environments = useEnvironments();
  const [envDialogOpen, setEnvDialogOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  const { tabs, activeRequest, currentId, setActiveId, open, close, reorder } = useOpenRequests({
    lists,
    restored,
    onStateChange,
  });
  const [requestTabs, setRequestTabs] = useState<Record<string, RequestTabKey>>({});
  const [sends, setSends] = useState<Record<string, SendState>>({});
  /**
   * The sends that have gone out and not yet come back, where the unmount cleanup can reach them.
   *
   * A ref and not `sends`, because the cleanup that matters runs once, on the way out, and an
   * effect cleanup closes over the render it was created in — `sends` there would be whatever was
   * on screen when the tab was opened.
   */
  const inFlight = useRef(new Set<string>());
  const [preferredModes, setPreferredModes] = useState<Record<string, ViewMode>>({});
  const [headersOpen, setHeadersOpen] = useState<Record<string, boolean>>({});

  const { width, ratio, panesRef, sidebarSplitter, paneSplitter } = usePaneSizes(workspace);
  const urlRef = useRef<HTMLInputElement>(null);
  /** The request whose URL box is still owed the keyboard — set by New, cleared once given. */
  const [focusUrlFor, setFocusUrlFor] = useState<string | null>(null);
  /** A paste that the environment has names for, and the request it would become. Held rather
   *  than applied: the question is put to whoever pasted it, and both answers are cheap. */
  const [swap, setSwap] = useState<{ request: RestRequest; found: Substitution[] } | null>(null);
  /** Set by a paste into the URL box that was not a whole request: the query that paste brings
   *  replaces the ticked Params rows at once rather than waiting for Send. */
  const pastedIntoUrl = useRef(false);

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

  const { env, chooseEnv } = useEnvironmentChoice(workspace, environments);

  const label = (request: RestRequest) =>
    request.name !== "" ? request.name : shortUrl(request.url) || t("rest.untitled");

  /* The request as Send would fold it: a query still sitting in the URL box counts as the Params
     rows it is about to become. The ids are placeholders — nothing here is saved, and `send` folds
     again with real ones. */
  const folded = useMemo(() => {
    if (activeRequest === undefined) return undefined;
    let n = 0;
    return foldQuery(activeRequest, () => `pending-${++n}`);
  }, [activeRequest]);
  /* Resolved once per render rather than at the moment of sending, so that the line under the URL
     box and the state of the Send button are two readings of one answer and cannot disagree. */
  const resolved = useMemo(
    () => (folded === undefined ? null : resolveRequest(folded, varMap(env))),
    [folded, env],
  );
  /** The URL as the line below the box shows it: the ticked Params as its query, secrets as dots,
   *  anything unfilled still in its braces. Drawn with no environment too — the box holds no
   *  query, so this is the only place the whole URL can be read. */
  const preview = useMemo(
    () =>
      folded === undefined
        ? null
        : interpolate(urlWithParams(folded.url, folded.params), previewVars(env) ?? {}).text,
    [folded, env],
  );
  /** A request that asks for a value nobody has does not go out. Sending `{{token}}` as those nine
   *  characters helps nobody, and a server's answer to it is not an answer to anything. */
  const blocked = resolved !== null && (resolved.missing.length > 0 || resolved.cyclic);

  /* The shell's tab says what the tab is, not what is open in it.

     It used to carry the active request's name — which is a name this module already draws, on
     the request strip inside, beside the other requests that are open. Up on the shell strip it
     had no such company: it sat between a connection and a terminal saying "users", and nothing
     said which of the app's three modules it belonged to. One word now, with the globe the shell
     puts beside a tab that reports no badges of its own — the pair the [+] menu opens it from. */
  const title = t("app.moduleRest");
  useEffect(() => {
    onTitleChange(title);
  }, [title, onTitleChange]);

  useEffect(() => {
    if (focusUrlFor === null || activeRequest?.id !== focusUrlFor) return;
    urlRef.current?.focus();
    setFocusUrlFor(null);
  }, [focusUrlFor, activeRequest]);

  function makeRequest() {
    const request = createRequest();
    open(request.id);
    // The URL is the only thing a new request needs, so that is where the keyboard goes. Asked for
    // by id and not done here: the box does not exist until the tab this just opened has rendered.
    setFocusUrlFor(request.id);
  }

  function duplicate(request: RestRequest) {
    const copy: RestRequest = {
      ...structuredClone(request),
      id: crypto.randomUUID(),
      name: t("rest.copySuffix", { name: label(request) }),
      createdAt: Date.now(),
      lastUsedAt: Date.now(),
      // A copy is something someone chose to have, whatever the original was.
      origin: "manual",
    };
    addRequest(copy);
    open(copy.id);
  }

  /** An edit to the request on screen. Everything in the request pane goes through this. */
  function edit(patch: Partial<RestRequest>) {
    if (!activeRequest) return;
    saveRequest({ ...activeRequest, ...patch });
  }

  /** The URL box changed. A query typed into it stays there until Send folds it into Params; one
   *  that arrived by paste takes the place of the ticked rows now. */
  function editUrl(url: string) {
    if (!activeRequest) return;
    const next = { ...activeRequest, url };
    if (pastedIntoUrl.current) {
      pastedIntoUrl.current = false;
      saveRequest(replaceQuery(next, () => crypto.randomUUID()));
      return;
    }
    saveRequest(next);
  }

  /** A paste into the URL box: a whole request is taken by `pasteInto`; anything else lands in the
   *  box as text, and the edit it makes goes through `replaceQuery`. The flag is dropped after the event either way,
   *  so a paste that changed nothing does not fold the next keystroke. */
  function pasteIntoUrl(text: string): boolean {
    if (pasteInto(text)) return true;
    pastedIntoUrl.current = true;
    setTimeout(() => {
      pastedIntoUrl.current = false;
    }, 0);
    return false;
  }

  /**
   * A paste in the URL box that turned out to be a cURL command.
   *
   * A tab nobody has put anything into is filled where it stands, and the request moves to Recent
   * with it — pressing New to have somewhere to paste into is not a decision to keep the result. A
   * tab with anything in it is left alone and the paste opens a tab of its own, which destroys
   * nothing and so needs no undo.
   *
   * Returns whether the paste was taken, which is what stops the box also receiving it.
   *
   * A command copied out of a browser has the host, the token and the api key written into it, and
   * those are the very things the environment beside it exists to hold. Where they match, the
   * offer to put the variables back is made — asked rather than done, because the values may be
   * the whole reason this particular command was pasted.
   */
  function pasteInto(text: string): boolean {
    const parsed = parsePaste(text, () => crypto.randomUUID());
    if (parsed === null) return false;
    const filled =
      activeRequest !== undefined && isBlank(activeRequest)
        ? pasteOverBlank(activeRequest, parsed)
        : pasteRequest(parsed);
    // The same id when the husk was filled in place; a different one when a duplicate was found in
    // Recent, and then it is that row's tab which comes forward.
    open(filled.id);
    if (env !== null) {
      const found = findSubstitutions(filled, env);
      if (found.length > 0) setSwap({ request: substitute(filled, env), found });
    }
    return true;
  }

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

  /**
   * The other way in: pasting a command with no request open at all.
   *
   * Without this the only paste target is a URL box, so a command could not be pasted without
   * pressing New first — and since a blank request is filled where it stands, that first paste
   * would never reach Recent. The empty screen says both ways in; this is the one it names first.
   *
   * Listened for on the document because nothing in an empty workspace holds the keyboard, so the
   * event has no element of ours to fire on. Which is also why the guards are needed: a paste aimed
   * at a text box is that box's, and a dialog or menu that is up holds the keyboard the same way it
   * holds every shortcut.
   */
  // `pasteInto` is remade every render; through an effect event the listener always runs the
  // current one without being re-subscribed for it.
  const pasteText = useEffectEvent((text: string) => pasteInto(text));
  useEffect(() => {
    if (!active || activeRequest !== undefined) return;
    function onPaste(event: ClipboardEvent) {
      if (modalDepth() !== 0) return;
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || target.closest("input, textarea") !== null)
      ) {
        return;
      }
      if (pasteText(event.clipboardData?.getData("text") ?? "")) event.preventDefault();
    }
    document.addEventListener("paste", onPaste);
    return () => document.removeEventListener("paste", onPaste);
  }, [active, activeRequest]);

  const requestTab = currentId === null ? "params" : (requestTabs[currentId] ?? "params");
  const sendState = currentId === null ? IDLE_SEND : (sends[currentId] ?? IDLE_SEND);

  /* `active` — the prop, not the open request — is what keeps the REST tabs behind this one
     quiet: all of them stay mounted, and all of them would otherwise answer the same keystroke. */
  useShortcut(
    "rest.send",
    () => void send(),
    active && activeRequest !== undefined && sendState.phase !== "sending" && !blocked,
  );
  useShortcut("rest.newRequest", makeRequest, active);
  // Only while there is a request tab to close — otherwise the chord is the shell's, as before.
  useShortcut(
    "rest.closeRequest",
    () => currentId !== null && close(currentId),
    active && currentId !== null,
  );
  useShortcut("rest.history", () => setHistoryOpen(true), active);

  const paneTabs: { key: RequestTabKey; label: string }[] = [
    { key: "params", label: t("rest.paramsTab") },
    { key: "body", label: t("rest.bodyTab") },
    { key: "headers", label: t("rest.requestHeadersTab") },
    { key: "auth", label: t("rest.authTab") },
  ];

  return (
    <div className="rest-tab">
      <aside className="rest-sidebar" style={{ width }}>
        <RequestList
          lists={lists}
          activeId={currentId}
          vars={varMap(env)}
          onOpen={open}
          onNew={makeRequest}
          onHistory={() => setHistoryOpen(true)}
          onSave={saveRequest}
          onDuplicate={duplicate}
          onPin={pinRequest}
          onDelete={deleteRequest}
        />
      </aside>

      <Splitter
        orientation="vertical"
        ariaLabel={t("rest.resizeSidebar")}
        title={t("rest.resizeSidebar")}
        {...sidebarSplitter}
      />

      <div className="rest-main">
        <div className="rest-tabs-row">
          <RequestTabs
            className="rest-tabs-strip"
            tabs={tabs}
            activeId={currentId}
            onSelect={setActiveId}
            onClose={close}
            onReorder={reorder}
            onNew={makeRequest}
            label={label}
          />
          <div className="rest-env">
            <EnvironmentSelect
              environments={environments}
              value={env?.id ?? null}
              onChange={chooseEnv}
              onManage={() => setEnvDialogOpen(true)}
            />
          </div>
        </div>

        {activeRequest === undefined ? (
          <p className="rest-empty muted">{t("rest.emptyMain")}</p>
        ) : (
          <div className="rest-panes" ref={panesRef}>
            <section className="rest-request-pane" style={{ flex: `0 0 ${ratio * 100}%` }}>
              <UrlBar
                inputRef={urlRef}
                method={activeRequest.method}
                url={activeRequest.url}
                sending={sendState.phase === "sending"}
                blocked={blocked}
                onMethodChange={(method) => edit({ method })}
                onUrlChange={editUrl}
                onPasteText={pasteIntoUrl}
                onSend={() => void send()}
                onCancel={cancel}
              />
              {preview !== null && resolved !== null && (
                <UrlPreview
                  preview={preview}
                  missing={resolved.missing}
                  cyclic={resolved.cyclic}
                  envName={env?.name ?? ""}
                  onAddMissing={() => {
                    // Only offered when something is missing, which takes an environment.
                    if (env === null) return;
                    addVariables(env.id, resolved.missing);
                    setEnvDialogOpen(true);
                  }}
                />
              )}
              <TabStrip size="small" role="tablist">
                {paneTabs.map((tab) => {
                  const pick = () =>
                    setRequestTabs((prev) => ({ ...prev, [activeRequest.id]: tab.key }));
                  return (
                    <Tab
                      key={tab.key}
                      active={requestTab === tab.key}
                      role="tab"
                      aria-selected={requestTab === tab.key}
                      tabIndex={0}
                      onClick={pick}
                      onKeyDown={tabKeyDown(pick)}
                    >
                      {tab.label}
                    </Tab>
                  );
                })}
              </TabStrip>
              <div className="rest-pane-body">
                {requestTab === "params" && (
                  <KeyValueTable rows={activeRequest.params} onChange={(params) => edit({ params })} />
                )}
                {requestTab === "body" && (
                  <BodyEditor body={activeRequest.body} onChange={(body) => edit({ body })} />
                )}
                {requestTab === "headers" && (
                  <KeyValueTable rows={activeRequest.headers} onChange={(headers) => edit({ headers })} />
                )}
                {requestTab === "auth" && (
                  <AuthPane
                    auth={activeRequest.auth}
                    headers={activeRequest.headers}
                    params={activeRequest.params}
                    onChange={(auth) => edit({ auth })}
                  />
                )}
              </div>
            </section>

            <Splitter
              orientation="vertical"
              ariaLabel={t("rest.resizePanes")}
              title={t("rest.resizePanes")}
              {...paneSplitter}
            />

            <section className="rest-response-pane">
              <ResponsePane
                state={sendState}
                preferred={currentId === null ? "preview" : (preferredModes[currentId] ?? "preview")}
                onPreferredChange={(mode) =>
                  setPreferredModes((prev) => ({ ...prev, [activeRequest.id]: mode }))
                }
                headersOpen={headersOpen[activeRequest.id] ?? false}
                onHeadersOpenChange={(open) =>
                  setHeadersOpen((prev) => ({ ...prev, [activeRequest.id]: open }))
                }
                onDismissError={() =>
                  setSends((prev) => ({
                    ...prev,
                    [activeRequest.id]: { ...(prev[activeRequest.id] ?? IDLE_SEND), error: null },
                  }))
                }
              />
            </section>
          </div>
        )}
      </div>

      {envDialogOpen && (
        <EnvironmentDialog initialId={env?.id ?? null} onClose={() => setEnvDialogOpen(false)} />
      )}

      {historyOpen && (
        <HistoryDialog onOpenRequest={open} onClose={() => setHistoryOpen(false)} />
      )}

      {swap !== null && (
        <ConfirmDialog
          title={t("rest.swapTitle")}
          message={t("rest.swapMessage", { env: env?.name ?? "" })}
          confirmLabel={t("rest.swapConfirm")}
          cancelLabel={t("rest.swapCancel")}
          onConfirm={() => {
            saveRequest(swap.request);
            setSwap(null);
          }}
          onCancel={() => setSwap(null)}
        >
          <ul className="rest-swap-list">
            {swap.found.map((item) => (
              <li key={item.name} className="rest-swap-row">
                <code className="rest-swap-name">{`{{${item.name}}}`}</code>
                {/* A secret's value is dots here for the same reason it is dots under the URL box:
                    the question is which variable, and the answer never needs the credential. */}
                <span className="rest-swap-value">{item.secret ? SECRET_MASK : item.value}</span>
                <span className="rest-swap-count">
                  {t("rest.swapCount", { count: item.count })}
                </span>
              </li>
            ))}
          </ul>
        </ConfirmDialog>
      )}
    </div>
  );
}

export default RestTab;
