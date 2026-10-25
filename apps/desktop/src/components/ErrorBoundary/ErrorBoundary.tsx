import { Component, type ErrorInfo, type ReactNode } from "react";
import { appLogDir } from "@tauri-apps/api/path";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { logError } from "../../core/log";
import { useTranslation } from "../../i18n";
import styles from "./ErrorBoundary.module.css";

interface Props {
  children: ReactNode;
  /** `"tab"` (default): the rest of the app is still alive, so there is a Try again button.
   *  `"app"`: the whole App failed to render — nothing left to try again into, only a restart and
   *  a way to reach the log. */
  variant?: "tab" | "app";
}

interface State {
  error: Error | null;
}

interface FallbackProps {
  variant: "tab" | "app";
  onReset: () => void;
}

function Fallback({ variant, onReset }: FallbackProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.fallback} role="alert">
      <p className={styles.message}>
        {variant === "app" ? t("error.crashedApp") : t("error.crashedTab")}
      </p>
      <div className={styles.actions}>
        {variant === "tab" ? (
          <button type="button" className={styles.button} onClick={onReset}>
            {t("error.tryAgain")}
          </button>
        ) : (
          <>
            <button
              type="button"
              className={styles.button}
              onClick={() => void appLogDir().then(revealItemInDir)}
            >
              {t("settings.openLogFolder")}
            </button>
            {/* MixLab's own restart and not a plugin's — `tauri-plugin-process` went out with the
                updater it arrived for. `relaunch_app` starts the executable this process was
                started from, which after an update is the one that replaced it. */}
            <button type="button" className={styles.button} onClick={() => void invoke("relaunch_app")}>
              {t("error.restartApp")}
            </button>
          </>
        )}
      </div>
    </div>
  );
}

class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    void logError("react", error, info.componentStack ?? undefined);
  }

  reset = () => {
    this.setState({ error: null });
  };

  render() {
    if (this.state.error) {
      return <Fallback variant={this.props.variant ?? "tab"} onReset={this.reset} />;
    }
    return this.props.children;
  }
}

export default ErrorBoundary;
