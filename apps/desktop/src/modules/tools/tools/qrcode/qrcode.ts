/**
 * Wraps `qrcode-generator` (the original reference library, 0 dependencies) as a pure function:
 * text in, grid out. The Panel draws the canvas from the grid itself — not using the library's
 * built-in `createDataURL`, to be free to change module colours/styles.
 */
import qrcodeGenerator from "qrcode-generator";

export type ErrorCorrectionLevel = "L" | "M" | "Q" | "H";

export interface QrGrid {
  size: number;
  isDark: (row: number, col: number) => boolean;
}

/** `typeNumber = 0` lets the library pick the smallest QR version that fits `text`.
 *  Text exceeding even version 40 (the largest) makes `make()` throw — caught and returned as
 *  `null` instead of letting the Panel crash, just as `radix`/`diff` report "cannot read" instead
 *  of throwing an exception outwards. */
export function encodeQr(text: string, level: ErrorCorrectionLevel): QrGrid | null {
  const qr = qrcodeGenerator(0, level);
  qr.addData(text);
  try {
    qr.make();
  } catch {
    return null;
  }
  return { size: qr.getModuleCount(), isDark: (row, col) => qr.isDark(row, col) };
}
