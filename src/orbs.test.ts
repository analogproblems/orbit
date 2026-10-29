import { describe, expect, it } from "vitest";
import {
  drawOrb,
  LOOKS,
  lookNames,
  Orb,
  ORB_MOODS,
  ownPalette,
  tint,
  type Env,
} from "./orbs";

const COLOUR = /^(#[0-9a-f]{3,8}|(rgb|hsl)a?\([\d., %-]+\)|transparent)$/i;

/** A canvas that checks what it's asked to draw the way a browser would:
 * no NaN or infinite numbers, no negative radius and no colour it can't
 * read, any of which throws or draws nothing in a real one. */
function fakeCanvas() {
  const problems = new Set<string>();
  let calls = 0;
  const check = (what: string, ok: boolean) => {
    if (!ok) problems.add(what);
  };
  const colour = (what: string, c: unknown) =>
    check(
      `${what} ${String(c)}`,
      typeof c !== "string" || (COLOUR.test(c) && !c.includes("NaN")),
    );
  const gradient = {
    addColorStop(offset: number, c: string) {
      check(`stop at ${offset}`, offset >= 0 && offset <= 1);
      colour("stop", c);
    },
  };
  const radii: Record<string, number[]> = {
    arc: [2],
    ellipse: [2, 3],
    createRadialGradient: [2, 5],
  };
  const ctx = new Proxy({ globalAlpha: 1 } as Record<string, unknown>, {
    get(held, key: string) {
      if (key in held) return held[key];
      return (...args: unknown[]) => {
        calls++;
        for (const a of args)
          if (typeof a === "number") check(`${key} ${a}`, Number.isFinite(a));
        for (const i of radii[key] ?? [])
          check(`${key} radius ${args[i]}`, (args[i] as number) >= 0);
        if (key.startsWith("create")) return gradient;
      };
    },
    set(held, key: string, value: unknown) {
      held[key] = value;
      if (key === "fillStyle" || key === "strokeStyle") colour(key, value);
      if (key === "lineWidth" || key === "globalAlpha")
        check(`${key} ${value}`, Number.isFinite(value) && Number(value) >= 0);
      return true;
    },
  }) as unknown as CanvasRenderingContext2D;
  return { ctx, problems, calls: () => calls };
}

const env: Env = {
  dark: true,
  faces: true,
  pool: true,
  shadow: "0,0,0",
  z: "rgba(163,170,240,1)",
  dash: "rgba(140,150,166,1)",
};

describe("orbs", () => {
  it("names every look", () => {
    expect(LOOKS).toHaveLength(16);
    for (const look of LOOKS) {
      expect(lookNames[look].name).toMatch(/^[A-Z]/);
      expect(lookNames[look].kind).toMatch(/^the /);
    }
  });

  it("tints a colour into four readable shades", () => {
    for (const [r, g, b] of [
      [117, 201, 72],
      [255, 255, 255],
      [0, 0, 0],
      [140, 150, 166],
    ]) {
      const pal = tint(r, g, b);
      expect(Object.keys(pal).sort()).toEqual(["D", "G", "L", "M"]);
      for (const hex of Object.values(pal))
        expect(hex).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it("draws every look in every mood, held still and moving between them", () => {
    const pals = [undefined, tint(212, 96, 160)];
    for (const look of LOOKS)
      for (const pal of pals) {
        const { ctx, problems, calls } = fakeCanvas();
        const orb = new Orb(look, "idle", pal ?? ownPalette(look));
        for (const mood of ORB_MOODS) {
          orb.set(look, mood, pal ?? ownPalette(look));
          orb.pose();
          const head = drawOrb(ctx, orb, 100, 190, 48, env);
          for (const n of [head.x, head.y, head.angle, head.size])
            expect(Number.isFinite(n), `${look} ${mood} head`).toBe(true);
          expect(head.size).toBeGreaterThan(0);
          // A couple of seconds of motion, including the spring's overshoot
          // on arrival, and a tap.
          if (mood === "idle") orb.boop();
          for (let f = 0; f < 120; f++) {
            orb.step(1 / 60);
            if (f % 10 === 0) drawOrb(ctx, orb, 100, 190, 48, env);
          }
        }
        expect([...problems], `${look} ${pal ? "tinted" : "own"}`).toEqual([]);
        expect(calls()).toBeGreaterThan(0);
      }
  });

  it("keeps its head above its body", () => {
    const { ctx } = fakeCanvas();
    for (const look of LOOKS) {
      const orb = new Orb(look, "idle");
      orb.pose();
      const head = drawOrb(ctx, orb, 100, 190, 48, env);
      expect(head.y, look).toBeLessThan(190);
      expect(Math.abs(head.x - 100), look).toBeLessThan(48);
    }
  });
});
