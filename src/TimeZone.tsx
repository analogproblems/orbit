import { useMemo } from "react";
import {
  Typeahead,
  type SearchableItem,
  type SearchSource,
} from "@astryxdesign/core";

/** The choice that keeps the clock of the computer Orbit runs on, saved as
 * an empty time zone. */
const computer = "computer";

/** This device's time zone, such as Europe/London, if the browser says. */
export function deviceZone(): string {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
    // What some browsers say when they can't tell.
    return zone === "Etc/Unknown" ? "" : zone;
  } catch {
    return "";
  }
}
/** Every time zone this browser knows, and UTC, which some leave out. */
function knownZones(): string[] {
  let zones: string[] = [];
  try {
    zones = Intl.supportedValuesOf("timeZone");
  } catch {
    // None listed: the suggestions and UTC still are.
  }
  return zones.includes("UTC") ? zones : [...zones, "UTC"];
}

interface Props {
  /** Bit's time zone; empty for the computer's own. */
  value: string;
  /** The time zone of the computer Orbit runs on, when it says. */
  clockZone: string;
  /** Whether Orbit runs on another computer than this device: in a browser,
   * or in the desktop app showing an always-on Orbit. */
  elsewhere: boolean;
  isDisabled?: boolean;
  onChange: (zone: string) => void;
}

/** Whose clock the morning brief keeps: the computer Orbit runs on, this
 * device's zone when that's another, or any zone you search for. */
export function TimeZoneSelector({
  value,
  clockZone,
  elsewhere,
  isDisabled,
  onChange,
}: Props) {
  const device = useMemo(() => deviceZone(), []);
  const otherDevice = elsewhere && device !== "" && device !== clockZone;
  // Searched, not listed whole: there are hundreds of zones.
  const { chosen, source } = useMemo(() => {
    const named = (label: string, zone: string) =>
      zone ? `${label} (${shown(zone)})` : label;
    const suggested: SearchableItem[] = [
      {
        id: computer,
        label: named(
          elsewhere ? "Orbit’s computer" : "This computer",
          clockZone,
        ),
      },
    ];
    if (otherDevice) {
      suggested.push({ id: device, label: named("This device", device) });
    }
    const all = knownZones();
    if (value && !all.includes(value)) all.unshift(value);
    const items = [
      ...suggested,
      ...all
        .filter((z) => !suggested.some((o) => o.id === z))
        .map((z) => ({ id: z, label: shown(z) })),
    ];
    const chosen = items.find((i) => i.id === (value || computer)) ?? null;
    // Before you type, and while the box still shows the choice: the
    // suggestions, with the choice.
    const first =
      chosen && !suggested.includes(chosen)
        ? [...suggested, chosen]
        : suggested;
    const search = (query: string) => {
      const q = shown(query.trim()).toLowerCase();
      return !q || query.trim() === chosen?.label
        ? first
        : items.filter((i) => i.label.toLowerCase().includes(q));
    };
    const source: SearchSource = { search, bootstrap: () => first };
    return { chosen, source };
  }, [value, clockZone, elsewhere, device, otherDevice]);
  // Said while Orbit keeps another zone's clock than yours.
  const mismatch = otherDevice && !value && clockZone !== "";
  return (
    <Typeahead
      label="Time zone"
      description={
        mismatch
          ? `Orbit’s computer keeps ${shown(clockZone)} time, and this device keeps ${shown(device)}. Choose yours, so the brief comes at your time.`
          : "The brief comes at its time in this time zone."
      }
      placeholder="Search, such as London or New York"
      width="100%"
      searchSource={source}
      value={chosen}
      onChange={(item) => {
        if (item) onChange(item.id === computer ? "" : item.id);
      }}
      hasEntriesOnFocus
      minQueryLength={0}
      maxMenuItems={20}
      debounceMs={0}
      hasClear={false}
      isDisabled={isDisabled}
    />
  );
}
/** A zone as people read it: America/New_York as America/New York. */
function shown(zone: string) {
  return zone.replace(/_/g, " ");
}
