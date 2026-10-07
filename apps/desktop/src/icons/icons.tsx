import { Icon, type IconProps } from "./Icon";
import styles from "./Icon.module.css";

/* Every icon in the app, drawn on {@link Icon}'s 24×24 grid. Keeping them in one file keeps the
 * set visible as a set: adding one means matching the weight and shape of the others rather
 * than pasting in whatever a search turned up. */

/** The Backspace key itself, for saying which key does something rather than what it does. */
export function BackspaceIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M9.5 5h9.5a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9.5L3 12z" />
      <path d="M12.5 9.5l5 5" />
      <path d="M17.5 9.5l-5 5" />
    </Icon>
  );
}

/** Delete: a document, a property, an item. */
export function TrashIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3 6h18" />
      <path d="M8 6V4.5A1.5 1.5 0 0 1 9.5 3h5A1.5 1.5 0 0 1 16 4.5V6" />
      <path d="M18.5 6l-.8 13.1A2 2 0 0 1 15.7 21H8.3a2 2 0 0 1-2-1.9L5.5 6" />
      <path d="M10 10.5v6" />
      <path d="M14 10.5v6" />
    </Icon>
  );
}

/** Write something out of the app and onto disk — dumping a database to a file. */
export function DownloadIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 3v11" />
      <path d="M7.5 9.5L12 14l4.5-4.5" />
      <path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />
    </Icon>
  );
}

/** Read something off disk and into the app — restoring a database from a file. */
export function UploadIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 14V3" />
      <path d="M7.5 7.5L12 3l4.5 4.5" />
      <path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />
    </Icon>
  );
}

/** A field that cannot be edited — the primary key of a document. */
export function LockIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4" y="10.5" width="16" height="10.5" rx="2" />
      <path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" />
    </Icon>
  );
}

/** Confirmation that something landed — the flash after a save. */
export function CheckIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M20 6.5L9.5 17 4 11.5" />
    </Icon>
  );
}

/** Dismiss: a tab, a banner, a dialog. */
export function CloseIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M18 6L6 18" />
      <path d="M6 6l12 12" />
    </Icon>
  );
}

/** Add: a new tab. */
export function PlusIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </Icon>
  );
}

/** Remove one of a repeating set — the filter bar's "drop this row" button. {@link PlusIcon}'s
 * counterpart, and drawn as its horizontal stroke alone so the pair reads together. */
export function MinusIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 12h14" />
    </Icon>
  );
}

/** Duplicate: the table's "clone the selected rows" action. Two sheets, the front one offset
 * over the back one — the same shape the OS file managers use for a copy. */
export function CopyIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="9" y="9" width="12" height="12" rx="2" />
      <path d="M5 15H4.5A1.5 1.5 0 0 1 3 13.5v-9A1.5 1.5 0 0 1 4.5 3h9A1.5 1.5 0 0 1 15 4.5V5" />
    </Icon>
  );
}

/** A database with no engine's brand on it: the database *module* rather than any one connection,
 * which is what the `[+]` menu names. Three platters — the stack a database has been drawn as since
 * disk packs, and the one shape that reads as "database" without borrowing a logo. */
export function DatabaseGenericIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 3c3.9 0 7 1.1 7 2.5S15.9 8 12 8 5 6.9 5 5.5 8.1 3 12 3z" />
      <path d="M19 5.5v13c0 1.4-3.1 2.5-7 2.5s-7-1.1-7-2.5v-13" />
      <path d="M19 12c0 1.4-3.1 2.5-7 2.5S5 13.4 5 12" />
    </Icon>
  );
}

/** Open something for editing — a column, an index. Drawn as a pencil laid across the grid, its
 * tip at the bottom left, with the ferrule as the short stroke that tells the two ends apart. */
export function PencilIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4.5 19.5l.9-3.9L16 5a2 2 0 0 1 2.9 2.9L8.4 18.6l-3.9.9Z" />
      <path d="M14.6 6.4l3 3" />
    </Icon>
  );
}

/** Collapsed disclosure, and "next" in a pager. */
export function ChevronRightIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M9 5l7 7-7 7" />
    </Icon>
  );
}

/** "Previous" in a pager. */
export function ChevronLeftIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M15 5l-7 7 7 7" />
    </Icon>
  );
}

/** Expanded disclosure, and "sorted descending" in a grid header. */
export function ChevronDownIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 9l7 7 7-7" />
    </Icon>
  );
}

/** "Sorted ascending" in a grid header — {@link ChevronDownIcon} the other way up. */
export function ChevronUpIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M5 15l7-7 7 7" />
    </Icon>
  );
}

/** Refetch a list from the server — the sidebar's reload action. Drawn as an almost-closed
 * circle so that spinning it (while the reload is in flight) reads as motion. */
export function ReloadIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M20.25 12a8.25 8.25 0 1 1-2.4-5.835" />
      <path d="M20.25 3v4.8h-4.8" />
    </Icon>
  );
}

/** Two arrows chasing each other round: sync, as a Settings pane. */
export function SyncIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M20.25 12a8.25 8.25 0 0 1-14.3 5.6" />
      <path d="M3.75 12a8.25 8.25 0 0 1 14.3-5.6" />
      <path d="M18.05 3v3.4h-3.4" />
      <path d="M5.95 21v-3.4h3.4" />
    </Icon>
  );
}

interface CloudArrowIconProps extends IconProps {
  /** The arrow travels, for a transfer under way. Still for anyone who asked for less motion. */
  moving?: boolean;
}

/* The cloud both transfer icons share, open at the bottom so the arrow passes through it. */
const CLOUD = "M7 18a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 18 8.5a4 4 0 0 1 0 8";

/** Sync bringing the server's changes down to this machine. */
export function CloudDownloadIcon({ moving = false, ...props }: CloudArrowIconProps) {
  return (
    <Icon {...props}>
      <path d={CLOUD} />
      <g className={moving ? styles.fall : undefined}>
        <path d="M12 11v8" />
        <path d="M9 16.5l3 3 3-3" />
      </g>
    </Icon>
  );
}

/** Sync sending this machine's changes up to the server — only ever while it really is. */
export function CloudUploadIcon({ moving = false, ...props }: CloudArrowIconProps) {
  return (
    <Icon {...props}>
      <path d={CLOUD} />
      <g className={moving ? styles.rise : undefined}>
        <path d="M12 20v-8" />
        <path d="M9 14.5l3-3 3 3" />
      </g>
    </Icon>
  );
}

/** Reveal something deliberately hidden — the connection string with its credentials in it. */
export function EyeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M1.5 12s3.9-6.75 10.5-6.75S22.5 12 22.5 12s-3.9 6.75-10.5 6.75S1.5 12 1.5 12Z" />
      <circle cx="12" cy="12" r="3" />
    </Icon>
  );
}

/** {@link EyeIcon} struck through: the same thing, currently hidden. */
export function EyeOffIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M1.5 12s3.9-6.75 10.5-6.75S22.5 12 22.5 12s-3.9 6.75-10.5 6.75S1.5 12 1.5 12Z" />
      <circle cx="12" cy="12" r="3" />
      <path d="M3.75 20.25 20.25 3.75" />
    </Icon>
  );
}

/** A grouping rather than a thing — a row in the Redis key tree that stands for a shared prefix
 * and not for a key. Drawn as the tabbed folder the file managers use, so it reads at a glance
 * as "there is more inside" next to the type badges the real keys carry. */
export function FolderIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3 6.5a2 2 0 0 1 2-2h4a2 2 0 0 1 1.5.7l1.3 1.5H19a2 2 0 0 1 2 2v9.3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
    </Icon>
  );
}

/** Held at the top of a list. A pushpin seen head-on rather than at the usual angle: at 14px the
 * tilted one is a smudge, while the symmetrical shape keeps its cap, shoulders and needle. */
export function PinIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M8.5 3h7" />
      <path d="M10 3v6.5L7 13h10l-3-3.5V3" />
      <path d="M12 13v8" />
    </Icon>
  );
}

/** Run or start something — the SQL editor's Run button, a service's Start. Filled, so it holds
 * its shape at the button's font size the way an outlined triangle would not; its corners are
 * rounded to match the round joins of the rest of the set, and its centroid sits on the grid's
 * centre so it does not lean left inside a square button. Paired with {@link StopIcon}, and as
 * tall as it — 14 units — so a Start and a Stop button drawn at the same `size` carry the same
 * weight: at 11.6 the triangle sat visibly lower than the square beside it. */
export function PlayIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path
        d="M8 6.1v11.8a1.1 1.1 0 0 0 1.65.95l10.2-5.9a1.1 1.1 0 0 0 0-1.9L9.65 5.15A1.1 1.1 0 0 0 8 6.1z"
        fill="currentColor"
        stroke="none"
      />
    </Icon>
  );
}

/** Lay the script out: lines of text, indented as a formatter would indent them. */
export function FormatIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 6h16" />
      <path d="M9 11h11" />
      <path d="M9 16h7" />
      <path d="M4 21h16" />
    </Icon>
  );
}

/** What has been run before: a clock with its hand wound back. */
export function HistoryIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1" />
      <path d="M3.5 4.5v4h4" />
      <path d="M12 7.5V12l3 1.8" />
    </Icon>
  );
}

/** Keep this query under a name: a bookmark. */
export function BookmarkIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6.5 3.5h11a1 1 0 0 1 1 1v16l-6.5-4-6.5 4v-16a1 1 0 0 1 1-1z" />
    </Icon>
  );
}

/** A console waiting for something to be run — the SQL editor's empty results panel. */
export function TerminalIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M7.5 9.5L10.5 12l-3 2.5" />
      <path d="M13.5 15h3.5" />
    </Icon>
  );
}

/** Open this out over the whole window — the query results' zoom button. Four corners pushed
 * outwards, the shape a video player uses for full screen, so it reads as "give this the room"
 * rather than as a disclosure arrow. */
export function ExpandIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M9.5 4H4v5.5" />
      <path d="M14.5 4H20v5.5" />
      <path d="M20 14.5V20h-5.5" />
      <path d="M4 14.5V20h5.5" />
    </Icon>
  );
}

/** Settings. A gear rather than sliders: it is the shape every desktop app puts on this door, and
 * the tab bar has room for one glyph only, so it has to be the one nobody has to learn. */
export function SettingsIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="12" cy="12" r="3.2" />
      <path d="M19.5 13.9a7.9 7.9 0 0 0 0-3.8l2-1.5-2-3.4-2.4 1a7.8 7.8 0 0 0-3.3-1.9L13.3 2h-2.6l-.5 2.3a7.8 7.8 0 0 0-3.3 1.9l-2.4-1-2 3.4 2 1.5a7.9 7.9 0 0 0 0 3.8l-2 1.5 2 3.4 2.4-1a7.8 7.8 0 0 0 3.3 1.9l.5 2.3h2.6l.5-2.3a7.8 7.8 0 0 0 3.3-1.9l2.4 1 2-3.4z" />
    </Icon>
  );
}

/** Colours to choose from — the look of the app, in settings. */
export function PaletteIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 3a9 9 0 0 0 0 18 1.9 1.9 0 0 0 1.5-3.1 1.9 1.9 0 0 1 1.5-3.1h2A3.9 3.9 0 0 0 21 10.6C20.4 6.3 16.6 3 12 3z" />
      <circle cx="7.6" cy="12.4" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="9.8" cy="8.2" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="14.4" cy="7.8" r="1.1" fill="currentColor" stroke="none" />
    </Icon>
  );
}

/** The Modules pane in Settings — which of the app's parts this window draws. */
export function ModulesIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
    </Icon>
  );
}

/** The shortcut table in Settings. */
export function KeyboardIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="2" y="6" width="20" height="12" rx="2" />
      <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01" />
      <path d="M6 14h.01M18 14h.01" />
      <path d="M9.5 14h5" />
    </Icon>
  );
}

/** A status marker rather than an action — currently the "unsaved changes" bullet. Filled, so
 * it reads as a dot at the small sizes it is used at instead of as a thin ring. */
/** The MixEngine module in the `[+]` menu: a chip with its pins — the processor the whole
 * environment runs on. Two pins a side rather than three, so the gaps between them survive the
 * 14px the tab strip draws it at. Laid out `2..22` on the grid, the extent [`SettingsIcon`] and
 * [`KeyboardIcon`] use, because a chip's pins carry no visual weight: drawn any smaller, its body
 * sat as a dark knot beside the cog and the globe in the Settings sidebar. The earlier drawing, a
 * block seen from the side with a ring at its centre, read as an eye at that size. */
export function EngineIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="5" y="5" width="14" height="14" rx="2" />
      <rect x="9.5" y="9.5" width="5" height="5" />
      <path d="M9 5V2M15 5V2M9 22v-3M15 22v-3M5 9H2M5 15H2M22 9h-3M22 15h-3" />
    </Icon>
  );
}

/** The overflow menu: the actions a row has that do not fit beside it.
 *
 * Filled circles rather than a stroked glyph, on [`DotIcon`]'s precedent — and vertical, which is
 * the arrangement that reads as "more of this row" rather than "more of this toolbar". Laid out
 * `3..21` on the grid, the same extent [`CopyIcon`] uses, so it sits at the weight of every other
 * icon beside it instead of shrinking into the column.
 */
export function MoreIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="12" cy="5" r="2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="19" r="2" fill="currentColor" stroke="none" />
    </Icon>
  );
}

export function DotIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="12" cy="12" r="5" fill="currentColor" stroke="none" />
    </Icon>
  );
}

/** The REST module in the `[+]` menu: something reached over the network. */
export function GlobeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9s1.3-6.3 3.8-9z" />
    </Icon>
  );
}

/** Send: the one button a request pane is asking for. */
export function SendIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M20 12l-16-8 6 8-6 8 16-8z" />
    </Icon>
  );
}

/** Turn this off: the power symbol, on the button that closes a database connection. Read the
 *  same way on every appliance anyone owns, which no drawing of a plug manages at 14px. */
export function PowerIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M12 3.5v8" />
      <path d="M6.8 8.3A7 7 0 1 0 17.2 8.3" />
    </Icon>
  );
}

/** Stop something running — a service, or a request in flight on {@link SendIcon}'s button.
 * Filled like {@link PlayIcon}, its pair. Big enough to read as a square at a row button's size:
 * at 11px a smaller one shrank to a red speck nobody recognised as Stop. */
export function StopIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="5" y="5" width="14" height="14" rx="3" fill="currentColor" stroke="none" />
    </Icon>
  );
}

/** A wrench — the Tools module's toolbox. */
export function ToolsIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M14.7 6.3a4 4 0 0 0 5 5l-8.4 8.4a2.1 2.1 0 0 1-3-3z" />
      <path d="M14.7 6.3 17.5 3.5" />
    </Icon>
  );
}

/** The Dashboard screen: a page of panels at a glance. */
export function DashboardIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3.5" y="3.5" width="7" height="9" rx="1.5" />
      <rect x="13.5" y="3.5" width="7" height="5" rx="1.5" />
      <rect x="13.5" y="11.5" width="7" height="9" rx="1.5" />
      <rect x="3.5" y="15.5" width="7" height="5" rx="1.5" />
    </Icon>
  );
}

/** Metrics over time: a pulse line. */
export function PulseIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3 12h4l3-8 4 16 3-8h4" />
    </Icon>
  );
}

/** A log: a page of lines. */
export function LogIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M14 3H6.5A2.5 2.5 0 0 0 4 5.5v13A2.5 2.5 0 0 0 6.5 21h11a2.5 2.5 0 0 0 2.5-2.5V9z" />
      <path d="M14 3v6h6" />
      <path d="M8 13h8M8 17h5" />
    </Icon>
  );
}

/** Something installed: a package box. */
export function PackageIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M21 16V8l-9-5-9 5v8l9 5z" />
      <path d="M3.3 7 12 12l8.7-5M12 22V12" />
    </Icon>
  );
}

/** An extension to something: a puzzle piece. */
export function PuzzleIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M10 3h4v3a2 2 0 1 0 4 0h3v5h-3a2 2 0 1 0 0 4h3v6h-6v-3a2 2 0 1 0-4 0v3H4v-6h3a2 2 0 1 0 0-4H4V6h6z" />
    </Icon>
  );
}

/** This machine: a screen on a stand. */
export function MonitorIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M8 20h8M12 16v4" />
    </Icon>
  );
}

/** A running server process: stacked units with their lights. */
export function ServerIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4" y="4" width="16" height="6.5" rx="1.5" />
      <rect x="4" y="13.5" width="16" height="6.5" rx="1.5" />
      <path d="M8 7.25h.01M8 16.75h.01" />
    </Icon>
  );
}

/** A ready-made layout to start from. */
export function BlueprintIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <path d="M4 10h16M10 10v10" />
    </Icon>
  );
}

/** Something added to the app: a square with a plus. */
export function AddOnIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <rect x="4" y="4" width="16" height="16" rx="2.5" />
      <path d="M12 8.5v7M8.5 12h7" />
    </Icon>
  );
}

/** A setting with sliders. */
export function SlidersIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12" />
      <circle cx="16" cy="6" r="2" />
      <circle cx="10" cy="12" r="2" />
      <circle cx="18" cy="18" r="2" />
    </Icon>
  );
}