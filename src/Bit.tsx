import { useEffect, useState, type ReactNode } from "react";
import type { BuddyHat, Mood } from "./types";

// Bit is an inline SVG: Astryx's Avatar is for people (a photo, initials or
// an icon), so like the Live chart this is a raw-element exception to
// AGENTS.md. Every colour comes from a theme token.

/** Each colour's label and token. Pastel shades use the text tokens, which
 * are light in Orbit's dark theme; bold shades use the icon tokens. */
export const bitColours: Record<string, { label: string; token: string }> = {
  lime: { label: "Lime", token: "--color-text-green" },
  green: { label: "Green", token: "--color-icon-green" },
  aqua: { label: "Aqua", token: "--color-text-teal" },
  teal: { label: "Teal", token: "--color-icon-teal" },
  ice: { label: "Ice", token: "--color-text-cyan" },
  cyan: { label: "Cyan", token: "--color-icon-cyan" },
  sky: { label: "Sky", token: "--color-text-blue" },
  blue: { label: "Blue", token: "--color-icon-blue" },
  lavender: { label: "Lavender", token: "--color-text-purple" },
  purple: { label: "Purple", token: "--color-icon-purple" },
  bubblegum: { label: "Bubblegum", token: "--color-text-pink" },
  pink: { label: "Pink", token: "--color-icon-pink" },
  rose: { label: "Rose", token: "--color-text-red" },
  red: { label: "Red", token: "--color-icon-red" },
  peach: { label: "Peach", token: "--color-text-orange" },
  orange: { label: "Orange", token: "--color-icon-orange" },
  gold: { label: "Gold", token: "--color-text-yellow" },
  lemon: { label: "Lemon", token: "--color-icon-yellow" },
  moon: { label: "Moon", token: "--color-text-gray" },
  grey: { label: "Grey", token: "--color-icon-gray" },
};
export const bitHats: Record<BuddyHat, string> = {
  none: "No hat",
  party: "Party hat",
  "top-hat": "Top hat",
  beanie: "Beanie",
  crown: "Crown",
  propeller: "Propeller cap",
  wizard: "Wizard hat",
  cowboy: "Cowboy hat",
  chef: "Chef's hat",
  cap: "Baseball cap",
  graduation: "Graduation cap",
  beret: "Beret",
  flower: "Flower",
  bow: "Bow",
  halo: "Halo",
  headphones: "Headphones",
  santa: "Santa hat",
  viking: "Viking helmet",
  "cat-ears": "Cat ears",
  antenna: "Antennae",
  "space-helmet": "Space helmet",
};
export const moodLabels: Record<Mood, string> = {
  happy: "Happy",
  busy: "Busy",
  worried: "Worried",
  sleepy: "Sleepy",
};

const v = (token: string) => `var(${token})`;
const fill = (token: string) => ({ fill: v(token) });
const stroke = (token: string, width = 3) => ({
  fill: "none",
  stroke: v(token),
  strokeWidth: width,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
});

/** Whether the viewer asked for less motion. */
function useReducedMotion() {
  const query = "(prefers-reduced-motion: reduce)";
  const [reduced, setReduced] = useState(
    () => typeof window !== "undefined" && !!window.matchMedia?.(query).matches,
  );
  useEffect(() => {
    const media = window.matchMedia?.(query);
    if (!media) return;
    const update = () => setReduced(media.matches);
    media.addEventListener?.("change", update);
    return () => media.removeEventListener?.("change", update);
  }, []);
  return reduced;
}

const faces: Record<Mood, ReactNode> = {
  happy: (
    <>
      <path
        className="bit-feature"
        d="M46 53 Q50 47 54 53 M66 53 Q70 47 74 53"
      />
      <path className="bit-feature" d="M53 60 Q60 67 67 60" />
    </>
  ),
  busy: (
    <>
      <circle className="bit-eye" cx="54" cy="52" r="3.6" />
      <circle className="bit-eye" cx="72" cy="52" r="3.6" />
      <path className="bit-feature" d="M57 62 L65 62" />
    </>
  ),
  worried: (
    <>
      <path className="bit-feature" d="M45 47 L55 44 M75 47 L65 44" />
      <circle className="bit-eye" cx="51" cy="53" r="3.6" />
      <circle className="bit-eye" cx="69" cy="53" r="3.6" />
      <path className="bit-feature" d="M52 63 Q56 59 60 63 Q64 67 68 63" />
    </>
  ),
  sleepy: (
    <>
      <path
        className="bit-feature"
        d="M46 52 Q50 56 54 52 M66 52 Q70 56 74 52"
      />
      <path className="bit-feature" d="M57 62 Q60 64 63 62" />
    </>
  ),
};

/** Where a hat is drawn: behind Bit, over its face under the ring, or on
 * top of everything. */
type Layer = "back" | "head" | "top";
function hat(
  name: BuddyHat,
  body: string,
  spin: boolean,
): Partial<Record<Layer, ReactNode>> {
  switch (name) {
    case "party":
      return {
        top: (
          <g transform="rotate(14 60 32)">
            <path
              d="M49 35 L60 6 L71 35 Z"
              style={fill("--color-icon-purple")}
            />
            <path
              d="M53.5 25 L66.5 25 M56.5 16 L63.5 16"
              style={stroke("--color-icon-yellow")}
            />
            <circle
              cx="60"
              cy="6"
              r="4.5"
              style={fill("--color-icon-yellow")}
            />
          </g>
        ),
      };
    case "top-hat":
      return {
        top: (
          <g transform="rotate(-8 60 32)">
            <rect
              x="42"
              y="30"
              width="36"
              height="5"
              rx="2.5"
              style={fill("--color-icon-secondary")}
            />
            <rect
              x="48"
              y="8"
              width="24"
              height="24"
              rx="2"
              style={fill("--color-icon-secondary")}
            />
            <rect
              x="48"
              y="24"
              width="24"
              height="4"
              style={fill("--color-icon-red")}
            />
          </g>
        ),
      };
    case "beanie":
      return {
        top: (
          <>
            <path
              d="M40 38 A20 20 0 0 1 80 38 Z"
              style={fill("--color-icon-red")}
            />
            <rect
              x="38"
              y="34"
              width="44"
              height="7"
              rx="3.5"
              style={fill("--color-text-red")}
            />
            <circle cx="60" cy="17" r="5" style={fill("--color-text-gray")} />
          </>
        ),
      };
    case "crown":
      return {
        top: (
          <>
            <path
              d="M45 35 L43 15 L52 23 L60 10 L68 23 L77 15 L75 35 Z"
              style={fill("--color-text-yellow")}
            />
            <circle cx="60" cy="28" r="2.6" style={fill("--color-icon-red")} />
            <circle cx="50" cy="29" r="2" style={fill("--color-icon-blue")} />
            <circle cx="70" cy="29" r="2" style={fill("--color-icon-blue")} />
          </>
        ),
      };
    case "propeller":
      return {
        top: (
          <>
            <path
              d="M45 34 A15 13 0 0 1 75 34 Z"
              style={fill("--color-icon-blue")}
            />
            <path
              d="M52 22 A15 13 0 0 1 60 21 L60 34 L52 34 Z"
              style={fill("--color-icon-red")}
            />
            <path
              d="M60 22 L60 14"
              style={stroke("--color-icon-secondary", 2.5)}
            />
            <ellipse
              cx="60"
              cy="13"
              rx="13"
              ry="3"
              style={fill("--color-icon-yellow")}
            >
              {spin && (
                <animate
                  attributeName="rx"
                  values="13;2;13"
                  dur="0.5s"
                  repeatCount="indefinite"
                />
              )}
            </ellipse>
          </>
        ),
      };
    case "wizard":
      return {
        top: (
          <>
            <path
              d="M44 35 Q56 24 58 2 Q66 22 76 35 Z"
              style={fill("--color-icon-purple")}
            />
            <ellipse
              cx="60"
              cy="35"
              rx="21"
              ry="4"
              style={fill("--color-text-purple")}
            />
            <path
              d="M63 19 L64.5 22.5 L68 24 L64.5 25.5 L63 29 L61.5 25.5 L58 24 L61.5 22.5 Z"
              style={fill("--color-icon-yellow")}
            />
          </>
        ),
      };
    case "cowboy":
      return {
        top: (
          <>
            <path
              d="M44 33 Q44 14 52 16 Q60 20 68 16 Q76 14 76 33 Z"
              style={fill("--color-icon-orange")}
            />
            <rect
              x="44"
              y="27"
              width="32"
              height="4"
              style={fill("--color-border-orange")}
            />
            <path
              d="M26 30 Q34 40 60 40 Q86 40 94 30 Q84 35 60 35 Q36 35 26 30 Z"
              style={fill("--color-icon-orange")}
            />
          </>
        ),
      };
    case "chef":
      return {
        top: (
          <g
            style={{
              ...fill("--color-text-gray"),
              stroke: v("--color-border-emphasized"),
              strokeWidth: 1.5,
            }}
          >
            <circle cx="49" cy="21" r="8" />
            <circle cx="71" cy="21" r="8" />
            <circle cx="60" cy="15" r="10" />
            <rect x="45" y="24" width="30" height="11" rx="2" />
          </g>
        ),
      };
    case "cap":
      return {
        top: (
          <>
            <path
              d="M40 36 A20 18 0 0 1 80 36 Z"
              style={fill("--color-icon-blue")}
            />
            <path
              d="M76 34 Q92 32 99 38 Q88 41 76 38 Z"
              style={fill("--color-border-blue")}
            />
            <circle
              cx="60"
              cy="18.5"
              r="2.5"
              style={fill("--color-text-blue")}
            />
          </>
        ),
      };
    case "graduation":
      return {
        top: (
          <>
            <path
              d="M44 35 L44 25 Q60 30 76 25 L76 35 Z"
              style={fill("--color-border-emphasized")}
            />
            <path
              d="M60 11 L93 20 L60 29 L27 20 Z"
              style={fill("--color-icon-secondary")}
            />
            <path
              d="M60 20 L86 24 L86 34"
              style={stroke("--color-icon-yellow", 2)}
            />
            <rect
              x="84"
              y="33"
              width="4"
              height="7"
              rx="1"
              style={fill("--color-icon-yellow")}
            />
            <circle cx="60" cy="20" r="2" style={fill("--color-icon-yellow")} />
          </>
        ),
      };
    case "beret":
      return {
        top: (
          <>
            <path d="M60 19 L63 12" style={stroke("--color-icon-red", 3)} />
            <path
              d="M36 34 Q33 20 58 18 Q87 17 85 31 Q70 37 36 34 Z"
              style={fill("--color-icon-red")}
            />
          </>
        ),
      };
    case "flower":
      return {
        top: (
          <>
            <ellipse
              cx="67"
              cy="37"
              rx="6"
              ry="3"
              transform="rotate(-30 67 37)"
              style={fill("--color-icon-green")}
            />
            {[
              [76, 23],
              [82.7, 27.8],
              [80.1, 35.7],
              [71.9, 35.7],
              [69.3, 27.8],
            ].map(([cx, cy]) => (
              <circle
                key={cx}
                cx={cx}
                cy={cy}
                r="5"
                style={fill("--color-text-pink")}
              />
            ))}
            <circle cx="76" cy="30" r="4" style={fill("--color-icon-yellow")} />
          </>
        ),
      };
    case "bow":
      return {
        top: (
          <>
            <path
              d="M60 28 L45 20 L46 37 Z"
              style={fill("--color-icon-pink")}
            />
            <path
              d="M60 28 L75 20 L74 37 Z"
              style={fill("--color-icon-pink")}
            />
            <circle cx="60" cy="28" r="4.5" style={fill("--color-text-pink")} />
          </>
        ),
      };
    case "halo":
      return {
        top: (
          <ellipse
            cx="60"
            cy="16"
            rx="18"
            ry="5"
            style={stroke("--color-icon-yellow", 4)}
          />
        ),
      };
    case "headphones":
      return {
        top: (
          <>
            <path
              d="M33 54 Q31 24 60 24 Q89 24 87 54"
              style={stroke("--color-icon-secondary", 5)}
            />
            <rect
              x="27"
              y="46"
              width="11"
              height="18"
              rx="4"
              style={fill("--color-icon-red")}
            />
            <rect
              x="82"
              y="46"
              width="11"
              height="18"
              rx="4"
              style={fill("--color-icon-red")}
            />
          </>
        ),
      };
    case "santa":
      return {
        top: (
          <>
            <path
              d="M40 35 Q47 12 66 10 Q83 10 89 25 Q79 18 71 22 Q75 28 80 35 Z"
              style={fill("--color-icon-red")}
            />
            <rect
              x="37"
              y="31"
              width="46"
              height="8"
              rx="4"
              style={fill("--color-text-gray")}
            />
            <circle cx="89" cy="26" r="5" style={fill("--color-text-gray")} />
          </>
        ),
      };
    case "viking":
      return {
        top: (
          <>
            <path
              d="M40 30 Q28 26 26 12 Q34 22 45 24 Z"
              style={fill("--color-text-orange")}
            />
            <path
              d="M80 30 Q92 26 94 12 Q86 22 75 24 Z"
              style={fill("--color-text-orange")}
            />
            <path
              d="M38 38 A22 22 0 0 1 82 38 Z"
              style={fill("--color-icon-secondary")}
            />
            <rect
              x="37"
              y="34"
              width="46"
              height="5"
              rx="2"
              style={fill("--color-border-emphasized")}
            />
          </>
        ),
      };
    case "cat-ears":
      return {
        back: (
          <>
            <path d="M40 40 L42 15 L58 32 Z" style={{ fill: body }} />
            <path d="M80 40 L78 15 L62 32 Z" style={{ fill: body }} />
            <path
              d="M44 34 L45 21 L53 30 Z"
              style={fill("--color-text-pink")}
            />
            <path
              d="M76 34 L75 21 L67 30 Z"
              style={fill("--color-text-pink")}
            />
          </>
        ),
      };
    case "antenna":
      return {
        back: (
          <path
            d="M52 34 L45 13 M68 34 L75 13"
            style={{ ...stroke("--color-icon-secondary", 3.5), stroke: body }}
          />
        ),
        top: (
          <>
            <circle cx="45" cy="12" r="5" style={fill("--color-icon-pink")} />
            <circle cx="75" cy="12" r="5" style={fill("--color-icon-pink")} />
          </>
        ),
      };
    case "space-helmet":
      return {
        head: (
          <>
            <circle
              cx="60"
              cy="57"
              r="35"
              style={{
                fill: v("--color-text-cyan"),
                fillOpacity: 0.16,
                stroke: v("--color-text-cyan"),
                strokeWidth: 3,
              }}
            />
            <path
              d="M39 40 Q45 29 57 26"
              style={{ ...stroke("--color-text-gray", 3), opacity: 0.7 }}
            />
          </>
        ),
      };
    default:
      return {};
  }
}

const RING = "M8 60 A52 17 0 0 0 112 60";
const ORBIT = "M8 60 A52 17 0 1 0 112 60 A52 17 0 1 0 8 60";

/** Bit, drawn in its colour and hat, with a face for its mood. */
export function Bit({
  mood,
  colour,
  hat: wearing = "none",
  size = "md",
  label,
}: {
  mood: Mood;
  colour: string;
  hat?: BuddyHat;
  size?: "xs" | "sm" | "md" | "lg";
  /** Names the picture; without one it is decoration. */
  label?: string;
}) {
  const reduced = useReducedMotion();
  const body = v((bitColours[colour] ?? bitColours.lime).token);
  const layers = hat(wearing, body, mood === "busy" && !reduced);
  return (
    <svg
      className={`bit bit-${size}`}
      data-mood={mood}
      viewBox="0 -15 120 120"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <g transform="rotate(-18 60 60)">
        <ellipse className="bit-ring" cx="60" cy="60" rx="52" ry="17" />
      </g>
      {layers.back}
      <circle
        className="bit-body"
        cx="60"
        cy="58"
        r="27"
        style={{ fill: body }}
      />
      {faces[mood]}
      {layers.head}
      <g transform="rotate(-18 60 60)">
        <path className="bit-ring" d={RING} />
        {mood === "busy" && (
          <circle
            className="bit-moon"
            r="5"
            cx={reduced ? 8 : 0}
            cy={reduced ? 60 : 0}
          >
            {!reduced && (
              <animateMotion dur="2.6s" repeatCount="indefinite" path={ORBIT} />
            )}
          </circle>
        )}
      </g>
      {layers.top}
      {mood === "worried" && (
        <g className="bit-badge">
          <circle cx="96" cy="28" r="10" />
          <path d="M96 22 L96 29 M96 33 L96 33.5" />
        </g>
      )}
      {mood === "sleepy" && (
        <g className="bit-zz">
          <path d="M84 24 h8 l-8 9 h8" />
          <path d="M97 12 h6 l-6 7 h6" />
        </g>
      )}
    </svg>
  );
}
