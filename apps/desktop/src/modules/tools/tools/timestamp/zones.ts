/**
 * The list of time zones, under IANA's current names.
 *
 * `Intl.supportedValuesOf("timeZone")` returns **old** names for some zones — `Asia/Saigon` rather
 * than `Asia/Ho_Chi_Minh`, `Asia/Calcutta` rather than `Asia/Kolkata`, `Europe/Kiev` rather than
 * `Europe/Kyiv`. Those are entries in IANA's `backward` file, kept so old data does not break, and
 * ICU still emits them. Not every name is just a spelling change: `Saigon` and `Kiev` are names
 * from another era, and this is a list users read.
 *
 * The other direction is nothing to worry about: the runtime **accepts** the current names as
 * input even though it does not list them, so a canonical name passed straight into `Intl` works.
 * That means we display, store and look up by the same name.
 */
export const LEGACY_ZONES: Record<string, string> = {
  "Africa/Asmera": "Africa/Asmara",
  "America/Buenos_Aires": "America/Argentina/Buenos_Aires",
  "America/Catamarca": "America/Argentina/Catamarca",
  "America/Coral_Harbour": "America/Atikokan",
  "America/Cordoba": "America/Argentina/Cordoba",
  "America/Godthab": "America/Nuuk",
  "America/Indianapolis": "America/Indiana/Indianapolis",
  "America/Jujuy": "America/Argentina/Jujuy",
  "America/Louisville": "America/Kentucky/Louisville",
  "America/Mendoza": "America/Argentina/Mendoza",
  "Asia/Calcutta": "Asia/Kolkata",
  "Asia/Katmandu": "Asia/Kathmandu",
  "Asia/Rangoon": "Asia/Yangon",
  "Asia/Saigon": "Asia/Ho_Chi_Minh",
  "Atlantic/Faeroe": "Atlantic/Faroe",
  "Europe/Kiev": "Europe/Kyiv",
  "Pacific/Enderbury": "Pacific/Kanton",
  "Pacific/Ponape": "Pacific/Pohnpei",
  "Pacific/Truk": "Pacific/Chuuk",
};

export function canonicalZone(zone: string): string {
  return LEGACY_ZONES[zone] ?? zone;
}

/* `Intl.supportedValuesOf` is ES2022 while the project's `lib` is ES2020, so it is not in the
   types. Probed here rather than widening `lib` for the whole repo for exactly one call. */
const intlZones = (Intl as { supportedValuesOf?: (key: "timeZone") => string[] }).supportedValuesOf;

/** Every time zone the runtime knows, under current names, without duplicates, sorted. */
export function allZones(): string[] {
  const raw = intlZones ? intlZones("timeZone") : [Intl.DateTimeFormat().resolvedOptions().timeZone];
  return [...new Set(raw.map(canonicalZone))].sort();
}

/* `Intl.Locale.prototype.getTimeZones` is not in the ES2020 `lib` either, probed as above. It used
   to be a `timeZones` getter before becoming a function, and older webviews may still have the old
   name. */
type LocaleWithZones = Intl.Locale & {
  getTimeZones?: () => string[] | undefined;
  timeZones?: string[];
};

function zonesOfLocale(locale: Intl.Locale): string[] {
  const withZones = locale as LocaleWithZones;
  return withZones.getTimeZones?.() ?? withZones.timeZones ?? [];
}

/**
 * The zone to preselect for a machine, when the zone the operating system reports may be in the
 * wrong country.
 *
 * Windows only has very coarse IDs — `SE Asia Standard Time` covers Bangkok, Hanoi and Jakarta
 * alike — and ICU maps each ID to one representative zone. For that ID the representative is
 * `Asia/Bangkok`. A machine set to Vietnamese therefore opens with Bangkok already chosen, and
 * since both places share `+07:00`, nobody looking at the offset notices.
 *
 * So: if the locale's country has zones of its own, the machine's zone is not in that country, and
 * **the offsets match**, take the country's zone. The offset condition is what keeps this from
 * breaking the genuine case: a Vietnamese person sitting in London has a machine reporting
 * `Europe/London` and must be left alone.
 *
 * Only changes when that country has exactly one zone with a matching offset. With more, there is
 * no basis for choosing, and guessing at random is worse than the default already there.
 */
export function preferredZone(machineZone: string, locales: readonly string[], at: number): string {
  const zone = canonicalZone(machineZone);
  const offset = zoneOffset(zone, at);

  for (const tag of locales) {
    if (!tag) continue;
    try {
      const locale = new Intl.Locale(tag);
      if (!locale.region) continue;

      const inRegion = zonesOfLocale(locale).map(canonicalZone);
      if (inRegion.length === 0) continue;
      // The machine's zone already belongs to this country — nothing to fix, and no further source
      // is asked.
      if (inRegion.includes(zone)) return zone;

      const matching = inRegion.filter((name) => zoneOffset(name, at) === offset);
      if (matching.length === 1) return matching[0];
    } catch {
      // A broken language tag is skipped and the next source asked.
    }
  }

  return zone;
}

/**
 * The offset from UTC at a given moment, as `+07:00`.
 *
 * By moment rather than fixed: half the world changes its clocks by season, and a list saying
 * `America/New_York` is `-05:00` in the middle of July is wrong.
 *
 * Returns an empty string for a zone that does not exist, instead of throwing: this function runs
 * for every row of a four-hundred-entry list, and one broken entry should not bring down the whole
 * list.
 */
export function zoneOffset(zone: string, at: number): string {
  try {
    const name = new Intl.DateTimeFormat("en-US", { timeZone: zone, timeZoneName: "longOffset" })
      .formatToParts(new Date(at))
      .find((part) => part.type === "timeZoneName")?.value;
    if (!name) return "";
    // `longOffset` gives `GMT+07:00`, and a bare `GMT` for zones exactly at UTC.
    const offset = name.replace("GMT", "");
    return offset === "" ? "+00:00" : offset;
  } catch {
    return "";
  }
}
