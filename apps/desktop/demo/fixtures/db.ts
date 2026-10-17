import constants from "../constants.json";
import type {
  SavedConnection,
  SqlColumnMeta,
  SqlCollation,
  SqlSchemaOutline,
  SqlTablePage,
  TableStats,
} from "../../src/modules/db/types";
import type { SqlServerInfo } from "../../src/modules/db/sql/api";
import { returns, type Handlers } from "../ipc/dispatch";
import { MINUTE, NOW } from "./time";

/** The acme-shop PostgreSQL database: four tables, forty orders on screen out of 1,284. */

const CONNECTION: SavedConnection = {
  id: constants.connectionId,
  name: "acme_shop",
  config: { kind: "postgres", host: "127.0.0.1", port: 5432, username: "acme", database: "acme_shop" },
};

export const dbFiles = {
  "connections.json": { saved: [CONNECTION] },
};

const CONNECTION_HANDLE = "demo-connection-1";

const TABLES: TableStats[] = [
  { name: "customers", rows: 412, dataSize: 96_000, indexSize: 40_000, avgRecordSize: 233 },
  { name: "order_items", rows: 3_902, dataSize: 512_000, indexSize: 180_000, avgRecordSize: 131 },
  { name: "orders", rows: 1_284, dataSize: 264_000, indexSize: 88_000, avgRecordSize: 205 },
  { name: "products", rows: 86, dataSize: 32_000, indexSize: 16_000, avgRecordSize: 372 },
];

type Column = [name: string, dataType: string, nullable: boolean];

const ORDER_COLUMNS: Column[] = [
  ["id", "bigint", false],
  ["customer", "varchar(120)", false],
  ["email", "varchar(160)", false],
  ["status", "varchar(16)", false],
  ["items", "integer", false],
  ["total", "numeric(10,2)", false],
  ["currency", "char(3)", false],
  ["created_at", "timestamptz", false],
];

const CUSTOMERS = [
  "Ada Nguyen",
  "Linus Berg",
  "Mai Tran",
  "Omar Haddad",
  "Sofia Rossi",
  "Kenji Sato",
  "Priya Patel",
  "Lucas Martin",
  "Hana Kim",
  "Diego Alvarez",
  "Emma Schulz",
  "Tuan Le",
];
const STATUSES = ["paid", "shipped", "paid", "pending", "delivered", "paid", "refunded", "shipped"];

function meta(dataType: string, nullable: boolean, extra = ""): SqlColumnMeta {
  return { dataType, nullable, defaultValue: null, extra, foreignKey: null };
}

function ordersPage(): SqlTablePage {
  const rows = Array.from({ length: 40 }, (_, i) => {
    const customer = CUSTOMERS[i % CUSTOMERS.length];
    const items = 1 + ((i * 7) % 5);
    return {
      id: 10_421 - i,
      customer,
      email: `${customer.toLowerCase().replace(" ", ".")}@example.com`,
      status: STATUSES[i % STATUSES.length],
      items,
      total: (items * 18.5 + ((i * 13) % 40) + 0.99).toFixed(2),
      currency: "USD",
      // As PostgreSQL prints a timestamptz, not as JavaScript does.
      created_at: new Date(NOW - i * 47 * MINUTE).toISOString().replace("T", " ").replace(".000Z", "+00"),
    };
  });
  return {
    columns: ORDER_COLUMNS.map(([name]) => name),
    columnMeta: Object.fromEntries(
      ORDER_COLUMNS.map(([name, dataType, nullable]) => [
        name,
        meta(dataType, nullable, name === "id" ? "auto_increment" : ""),
      ]),
    ),
    primaryKey: ["id"],
    autoIncrementColumn: "id",
    rows,
    total: 1_284,
  };
}

const OUTLINE: SqlSchemaOutline = {
  database: "acme_shop",
  tables: [
    {
      name: "orders",
      columns: ORDER_COLUMNS.map(([name, dataType, nullable]) => ({
        name,
        dataType,
        nullable,
        key: name === "id" ? "PRI" : "",
        references: null,
      })),
    },
    { name: "customers", columns: [{ name: "id", dataType: "bigint", nullable: false, key: "PRI", references: null }] },
    { name: "products", columns: [{ name: "id", dataType: "bigint", nullable: false, key: "PRI", references: null }] },
    {
      name: "order_items",
      columns: [
        { name: "id", dataType: "bigint", nullable: false, key: "PRI", references: null },
        { name: "order_id", dataType: "bigint", nullable: false, key: "MUL", references: "orders.id" },
      ],
    },
  ],
};

function emptyPage(): SqlTablePage {
  return {
    columns: ["id"],
    columnMeta: { id: meta("bigint", false) },
    primaryKey: ["id"],
    autoIncrementColumn: "id",
    rows: [],
    total: 0,
  };
}

export const dbHandlers: Handlers = {
  secrets_load: returns({ password: "demo" }),
  connect_db: returns(CONNECTION_HANDLE),
  disconnect_db: returns(null),
  sql_list_databases: returns<string[]>(["acme_analytics", "acme_shop", "postgres"]),
  sql_server_info: returns<SqlServerInfo>({ version: "17.2", os: "macOS" }),
  sql_collations: returns<SqlCollation[]>([]),
  sql_list_tables: returns<string[]>(TABLES.map((table) => table.name)),
  sql_table_stats: returns<TableStats[]>(TABLES),
  sql_schema_outline: returns<SqlSchemaOutline>(OUTLINE),
  sql_table_data: (args): SqlTablePage => (args.table === "orders" ? ordersPage() : emptyPage()),
};
