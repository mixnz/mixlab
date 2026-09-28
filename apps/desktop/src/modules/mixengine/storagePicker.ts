import type { StorageReport } from "@mixengine/api";

import { joinPath, PATH_STYLE, type PathStyle } from "../../core/paths";
import type { ChosenPaths } from "./api";

/**
 * Cổng vào biến câu trả lời `--storage` thành mấy dòng để vẽ, và mấy dòng đó thành cờ khởi động.
 *
 * Thuần và không gọi gì — cùng lý do `daemonState.ts` ở đây chứ không nằm trong component: luật
 * đáng có test là *khoá nào đi vào dòng lệnh*, và một `useState` thì không test được. Gửi thừa một
 * khoá không làm hỏng gì (daemon coi giá trị trùng là no-op im lặng), nhưng nó là nói ba câu mình
 * không có ý nói, và nó làm `config.toml` bị ghi lại vì một lần bấm không đổi gì.
 */

/** Bốn khoá `[paths]` có thể dời, đúng thứ tự tệp cấu hình liệt kê. */
export const KEYS = ["runtimes", "packages", "data", "logs"] as const;

export type StorageKey = (typeof KEYS)[number];

/** Một dòng của bảng chọn. Chỉ những gì bảng vẽ. */
export interface StorageRow {
  key: StorageKey;
  /** Chỗ nó đang ở, theo daemon. */
  current: string;
  /** Nó có nằm ngoài home không — daemon trả lời, không phải bên này so chuỗi. */
  relocated: boolean;
  /** Chỗ người dùng vừa chọn, hoặc `null` khi họ chưa chọn gì. */
  picked: string | null;
}

/** Bốn dòng từ một câu trả lời, chưa ai chọn gì. */
export function rowsFrom(report: StorageReport): StorageRow[] {
  return KEYS.map((key) => ({
    key,
    current: report.paths[key].path,
    relocated: report.paths[key].relocated,
    picked: null,
  }));
}

/** Một dòng sau khi người dùng chọn một thư mục cho nó. */
export function pick(rows: StorageRow[], key: StorageKey, directory: string): StorageRow[] {
  return rows.map((row) => (row.key === key ? { ...row, picked: directory } : row));
}

/**
 * Bốn dòng sau khi người dùng chọn **một** thư mục cho cả bốn.
 *
 * `<thư mục>\runtimes`, `<thư mục>\packages`, … — trường hợp thường gặp là "để hết lên ổ kia", và
 * bắt người ta bấm bốn lần cho một ý định là bắt họ làm việc của máy. Nối bằng dấu phân cách của hệ
 * điều hành, giống giá trị nút chọn từng dòng gửi đi: daemon ghi `config.toml` qua `toml_edit`, vốn
 * tự thoát dấu gạch ngược (T191).
 */
export function oneFolderFor(
  rows: StorageRow[],
  directory: string,
  style: PathStyle = PATH_STYLE,
): StorageRow[] {
  return rows.map((row) => ({ ...row, picked: joinPath(directory, row.key, style) }));
}

/**
 * Những khoá cần gửi đi, và chỉ những khoá đó.
 *
 * Một dòng chưa ai chọn không được gửi. Một dòng người ta chọn đúng chỗ nó đang ở cũng không —
 * daemon sẽ coi là no-op, nhưng không nhờ vào điều đó: cái được gửi nên là *cái đã đổi*, để lời
 * mình nói với daemon đúng bằng điều mình có ý nói.
 *
 * `undefined` khi không có gì đổi, vì đó là thứ `startDaemon` nhận cho "không chọn gì" — và một
 * object rỗng thì frontend đọc là "có chọn" trong khi nó không.
 */
export function chosenFrom(rows: StorageRow[]): ChosenPaths | undefined {
  const chosen: ChosenPaths = {};
  let any = false;

  for (const row of rows) {
    if (row.picked !== null && row.picked !== row.current) {
      chosen[row.key] = row.picked;
      any = true;
    }
  }

  return any ? chosen : undefined;
}

/** Quyền chọn còn mở không — daemon trả lời, bên này không suy ra từ đường dẫn. */
export function isFree(report: StorageReport): boolean {
  return report.changeable.changeable === "free";
}

/**
 * Câu daemon nói về những gì đã cài, hoặc `null` khi chưa cài gì.
 *
 * Câu của daemon chứ không phải câu bên này dựng: *cái gì đã được cài* là một phép đo nó vừa làm,
 * và một client viết lại câu đó là một câu trả lời thứ hai cho cùng một câu hỏi — cùng luật màn
 * hình Doctor và Uninstall đang theo.
 */
export function explanationOf(report: StorageReport): string | null {
  return report.changeable.changeable === "taken" ? report.changeable.explanation : null;
}
