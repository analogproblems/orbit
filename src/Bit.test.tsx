// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import options from "../test-fixtures/buddy-options.json";
import { Bit, bitColours, bitHats, moodLabels } from "./Bit";
import { describePatch } from "./describePatch";
import {
  actionLabels,
  buddyAnswerers,
  buddyTones,
  defaultBuddy,
  fixLabels,
  fixRuleLabels,
  memoryLimits,
  questionLabels,
  type BuddyHat,
  type Mood,
} from "./types";
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const moods = Object.keys(moodLabels) as Mood[];
const hats = Object.keys(bitHats) as BuddyHat[];

describe("Bit", () => {
  it("offers the same colours, hats and tones as the engine", () => {
    expect(Object.keys(bitColours)).toEqual(options.colours);
    expect(hats).toEqual(options.hats);
    expect(Object.keys(buddyTones)).toEqual(options.tones);
    expect(Object.keys(buddyAnswerers)).toEqual(options.answerers);
    expect(describePatch({ buddy: { answerer: "antigravity" } }, [])).toBe(
      "Who answers your questions: Antigravity",
    );
    expect(Object.keys(fixRuleLabels)).toEqual(options.rules);
    expect(Object.keys(fixLabels)).toEqual(options.fixes);
    expect(Object.keys(questionLabels)).toEqual(options.questions);
    expect(Object.keys(actionLabels)).toEqual([
      ...options.actions,
      ...options.helpActions,
    ]);
    expect(memoryLimits).toEqual(options.memoryLimits);
    // Every fix but a settings change or checking connections has a rule,
    // which starts on Ask first, under the same names the engine saves and
    // accepts.
    expect(Object.keys(defaultBuddy.rules)).toEqual(
      options.fixes.filter(
        (f) => f !== "turn_on_autopilot" && f !== "check_connections",
      ),
    );
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
});
