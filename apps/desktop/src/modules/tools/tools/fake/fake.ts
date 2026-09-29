/**
 * Generates fake data to seed/test tables, and guesses the field list up front from a sample JSON.
 *
 * The data pools (names, addresses, companies...) are written right in this file — no external
 * faker library is installed, in the same spirit as `ids.ts` writing its own UUID/ULID.
 * `inferFields` is written separately too, not reusing the Schema generator tool's recursive schema
 * inference (`schema/infer.ts`): here only the column name + the raw JS type of the first value are
 * needed, with no merging of several samples or descending into nested objects/arrays — pulling
 * that whole machinery in just to use a small part of it would force `fake` to understand another
 * tool's internal structure, which `tool.ts` deliberately does not allow.
 */

export type Locale = "vi" | "en";

export const LOCALES: Locale[] = ["vi", "en"];

export type FieldKind =
  | "fullName"
  | "firstName"
  | "middleName"
  | "lastName"
  | "email"
  | "phone"
  | "address"
  | "city"
  | "company"
  | "uuid"
  | "integer"
  | "float"
  | "boolean"
  | "date"
  | "word"
  | "sentence"
  | "constant";

export const FIELD_KINDS: FieldKind[] = [
  "fullName",
  "firstName",
  "middleName",
  "lastName",
  "email",
  "phone",
  "address",
  "city",
  "company",
  "uuid",
  "integer",
  "float",
  "boolean",
  "date",
  "word",
  "sentence",
  "constant",
];

/** Which kinds have locale-specific data pools — the Panel only shows the Locale picker for these
 *  kinds. */
export const LOCALE_KINDS: Set<FieldKind> = new Set([
  "fullName",
  "firstName",
  "middleName",
  "lastName",
  "phone",
  "address",
  "city",
  "company",
]);

export interface FieldSpec {
  name: string;
  kind: FieldKind;
  /** name/address/city/company/phone — defaults to "vi" when absent. */
  locale?: Locale;
  /** fullName — whether to insert a middle name between the family and given names, defaults to
   *  false. */
  includeMiddle?: boolean;
  /** integer/float */
  min?: number;
  max?: number;
  /** float — the number of decimal places, defaults to 2 */
  decimals?: number;
  /** date — ISO, defaults to 5 years ago up to the time of generation */
  from?: string;
  to?: string;
  /** constant */
  value?: string;
}

const FIRST_NAMES: Record<Locale, string[]> = {
  vi: [
    "An", "Bình", "Chi", "Dũng", "Giang", "Hà", "Hải", "Hoa", "Hùng", "Khánh",
    "Lan", "Linh", "Long", "Mai", "Minh", "Nam", "Ngọc", "Nhung", "Phong", "Phương",
    "Quân", "Quang", "Thảo", "Thắng", "Thủy", "Trang", "Trung", "Tuấn", "Vân", "Việt",
  ],
  en: [
    "James", "Mary", "John", "Patricia", "Robert", "Jennifer", "Michael", "Linda",
    "William", "Elizabeth", "David", "Barbara", "Richard", "Susan", "Joseph", "Jessica",
    "Thomas", "Sarah", "Charles", "Karen",
  ],
};

const LAST_NAMES: Record<Locale, string[]> = {
  vi: [
    "Nguyễn", "Trần", "Lê", "Phạm", "Hoàng", "Huỳnh", "Phan", "Vũ", "Võ", "Đặng",
    "Bùi", "Đỗ", "Hồ", "Ngô", "Dương", "Lý",
  ],
  en: [
    "Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller", "Davis",
    "Rodriguez", "Martinez", "Hernandez", "Lopez", "Wilson", "Anderson", "Taylor",
  ],
};

/** Vietnamese middle names have a pool of their own (not given names); English reuses the
 *  first-name pool — in real life a middle name is usually drawn from the same set of given names
 *  anyway. */
const MIDDLE_NAMES: Record<Locale, string[]> = {
  vi: [
    "Văn", "Thị", "Hữu", "Đức", "Minh", "Ngọc", "Thành", "Xuân",
    "Quang", "Anh", "Bảo", "Gia", "Khánh", "Thanh", "Hồng",
  ],
  en: FIRST_NAMES.en,
};

const CITIES: Record<Locale, string[]> = {
  vi: [
    "Hà Nội", "Hồ Chí Minh", "Đà Nẵng", "Hải Phòng", "Cần Thơ", "Huế",
    "Nha Trang", "Vũng Tàu", "Biên Hòa", "Quy Nhơn",
  ],
  en: [
    "New York", "Los Angeles", "Chicago", "Houston", "Phoenix", "Philadelphia",
    "San Antonio", "San Diego", "Dallas", "Austin",
  ],
};

const STREETS: Record<Locale, string[]> = {
  vi: [
    "Nguyễn Trãi", "Lê Lợi", "Trần Hưng Đạo", "Hai Bà Trưng", "Điện Biên Phủ",
    "Cách Mạng Tháng Tám", "Phan Đình Phùng", "Nguyễn Huệ", "Lý Thường Kiệt", "Tô Hiến Thành",
  ],
  en: [
    "Main St", "Oak Ave", "Maple St", "Cedar Ave", "Elm St",
    "Washington Ave", "Park Rd", "Lake St", "Hill Ave", "Church St",
  ],
};

const COMPANIES: Record<Locale, string[]> = {
  vi: [
    "Công ty TNHH An Phát", "Tập đoàn Sao Việt", "Công ty CP Minh Long", "Công ty TNHH Đại Dương",
    "Tập đoàn Hưng Thịnh", "Công ty CP Phú Quý", "Công ty TNHH Thành Công", "Tập đoàn Việt Tiến",
  ],
  en: [
    "Acme Corp", "Globex LLC", "Initech", "Umbrella Inc",
    "Stark Industries", "Wayne Enterprises", "Hooli", "Soylent Corp",
  ],
};

const WORDS = [
  "alpha", "beta", "gamma", "delta", "omega", "nova", "pixel", "vertex", "quantum", "cipher",
  "matrix", "zenith", "orbit", "spark", "echo", "fusion", "nimbus", "cobalt", "raven", "atlas",
];

const DOMAINS = ["example.com", "mail.test", "sample.dev", "demo.io"];

/** Common VN mobile prefixes — they need not be complete, only realistic enough for
 *  testing/demos. */
const VI_PHONE_PREFIXES = ["032", "070", "076", "081", "086", "088", "090", "091", "094", "096", "097", "098", "099"];

const DIACRITICS: [RegExp, string][] = [
  [/[àáạảãâầấậẩẫăằắặẳẵ]/g, "a"],
  [/[èéẹẻẽêềếệểễ]/g, "e"],
  [/[ìíịỉĩ]/g, "i"],
  [/[òóọỏõôồốộổỗơờớợởỡ]/g, "o"],
  [/[ùúụủũưừứựửữ]/g, "u"],
  [/[ỳýỵỷỹ]/g, "y"],
  [/đ/g, "d"],
];

/** Turns an accented name into the part of an email before the @ — good enough for unaccented
 *  locales (en) as well. */
export function slugify(text: string): string {
  let out = text.toLowerCase();
  for (const [pattern, replacement] of DIACRITICS) out = out.replace(pattern, replacement);
  return out.replace(/[^a-z0-9]+/g, "");
}

function pick<T>(items: T[], rnd: () => number): T {
  return items[Math.floor(rnd() * items.length)]!;
}

function randInt(min: number, max: number, rnd: () => number): number {
  return Math.floor(rnd() * (max - min + 1)) + min;
}

const HEX_DIGITS = "0123456789abcdef";

/** Does not reuse the ids tool's `uuidv4` — a different signature (`rnd: () => number` instead of
 *  real bytes from `crypto`), and here it only needs to look like a UUID v4 for test data, not have
 *  the right entropy. */
function fakeUuid(rnd: () => number): string {
  const hex = Array.from({ length: 32 }, () => HEX_DIGITS[Math.floor(rnd() * 16)]);
  hex[12] = "4";
  hex[16] = HEX_DIGITS[(HEX_DIGITS.indexOf(hex[16]!) & 0x3) | 0x8]!;
  const s = hex.join("");
  return `${s.slice(0, 8)}-${s.slice(8, 12)}-${s.slice(12, 16)}-${s.slice(16, 20)}-${s.slice(20)}`;
}

const FIVE_YEARS_MS = 5 * 365 * 24 * 60 * 60 * 1000;

function dateRange(field: FieldSpec, now: number): [number, number] {
  const from = field.from ? Date.parse(field.from) : NaN;
  const to = field.to ? Date.parse(field.to) : NaN;
  const start = Number.isNaN(from) ? now - FIVE_YEARS_MS : from;
  const end = Number.isNaN(to) ? now : to;
  return start <= end ? [start, end] : [end, start];
}

interface Person {
  first: string;
  middle: string;
  last: string;
}

function makePerson(locale: Locale, rnd: () => number): Person {
  return {
    first: pick(FIRST_NAMES[locale], rnd),
    middle: pick(MIDDLE_NAMES[locale], rnd),
    last: pick(LAST_NAMES[locale], rnd),
  };
}

/**
 * One "person" shared by every name/email field of the same locale in **the same row** — generated
 * once and then cached, so that `fullName`, `firstName`/`middleName`/`lastName` and `email` do not
 * tell three different stories about the same data row. Whichever field touches it first, the
 * remaining fields read back that same person — the result does not depend on the field order in
 * the list.
 */
function personFor(locale: Locale, rnd: () => number, people: Map<Locale, Person>): Person {
  let person = people.get(locale);
  if (!person) {
    person = makePerson(locale, rnd);
    people.set(locale, person);
  }
  return person;
}

const NAME_KINDS = new Set<FieldKind>(["fullName", "firstName", "middleName", "lastName"]);

/**
 * `email` has no Locale picker of its own on the Panel — it has to borrow the locale of the name
 * field in the same field list, rather than defaulting to "vi". Computed once for the whole
 * `generate()`, not per row: it depends only on the field list, not on the data generated.
 */
function emailLocaleHint(fields: FieldSpec[]): Locale {
  return fields.find((field) => NAME_KINDS.has(field.kind))?.locale ?? "vi";
}

function genValue(
  field: FieldSpec,
  rnd: () => number,
  now: number,
  people: Map<Locale, Person>,
  emailLocale: Locale,
): unknown {
  const locale: Locale = field.locale ?? (field.kind === "email" ? emailLocale : "vi");
  switch (field.kind) {
    case "firstName":
      return personFor(locale, rnd, people).first;
    case "middleName":
      return personFor(locale, rnd, people).middle;
    case "lastName":
      return personFor(locale, rnd, people).last;
    case "fullName": {
      const person = personFor(locale, rnd, people);
      if (locale === "vi") {
        return field.includeMiddle
          ? `${person.last} ${person.middle} ${person.first}`
          : `${person.last} ${person.first}`;
      }
      return field.includeMiddle
        ? `${person.first} ${person.middle} ${person.last}`
        : `${person.first} ${person.last}`;
    }
    case "email": {
      const person = personFor(locale, rnd, people);
      const suffix = randInt(1, 99, rnd);
      return `${slugify(person.first)}.${slugify(person.last)}${suffix}@${pick(DOMAINS, rnd)}`;
    }
    case "phone":
      return locale === "vi"
        ? `${pick(VI_PHONE_PREFIXES, rnd)}${String(randInt(0, 9999999, rnd)).padStart(7, "0")}`
        : `(${randInt(200, 999, rnd)}) ${randInt(200, 999, rnd)}-${String(randInt(0, 9999, rnd)).padStart(4, "0")}`;
    case "address":
      return `${randInt(1, 999, rnd)} ${pick(STREETS[locale], rnd)}`;
    case "city":
      return pick(CITIES[locale], rnd);
    case "company":
      return pick(COMPANIES[locale], rnd);
    case "uuid":
      return fakeUuid(rnd);
    case "integer":
      return randInt(field.min ?? 0, field.max ?? 1000, rnd);
    case "float": {
      const decimals = field.decimals ?? 2;
      const min = field.min ?? 0;
      const max = field.max ?? 1000;
      return Number((rnd() * (max - min) + min).toFixed(decimals));
    }
    case "boolean":
      return rnd() < 0.5;
    case "date": {
      const [start, end] = dateRange(field, now);
      return new Date(start + Math.floor(rnd() * (end - start + 1))).toISOString();
    }
    case "word":
      return pick(WORDS, rnd);
    case "sentence": {
      const words = Array.from({ length: randInt(4, 9, rnd) }, () => pick(WORDS, rnd));
      const text = words.join(" ");
      return text.charAt(0).toUpperCase() + text.slice(1) + ".";
    }
    case "constant":
      return field.value ?? "";
  }
}

/**
 * `rnd` returns a number in [0, 1) — the Panel passes a generator based on
 * `crypto.getRandomValues`, a test passes a deterministic LCG. `now` is kept separate for the same
 * reason `ids.ts` keeps it apart from `Date.now()`.
 */
export function generate(
  fields: FieldSpec[],
  count: number,
  rnd: () => number,
  now: number = Date.now(),
): Record<string, unknown>[] {
  const emailLocale = emailLocaleHint(fields);
  const rows: Record<string, unknown>[] = [];
  for (let i = 0; i < count; i++) {
    // A new map per row: a "person" is only shared within one row and does not leak into the next.
    const people = new Map<Locale, Person>();
    const row: Record<string, unknown> = {};
    for (const field of fields) row[field.name] = genValue(field, rnd, now, people, emailLocale);
    rows.push(row);
  }
  return rows;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const EMAIL_RE = /^\S+@\S+\.\S+$/;
const ISO_8601 = /^\d{4}-\d{2}-\d{2}([T ]\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:?\d{2})?)?$/;

function guessKind(name: string, value: unknown): FieldKind {
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "number") return Number.isInteger(value) ? "integer" : "float";
  if (typeof value === "string") {
    if (UUID_RE.test(value)) return "uuid";
    if (EMAIL_RE.test(value)) return "email";
    if (ISO_8601.test(value)) return "date";
  }
  const key = name.toLowerCase();
  if (/email/.test(key)) return "email";
  if (/phone|sdt|dienthoai|mobile/.test(key)) return "phone";
  if (/city|thanhpho/.test(key)) return "city";
  if (/address|diachi/.test(key)) return "address";
  if (/company|congty/.test(key)) return "company";
  if (/first[ _]?name|^ten$/.test(key)) return "firstName";
  if (/middle[ _]?name|tendem|ten_dem/.test(key)) return "middleName";
  if (/last[ _]?name|^ho$/.test(key)) return "lastName";
  if (/name|hoten/.test(key)) return "fullName";
  return "word";
}

/**
 * Guesses fields from the first row that can be read — an object, or the first object element of
 * an array. Only looks at one sample, without merging several rows like `schema/infer.ts`: this is
 * only an initial suggestion for the user to keep editing in the field table, not precise type
 * inference.
 */
export function inferFields(sample: unknown): FieldSpec[] | null {
  const row = isRecord(sample) ? sample : Array.isArray(sample) ? sample.find(isRecord) : undefined;
  if (!row) return null;
  return Object.entries(row)
    .filter(([, value]) => !isRecord(value) && !Array.isArray(value))
    .map(([name, value]) => ({ name, kind: guessKind(name, value) }));
}
