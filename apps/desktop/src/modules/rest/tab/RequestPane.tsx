import type { Ref } from "react";
import { Tab, TabStrip, tabKeyDown } from "../../../components/TabStrip";
import { useTranslation } from "../../../i18n";
import AuthPane from "../components/AuthPane";
import BodyEditor from "../components/BodyEditor";
import KeyValueTable from "../components/KeyValueTable";
import UrlBar from "../components/UrlBar";
import UrlPreview from "../components/UrlPreview";
import type { ResolvedRequest } from "../resolveRequest";
import type { RestRequest } from "../types";

/** Which of the request's parts the pane is showing. */
export type RequestTabKey = "params" | "body" | "headers" | "auth";

interface RequestPaneProps {
  request: RestRequest;
  /** The pane's share of the two panes. */
  ratio: number;
  urlRef: Ref<HTMLInputElement>;
  sending: boolean;
  blocked: boolean;
  /** The URL as the line below the box shows it. */
  preview: string | null;
  resolved: ResolvedRequest | null;
  envName: string;
  requestTab: RequestTabKey;
  onRequestTabChange: (key: RequestTabKey) => void;
  /** An edit to the request. Everything in the pane goes through this. */
  onEdit: (patch: Partial<RestRequest>) => void;
  onUrlChange: (url: string) => void;
  onPasteText: (text: string) => boolean;
  onSend: () => void;
  onCancel: () => void;
  onAddMissing: () => void;
}

/**
 * The request on screen, as it is edited: the URL bar, the line under it, and Params, Body,
 * Headers and Auth.
 *
 * Drawn only while a request is open, so it holds no state: which part each request shows is kept
 * by `RestTab`, per request, and outlives this pane.
 */
function RequestPane({
  request,
  ratio,
  urlRef,
  sending,
  blocked,
  preview,
  resolved,
  envName,
  requestTab,
  onRequestTabChange,
  onEdit,
  onUrlChange,
  onPasteText,
  onSend,
  onCancel,
  onAddMissing,
}: RequestPaneProps) {
  const { t } = useTranslation();
  const paneTabs: { key: RequestTabKey; label: string }[] = [
    { key: "params", label: t("rest.paramsTab") },
    { key: "body", label: t("rest.bodyTab") },
    { key: "headers", label: t("rest.requestHeadersTab") },
    { key: "auth", label: t("rest.authTab") },
  ];

  return (
    <section className="rest-request-pane" style={{ flex: `0 0 ${ratio * 100}%` }}>
      <UrlBar
        inputRef={urlRef}
        method={request.method}
        url={request.url}
        sending={sending}
        blocked={blocked}
        onMethodChange={(method) => onEdit({ method })}
        onUrlChange={onUrlChange}
        onPasteText={onPasteText}
        onSend={onSend}
        onCancel={onCancel}
      />
      {preview !== null && resolved !== null && (
        <UrlPreview
          preview={preview}
          missing={resolved.missing}
          cyclic={resolved.cyclic}
          envName={envName}
          onAddMissing={onAddMissing}
        />
      )}
      <TabStrip size="small" role="tablist">
        {paneTabs.map((tab) => {
          const pick = () => onRequestTabChange(tab.key);
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
          <KeyValueTable rows={request.params} onChange={(params) => onEdit({ params })} />
        )}
        {requestTab === "body" && (
          <BodyEditor body={request.body} onChange={(body) => onEdit({ body })} />
        )}
        {requestTab === "headers" && (
          <KeyValueTable rows={request.headers} onChange={(headers) => onEdit({ headers })} />
        )}
        {requestTab === "auth" && (
          <AuthPane
            auth={request.auth}
            headers={request.headers}
            params={request.params}
            onChange={(auth) => onEdit({ auth })}
          />
        )}
      </div>
    </section>
  );
}

export default RequestPane;
