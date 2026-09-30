import type { TranslationKey } from "../i18n";

/**
 * Turning what a failed command threw into the sentence the user reads.
 *
 * Every backend command rejects with an `AppError` — `{ code, params, cause? }`, see
 * `src-tauri/src/error.rs` — rather than with a finished English message, because the message has
 * to exist in both languages and only the frontend knows which one is on.
 */

/** The shape a rejected `invoke` carries. */
export interface AppError {
  /** A key under `error.*` in the dictionaries. */
  code: string;
  params?: Record<string, string>;
  /** The failure this one wraps, e.g. the server's reason under "Row 3: …". */
  cause?: AppError;
}

function isAppError(value: unknown): value is AppError {
  return typeof value === "object" && value !== null && typeof (value as AppError).code === "string";
}

type Translate = (key: TranslationKey, vars?: Record<string, string | number>) => string;

/**
 * The message for a rejected command, in the current language.
 *
 * An error MixLab doesn't recognise — anything thrown that isn't an `AppError`, which in practice
 * means a bug in the webview rather than a failure in the backend — is shown as its own text
 * instead of being swallowed: something unreadable on screen beats nothing on screen.
 */
export function errorMessage(t: Translate, error: unknown): string {
  if (!isAppError(error)) return t("error.unknown", { message: String(error) });
  const params: Record<string, string> = { ...error.params };
  // The wrapped failure is translated first and handed to the outer message as `{{cause}}`, so
  // both halves of "Row 3: MySQL said …" come out in the same language.
  if (error.cause) params.cause = errorMessage(t, error.cause);
  return t(error.code as TranslationKey, params);
}
