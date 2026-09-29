import type { ToolDefinition } from "./tool";
import CasePanel from "./tools/case/Panel";
import CheatsheetPanel from "./tools/cheatsheet/Panel";
import ConnectionPanel from "./tools/connection/Panel";
import ConvertPanel from "./tools/convert/Panel";
import DiffPanel from "./tools/diff/Panel";
import EncodePanel from "./tools/encode/Panel";
import EnvPanel from "./tools/env/Panel";
import FakePanel from "./tools/fake/Panel";
import FormatPanel from "./tools/format/Panel";
import IdsPanel from "./tools/ids/Panel";
import JwtPanel from "./tools/jwt/Panel";
import MaskPanel from "./tools/mask/Panel";
import PortsPanel from "./tools/ports/Panel";
import QrcodePanel from "./tools/qrcode/Panel";
import RadixPanel from "./tools/radix/Panel";
import RegexPanel from "./tools/regex/Panel";
import SchemaPanel from "./tools/schema/Panel";
import SqlToMongoPanel from "./tools/sql-to-mongo/Panel";
import TimestampPanel from "./tools/timestamp/Panel";

/** Every tool this module has. Adding a tool is one line here — and this file is the only place
 *  `ToolsTab` learns which tools exist. */
export const TOOLS: ToolDefinition[] = [
  {
    id: "sql-to-mongo",
    labelKey: "toolbox.sqlToMongo.label",
    group: "data",
    Panel: SqlToMongoPanel,
  },
  { id: "timestamp", labelKey: "toolbox.timestamp.label", group: "time", Panel: TimestampPanel },
  { id: "case", labelKey: "toolbox.case.label", group: "text", Panel: CasePanel },
  { id: "ids", labelKey: "toolbox.ids.label", group: "data", Panel: IdsPanel },
  { id: "fake", labelKey: "toolbox.fake.label", group: "data", Panel: FakePanel },
  { id: "encode", labelKey: "toolbox.encode.label", group: "encode", Panel: EncodePanel },
  { id: "jwt", labelKey: "toolbox.jwt.label", group: "encode", Panel: JwtPanel },
  { id: "format", labelKey: "toolbox.format.label", group: "text", Panel: FormatPanel },
  { id: "convert", labelKey: "toolbox.convert.label", group: "data", Panel: ConvertPanel },
  { id: "schema", labelKey: "toolbox.schema.label", group: "data", Panel: SchemaPanel },
  { id: "mask", labelKey: "toolbox.mask.label", group: "data", Panel: MaskPanel },
  { id: "env", labelKey: "toolbox.env.label", group: "infra", Panel: EnvPanel },
  { id: "diff", labelKey: "toolbox.diff.label", group: "text", Panel: DiffPanel },
  { id: "regex", labelKey: "toolbox.regex.label", group: "text", Panel: RegexPanel },
  { id: "ports", labelKey: "toolbox.ports.label", group: "infra", Panel: PortsPanel },
  { id: "radix", labelKey: "toolbox.radix.label", group: "encode", Panel: RadixPanel },
  { id: "qrcode", labelKey: "toolbox.qrcode.label", group: "encode", Panel: QrcodePanel },
  {
    id: "cheatsheet",
    labelKey: "toolbox.cheatsheet.label",
    group: "infra",
    Panel: CheatsheetPanel,
  },
  {
    id: "connection",
    labelKey: "toolbox.connection.label",
    group: "infra",
    Panel: ConnectionPanel,
  },
];
