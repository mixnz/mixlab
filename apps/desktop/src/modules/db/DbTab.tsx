import { useSavedConnections, useSavedConnectionsLoaded } from "./saved/savedConnectionsStore";
import type { SavedConnection } from "./types";
import ConnectionForm from "./components/ConnectionForm";
import { configFrom } from "./tab/connectionForm";
import Button from "../../components/Button";
import ErrorBanner from "../../components/ErrorBanner";
import NoticeBanner from "../../components/NoticeBanner";
import { useTranslation } from "../../i18n";
import type { ModuleTabProps } from "../../shell/module";
import { useTabBadges } from "./tab/useTabBadges";
import { useConnection } from "./tab/useConnection";
import { useConnectionForm } from "./tab/useConnectionForm";
import { useTabArrival } from "./tab/useTabArrival";
import { useSavedList } from "./tab/useSavedList";
import SavedConnectionList from "./tab/SavedConnectionList";
import SavedConnectionMenu from "./tab/SavedConnectionMenu";
import ConnectionWorkspace from "./tab/ConnectionWorkspace";
/* The module's own global stylesheet: the connection form, the saved list and the three
   workspaces. Component-scoped rules live in each component's CSS Module. */
import "./db.css";

/**
 * A database tab: the saved list and the connection form, or the workspace of the connection it
 * opened. What it does lives in the hooks under `tab/`; this is the one choice between the two.
 */
function DbTab({ active, onTitleChange, onBadgesChange, restored, onStateChange }: ModuleTabProps) {
  const { t } = useTranslation();
  const savedConnections = useSavedConnections();
  const connection = useConnection({ onTitleChange, onStateChange });
  const { connectionId, status, error, setError, tunnelStatus, connecting } = connection;
  const savedConnectionsLoaded = useSavedConnectionsLoaded();
  const editor = useConnectionForm({
    savedConnections,
    savedConnectionsLoaded,
    onTitleChange,
    onStateChange,
    onReplaced: connection.clearFeedback,
  });
  const { form, editingId, saveAsName, stale } = editor;
  /* The tab reads three of the eighteen for itself: the kind and host name a connection in the
     title, the database is what a workspace opens on, and the tunnel decides who reports a lost
     connection. The rest of the eighteen belongs to the form and stays there. */
  const { kind, uri, database, tunnelType } = form;
  const list = useSavedList({ savedConnections, savedConnectionsLoaded, connectionId, editingId });

  /**
   * Whether the tab bar should be showing a lock for this tab.
   *
   * Only once connected: before that the mark belongs to the row in the sidebar, and the form on
   * screen may be for another connection entirely. Read off the store rather than remembered from
   * the click, so clearing the flag in one tab takes the lock off this one too.
   */
  const activeReadOnly = Boolean(connectionId && editor.savedEntry?.readOnly);

  useTabBadges({ connectionId, kind, readOnly: activeReadOnly, onBadgesChange });

  /** Connect, from what the form holds. */
  function connectFromForm() {
    // Read the form before the flag goes up: anything that threw between the two would leave the
    // flag raised with no `finally` to lower it, and the tab stuck on "Connecting…" for good.
    connection.connect({
      config: configFrom(form),
      keyringRef: form.keyringRef,
      savedId: editingId,
      name: saveAsName,
    });
  }

  function openAndConnect(entry: SavedConnection) {
    editor.applySaved(entry);
    connection.connect({
      config: entry.config,
      keyringRef: entry.keyringRef ?? null,
      savedId: entry.id,
      title: entry.name,
      name: saveAsName,
    });
  }

  const disconnect = () => connection.disconnect(editor.savedEntry);

  useTabArrival({
    restored,
    savedConnections,
    savedConnectionsLoaded,
    editor,
    connection,
    openAndConnect,
    onTitleChange,
    onStateChange,
  });

  if (!connectionId) {
    return (
      <div className="login-view">
        <SavedConnectionList
          list={list}
          savedConnections={savedConnections}
          editingId={editingId}
          onSelect={editor.applySaved}
          onOpen={openAndConnect}
          onNew={editor.newForm}
        />
        <section className="login-form">
          <ConnectionForm
            form={form}
            onChange={editor.set}
            onKindChange={editor.changeKind}
            editingId={editingId}
            name={saveAsName}
            onNameChange={editor.setSaveAsName}
            saveDisabled={editor.saveDisabled}
            status={status}
            connecting={connecting}
            tunnelStatus={tunnelStatus}
            focusPassword={editor.focusPassword}
            notices={
              <>
                {error && <ErrorBanner message={error} onDismiss={() => setError("")} />}
                {stale === "changed" && (
                  <div className="saved-elsewhere">
                    <NoticeBanner message={t("connection.changedElsewhere")} />
                    <Button
                      size="small"
                      onClick={() => {
                        if (editor.savedEntry) editor.reloadSaved(editor.savedEntry);
                      }}
                    >
                      {t("connection.loadNewVersion")}
                    </Button>
                  </div>
                )}
                {stale === "removed" && (
                  <NoticeBanner message={t("connection.removedElsewhere")} onDismiss={editor.dismissStale} />
                )}
              </>
            }
            onSave={editor.save}
            onSaveAsNew={editor.saveAsNew}
            onConnect={connectFromForm}
            onTestTunnel={() => connection.testTunnel(configFrom(form).ssh)}
          />
        </section>
        <SavedConnectionMenu list={list} savedConnections={savedConnections} onDelete={editor.deleteSaved} />
      </div>
    );
  }

  return (
    <ConnectionWorkspace
      active={active}
      kind={kind}
      connectionId={connectionId}
      database={database}
      uri={uri}
      tunnelled={tunnelType === "ssh"}
      status={status}
      error={error}
      saved={editor.savedEntry}
      onDisconnect={disconnect}
    />
  );
}

export default DbTab;
