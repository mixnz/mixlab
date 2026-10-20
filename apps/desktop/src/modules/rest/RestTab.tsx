import { useEffect, useMemo, useRef, useState } from "react";
import ConfirmDialog from "../../components/ConfirmDialog";
import Splitter from "../../components/Splitter";
import { Tab, TabStrip, tabKeyDown } from "../../components/TabStrip";
import { useShortcut } from "../../core/shortcuts";
import type { ModuleTabProps } from "../../shell/module";
import { useTranslation } from "../../i18n";
import type { ViewMode } from "./contentType";
import { SECRET_MASK, previewVars, varMap } from "./environments";
import { addVariables, useEnvironments } from "./environmentsStore";
import { interpolate } from "./interpolate";
import { resolveRequest } from "./resolveRequest";
import AuthPane from "./components/AuthPane";
import BodyEditor from "./components/BodyEditor";
import EnvironmentDialog from "./components/EnvironmentDialog";
import EnvironmentSelect from "./components/EnvironmentSelect";
import HistoryDialog from "./components/HistoryDialog";
import ResponsePane from "./components/ResponsePane";
import KeyValueTable from "./components/KeyValueTable";
import RequestList from "./components/RequestList";
import RequestTabs from "./components/RequestTabs";
import UrlBar from "./components/UrlBar";
import UrlPreview from "./components/UrlPreview";
import { shortUrl } from "./format";
import {
  addRequest,
  createRequest,
  deleteRequest,
  pinRequest,
  saveRequest,
  useRequestLists,
} from "./requestsStore";
import { foldQuery, urlWithParams } from "./syncUrlParams";
import type { RestRequest } from "./types";
import { useWorkspace } from "./workspace";
import { useEnvironmentChoice } from "./tab/useEnvironmentChoice";
import { useOpenRequests } from "./tab/useOpenRequests";
import { usePaneSizes } from "./tab/usePaneSizes";
import { usePasteRequest } from "./tab/usePasteRequest";
import { useSends } from "./tab/useSends";
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
  const [preferredModes, setPreferredModes] = useState<Record<string, ViewMode>>({});
  const [headersOpen, setHeadersOpen] = useState<Record<string, boolean>>({});

  const { width, ratio, panesRef, sidebarSplitter, paneSplitter } = usePaneSizes(workspace);
  const urlRef = useRef<HTMLInputElement>(null);
  /** The request whose URL box is still owed the keyboard — set by New, cleared once given. */
  const [focusUrlFor, setFocusUrlFor] = useState<string | null>(null);

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

  const { sendState, send, cancel, dismissError } = useSends({
    activeRequest,
    currentId,
    resolved,
    blocked,
    env,
    workspace,
  });
  const { editUrl, pasteIntoUrl, swap, setSwap } = usePasteRequest({ active, activeRequest, env, open });

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

  const requestTab = currentId === null ? "params" : (requestTabs[currentId] ?? "params");

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
                onDismissError={() => dismissError(activeRequest.id)}
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
