// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import options from "../test-fixtures/buddy-options.json";
import {
  Bit,
  bitColours,
  bitHats,
  bitLooks,
  moodLabels,
  orbMoods,
} from "./Bit";
import { ORB_MOODS } from "./orbs";
import { describePatch } from "./describePatch";
import { freshDemo } from "./demo";
import {
  actionLabels,
  buddyAnswerers,
  buddyMood,
  buddyTones,
  defaultBuddy,
  fixLabels,
  fixRuleLabels,
  postLabels,
  memoryLimits,
  questionLabels,
  type BuddyHat,
  type BuddyLook,
  type Mood,
} from "./types";
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const moods = Object.keys(moodLabels) as Mood[];

/** A canvas that draws nothing but lets the orbs run, in a picture `width`
 * across. */
function fakeDrawing(width = 80) {
  const ctx = new Proxy({} as Record<string, unknown>, {
    get: (held, key: string) =>
      key in held
        ? held[key]
        : () =>
            key === "getImageData"
              ? { data: [220, 90, 160, 255] }
              : { addColorStop() {} },
    set: (held, key: string, value) => ((held[key] = value), true),
  });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    () => ctx as unknown as CanvasRenderingContext2D,
  );
  const rect = vi
    .spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockReturnValue({ width } as DOMRect);
  return {
    resize: (w: number) => rect.mockReturnValue({ width: w } as DOMRect),
  };
}

/** Animation frames that run only when the test says so. */
function fakeFrames() {
  const frames = new Map<number, FrameRequestCallback>();
  let id = 0;
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    frames.set(++id, cb);
    return id;
  });
  vi.stubGlobal("cancelAnimationFrame", (n: number) => frames.delete(n));
  return {
    waiting: () => frames.size,
    run(now: number) {
      const due = [...frames.values()];
      frames.clear();
      for (const cb of due) cb(now);
    },
  };
}
const hats = Object.keys(bitHats) as BuddyHat[];

describe("Bit", () => {
  it("isn't worried by the morning brief alone", () => {
    const s = freshDemo();
    delete s.mood;
    const brief = {
      key: "brief:2026-10-31",
      kind: "brief" as const,
      prId: null,
      reviewId: null,
      approvalId: null,
      repo: null,
      reason: "Here's your morning brief!",
    };
    s.attention = [brief];
    expect(buddyMood(s)).not.toBe("worried");
    s.attention = [
      brief,
      { ...brief, key: "sync:github", kind: "sync_failing" },
    ];
    expect(buddyMood(s)).toBe("worried");
  });
  it("offers the same colours, hats and tones as the engine", () => {
    expect(Object.keys(bitColours)).toEqual(options.colours);
    expect(hats).toEqual(options.hats);
    expect(Object.keys(buddyTones)).toEqual(options.tones);
    expect(Object.keys(buddyAnswerers)).toEqual(options.answerers);
    expect(describePatch({ buddy: { answerer: "antigravity" } }, [])).toBe(
      "Who answers your questions: Antigravity",
    );
    expect(
      describePatch({ buddy: { ollamaUrl: "http://jupiter:11434" } }, []),
    ).toBe("Ollama address: http://jupiter:11434");
    expect(
      describePatch(
        { buddy: { look: "fizz", colour: "own", hat: "top-hat" } },
        [],
      ),
    ).toBe(
      "Buddy's look: Fizz; Buddy's colour: Its own colours; Buddy's hat: Top hat",
    );
    expect(
      describePatch({ buddy: { brief: false, briefAt: "07:30" } }, []),
    ).toBe("Morning brief: Off; Brief time: 07:30");
    expect(Object.keys(fixRuleLabels)).toEqual(options.rules);
    expect(Object.keys(fixLabels)).toEqual(options.fixes);
    expect(Object.keys(questionLabels)).toEqual(options.questions);
    expect(Object.keys(actionLabels)).toEqual([
      ...options.actions,
      ...options.helpActions,
    ]);
    expect(memoryLimits).toEqual(options.memoryLimits);
    // Every fix but a settings change, checking connections or posting
    // again where another Orbit posts has a rule,
    // which starts on Ask first, and so does each post that tells an agent
    // about failing CI, which starts on Do it, under the same names the
    // engine saves and accepts.
    expect(Object.keys(defaultBuddy.rules)).toEqual([
      ...options.fixes.filter(
        (f) =>
          f !== "turn_on_autopilot" &&
          f !== "check_connections" &&
          f !== "post_here_again",
      ),
      ...options.posts,
    ]);
    expect(Object.keys(postLabels)).toEqual(options.posts);
    expect(defaultBuddy.rules).toEqual(options.defaultRules);
  });

  it("draws every hat in every mood with theme colours only", () => {
    const sizes = new Map<BuddyHat, number>();
    for (const hat of hats)
      for (const mood of moods) {
        const { container, unmount } = render(
          <Bit mood={mood} colour="gold" hat={hat} />,
        );
        const svg = container.querySelector("svg")!;
        expect(svg.dataset.mood).toBe(mood);
        for (const el of svg.querySelectorAll("[style]")) {
          const style = el.getAttribute("style")!;
          expect(style).not.toMatch(/#|rgb|\dpx/);
          expect(style).toMatch(/var\(--color-/);
        }
        if (mood === "happy") sizes.set(hat, svg.querySelectorAll("*").length);
        unmount();
      }
    for (const hat of hats.filter((h) => h !== "none"))
      expect(sizes.get(hat)).toBeGreaterThan(sizes.get("none")!);
  });

  it("uses the chosen colour, falling back to lime for one it doesn't know", () => {
    const body = (colour: string) => {
      const { container, unmount } = render(
        <Bit mood="happy" colour={colour} />,
      );
      const fill = container.querySelector<SVGElement>(".bit-body")!.style.fill;
      unmount();
      return fill;
    };
    expect(body("purple")).toBe("var(--color-icon-purple)");
    expect(body("plaid")).toBe("var(--color-text-green)");
  });

  it("is a named picture with a label and decoration without one", () => {
    render(<Bit mood="worried" colour="lime" label="Bit, worried" />);
    expect(screen.getByRole("img", { name: "Bit, worried" })).toBeTruthy();
    cleanup();
    const { container } = render(<Bit mood="worried" colour="lime" />);
    expect(container.querySelector("svg")!.getAttribute("aria-hidden")).toBe(
      "true",
    );
  });

  it("keeps its moon and propeller still when less motion is asked for", () => {
    const motion = (reduce: boolean) => {
      vi.stubGlobal("matchMedia", (query: string) => ({
        matches: reduce && query.includes("reduce"),
        addEventListener: () => {},
        removeEventListener: () => {},
      }));
      const { container, unmount } = render(
        <Bit mood="busy" colour="lime" hat="propeller" />,
      );
      const moving = container.querySelectorAll(
        "animate, animateMotion",
      ).length;
      expect(container.querySelector(".bit-moon")).toBeTruthy();
      unmount();
      return moving;
    };
    expect(motion(false)).toBe(2);
    expect(motion(true)).toBe(0);
  });

  it("offers the same looks as the engine, with an orb mood for each of Bit's", () => {
    expect(Object.keys(bitLooks)).toEqual(options.looks);
    expect(defaultBuddy.look).toBe("glim");
    expect(Object.keys(orbMoods)).toEqual(moods);
    const used = Object.values(orbMoods);
    expect(new Set(used).size).toBe(moods.length);
    for (const m of used) expect(ORB_MOODS).toContain(m);
  });

  it("draws an orb on a canvas, with its hat in a picture over it", () => {
    render(
      <Bit
        mood="busy"
        colour="pink"
        hat="crown"
        look="fizz"
        label="Bit, busy"
      />,
    );
    const box = screen.getByRole("img", { name: "Bit, busy" });
    expect(box.dataset.look).toBe("fizz");
    expect(box.dataset.mood).toBe("busy");
    expect(box.querySelector("canvas")).toBeTruthy();
    const layers = box.querySelectorAll(".bit-hat > g");
    expect(layers).toHaveLength(2);
    expect(layers[1].children.length).toBeGreaterThan(0);
    // With nothing drawn, nothing says where its head is, so the hat waits.
    for (const g of layers) expect(g.getAttribute("visibility")).toBe("hidden");
    cleanup();
    const { container } = render(
      <Bit mood="happy" colour="lime" look={"blob" as BuddyLook} />,
    );
    expect(container.querySelector<HTMLElement>(".bit")!.dataset.look).toBe(
      "glim",
    );
  });

  it("puts the hat on the orb's head once it's drawn", () => {
    fakeDrawing();
    const { container } = render(
      <Bit mood="worried" colour="own" hat="wizard" look="pip" still />,
    );
    for (const g of container.querySelectorAll(".bit-hat > g")) {
      expect(g.hasAttribute("visibility")).toBe(false);
      expect(g.getAttribute("transform")).toMatch(
        /^translate\([\d.-]+ [\d.-]+\) rotate\([\d.e-]+\) scale\([\d.]+\) translate\(-60 -31\)$/,
      );
    }
  });

  it("moves every orb on one clock, which stops with the last one", () => {
    fakeDrawing();
    const frames = fakeFrames();
    const first = render(
      <Bit mood="busy" colour="lime" hat="crown" look="pip" />,
    );
    const second = render(<Bit mood="happy" colour="pink" look="gem" />);
    expect(frames.waiting()).toBe(1);
    frames.run(1000);
    expect(frames.waiting()).toBe(1);
    const crown = first.container.querySelector(".bit-hat > g")!;
    expect(crown.hasAttribute("visibility")).toBe(false);
    const was = crown.getAttribute("transform");
    frames.run(1050);
    expect(crown.getAttribute("transform")).not.toBe(was);
    first.unmount();
    expect(frames.waiting()).toBe(1);
    second.unmount();
    expect(frames.waiting()).toBe(0);
  });

  it("holds still once less motion is asked for, and redraws at a new size", () => {
    const drawing = fakeDrawing();
    const frames = fakeFrames();
    let reduce = false;
    const changed = new Set<() => void>();
    vi.stubGlobal("matchMedia", (query: string) => ({
      get matches() {
        return reduce && query.includes("reduce");
      },
      addEventListener: (_: string, fn: () => void) => changed.add(fn),
      removeEventListener: (_: string, fn: () => void) => changed.delete(fn),
    }));
    const { container } = render(
      <Bit mood="busy" colour="lime" look="twirl" />,
    );
    expect(frames.waiting()).toBe(1);
    act(() => {
      reduce = true;
      for (const fn of changed) fn();
    });
    frames.run(1000);
    expect(frames.waiting()).toBe(0);
    // A picture that holds still is drawn again when its size changes.
    const canvas = container.querySelector("canvas")!;
    expect(canvas.width).toBe(120);
    let resized: ResizeObserverCallback | undefined;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(cb: ResizeObserverCallback) {
          resized = cb;
        }
        observe() {}
        disconnect() {}
      },
    );
    cleanup();
    const still = render(
      <Bit mood="sleepy" colour="lime" look="twirl" still />,
    );
    drawing.resize(40);
    resized!([], {} as ResizeObserver);
    expect(still.container.querySelector("canvas")!.width).toBe(60);
  });

  it("holds the classic look still in a list of choices too", () => {
    const { container } = render(
      <Bit mood="busy" colour="lime" hat="propeller" still />,
    );
    expect(container.querySelectorAll("animate, animateMotion")).toHaveLength(
      0,
    );
  });
});
