import { useEffect, useRef, useState } from "react";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { CheckIcon, CopyIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { copyImage } from "../../../../core/clipboard";
import { encodeQr, type ErrorCorrectionLevel } from "./qrcode";
import styles from "./Panel.module.css";

type ModuleStyle = "square" | "rounded" | "dots" | "connected" | "classy";
type Grid = { size: number; isDark: (row: number, col: number) => boolean };
type Corners = [number, number, number, number];

const COPIED_MS = 1500;
const MIN_CELL = 2;
const MAX_CELL = 32;
const MIN_MARGIN = 0;
const MAX_MARGIN = 20;

function canvasToBlob(canvas: HTMLCanvasElement): Promise<Blob | null> {
  return new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
}

/** Dark modules in the 4 orthogonal directions around (row, col) — the basis for both styles below
 *  that round according to neighbouring modules, "Connected" and "Classy". */
function neighborsOf(grid: Grid, row: number, col: number) {
  const dark = (r: number, c: number) => r >= 0 && r < grid.size && c >= 0 && c < grid.size && grid.isDark(r, c);
  return { up: dark(row - 1, col), down: dark(row + 1, col), left: dark(row, col - 1), right: dark(row, col + 1) };
}

/** [top-left, top-right, bottom-right, bottom-left]. A corner is only rounded when both orthogonal
 *  modules next to it are light — a corner touching a neighbouring dark module stays square, so
 *  adjacent dark modules join into a smooth block instead of separate cells. */
function connectedCorners(n: ReturnType<typeof neighborsOf>, radius: number): Corners {
  return [
    n.up || n.left ? 0 : radius,
    n.up || n.right ? 0 : radius,
    n.down || n.right ? 0 : radius,
    n.down || n.left ? 0 : radius,
  ];
}

/** Like `connectedCorners`, but only rounds the two diagonal corners (top-left, bottom-right) — the
 *  other two always stay square, giving a slanted one-way "flowing" feel instead of symmetric
 *  rounding on all 4 corners. */
function classyCorners(n: ReturnType<typeof neighborsOf>, radius: number): Corners {
  return [n.up || n.left ? 0 : radius, 0, n.down || n.right ? 0 : radius, 0];
}

/** Draws each module onto the canvas by hand instead of using the library's built-in
 *  `createDataURL` — so the colours and module shapes can be chosen. "Square"/"Rounded"/"Dots"
 *  draw each cell fully independently; "Connected" and "Classy" know about neighbouring modules —
 *  rounding each module's 4 corners individually (`roundRect` takes an array of per-corner radii)
 *  so adjacent dark modules look like one seamless block. */
function draw(
  canvas: HTMLCanvasElement,
  grid: Grid,
  moduleStyle: ModuleStyle,
  fgColor: string,
  bgColor: string,
  cellSize: number,
  margin: number,
) {
  const total = (grid.size + margin * 2) * cellSize;
  canvas.width = total;
  canvas.height = total;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  ctx.fillStyle = bgColor;
  ctx.fillRect(0, 0, total, total);
  ctx.fillStyle = fgColor;

  for (let row = 0; row < grid.size; row++) {
    for (let col = 0; col < grid.size; col++) {
      if (!grid.isDark(row, col)) continue;
      const x = (col + margin) * cellSize;
      const y = (row + margin) * cellSize;
      switch (moduleStyle) {
        case "square":
          ctx.fillRect(x, y, cellSize, cellSize);
          break;
        case "rounded":
          ctx.beginPath();
          ctx.roundRect(x, y, cellSize, cellSize, cellSize * 0.3);
          ctx.fill();
          break;
        case "dots":
          ctx.beginPath();
          ctx.arc(x + cellSize / 2, y + cellSize / 2, cellSize * 0.42, 0, Math.PI * 2);
          ctx.fill();
          break;
        case "connected":
          ctx.beginPath();
          ctx.roundRect(x, y, cellSize, cellSize, connectedCorners(neighborsOf(grid, row, col), cellSize * 0.5));
          ctx.fill();
          break;
        case "classy":
          ctx.beginPath();
          ctx.roundRect(x, y, cellSize, cellSize, classyCorners(neighborsOf(grid, row, col), cellSize * 0.5));
          ctx.fill();
          break;
      }
    }
  }
}

function QrcodePanel() {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [level, setLevel] = useState<ErrorCorrectionLevel>("M");
  const [moduleStyle, setModuleStyle] = useState<ModuleStyle>("square");
  const [fgColor, setFgColor] = useState("#000000");
  const [bgColor, setBgColor] = useState("#ffffff");
  const [cellSize, setCellSize] = useState(8);
  const [margin, setMargin] = useState(2);
  const [copied, setCopied] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const trimmed = text.trim();
  const grid = trimmed === "" ? null : encodeQr(trimmed, level);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !grid) return;
    draw(canvas, grid, moduleStyle, fgColor, bgColor, cellSize, margin);
  }, [grid, moduleStyle, fgColor, bgColor, cellSize, margin]);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), COPIED_MS);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    void canvasToBlob(canvas)
      .then((blob) => (blob ? copyImage(blob) : Promise.reject()))
      .then(() => setCopied(true))
      .catch(() => {});
  };

  const levelOptions: SelectOption<ErrorCorrectionLevel>[] = [
    { value: "L", label: t("toolbox.qrcode.levelL") },
    { value: "M", label: t("toolbox.qrcode.levelM") },
    { value: "Q", label: t("toolbox.qrcode.levelQ") },
    { value: "H", label: t("toolbox.qrcode.levelH") },
  ];

  const styleOptions: SelectOption<ModuleStyle>[] = [
    { value: "square", label: t("toolbox.qrcode.styleSquare") },
    { value: "rounded", label: t("toolbox.qrcode.styleRounded") },
    { value: "dots", label: t("toolbox.qrcode.styleDots") },
    { value: "connected", label: t("toolbox.qrcode.styleConnected") },
    { value: "classy", label: t("toolbox.qrcode.styleClassy") },
  ];

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder={t("toolbox.qrcode.placeholder")}
          maxRows={6}
        />
      </label>

      <div className={styles.controls}>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.qrcode.style")}</span>
          <Select
            value={moduleStyle}
            options={styleOptions}
            onChange={setModuleStyle}
            ariaLabel={t("toolbox.qrcode.style")}
            className={styles.style}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.qrcode.level")}</span>
          <Select
            value={level}
            options={levelOptions}
            onChange={setLevel}
            ariaLabel={t("toolbox.qrcode.level")}
            className={styles.level}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.qrcode.fgColor")}</span>
          <input
            type="color"
            value={fgColor}
            onChange={(event) => setFgColor(event.target.value)}
            aria-label={t("toolbox.qrcode.fgColor")}
            className={styles.color}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.qrcode.bgColor")}</span>
          <input
            type="color"
            value={bgColor}
            onChange={(event) => setBgColor(event.target.value)}
            aria-label={t("toolbox.qrcode.bgColor")}
            className={styles.color}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.qrcode.cellSize")}</span>
          <Input
            type="number"
            min={MIN_CELL}
            max={MAX_CELL}
            value={cellSize}
            onChange={(event) =>
              setCellSize(Math.min(MAX_CELL, Math.max(MIN_CELL, Math.floor(Number(event.target.value)) || MIN_CELL)))
            }
            aria-label={t("toolbox.qrcode.cellSize")}
            className={styles.num}
          />
        </label>
        <label className={styles.field}>
          <span className={styles.fieldLabel}>{t("toolbox.qrcode.margin")}</span>
          <Input
            type="number"
            min={MIN_MARGIN}
            max={MAX_MARGIN}
            value={margin}
            onChange={(event) =>
              setMargin(
                Math.min(MAX_MARGIN, Math.max(MIN_MARGIN, Math.floor(Number(event.target.value)) || MIN_MARGIN)),
              )
            }
            aria-label={t("toolbox.qrcode.margin")}
            className={styles.num}
          />
        </label>
      </div>

      {trimmed !== "" && !grid ? <p className={styles.tooLong}>{t("toolbox.qrcode.tooLong")}</p> : null}

      {trimmed !== "" && grid ? (
        <div className={styles.preview}>
          <canvas ref={canvasRef} className={styles.canvas} />
          <Button onClick={copy}>
            {copied ? <CheckIcon /> : <CopyIcon />}
            {t(copied ? "toolbox.copied" : "toolbox.qrcode.copyImage")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}

export default QrcodePanel;
