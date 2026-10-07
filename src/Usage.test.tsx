// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { reachedMessage, resetLabel, UsageMeter, windowLabel } from "./Usage";
import type { Usage } from "./types";
afterEach(cleanup);

const now = Date.UTC(2026, 9, 7, 12, 0, 0);
const seconds = (ms: number) => Math.floor(ms / 1000);
const usage = (): Usage => ({
  rateLimits: {
    limitId: "codex",
    primary: {
      usedPercent: 10,
      windowDurationMins: 300,
      resetsAt: seconds(now + 2 * 3600 * 1000),
    },
    secondary: {
      usedPercent: 42,
      windowDurationMins: 10080,
      resetsAt: seconds(now + 4 * 86400 * 1000),
    },
    planType: "pro",
    credits: { hasCredits: true, unlimited: false, balance: "12.50" },
  },
  otherLimits: [
    {
      limitId: "codex_other",
      limitName: "GPT-5.5 Codex",
      primary: { usedPercent: 3, windowDurationMins: 300 },
    },
  ],
  updatedAt: new Date(now).toISOString(),
});
const weekday = (ms: number) =>
  new Date(ms).toLocaleDateString(undefined, { weekday: "short" });

describe("usage meter", () => {
  it("names each window by its length", () => {
    expect(windowLabel({ usedPercent: 0, windowDurationMins: 300 }, "x")).toBe(
      "5h",
    );
    expect(
      windowLabel({ usedPercent: 0, windowDurationMins: 10080 }, "x"),
    ).toBe("Week");
    expect(windowLabel({ usedPercent: 0, windowDurationMins: 1440 }, "x")).toBe(
      "Day",
    );
    expect(windowLabel({ usedPercent: 0, windowDurationMins: 90 }, "x")).toBe(
      "90m",
    );
    expect(windowLabel({ usedPercent: 0 }, "Week")).toBe("Week");
  });
  it("says when a window resets", () => {
    const later = now + 4 * 86400 * 1000;
    expect(resetLabel({ usedPercent: 0, resetsAt: seconds(later) }, now)).toBe(
      `resets ${weekday(later)}`,
    );
    expect(
      resetLabel({ usedPercent: 0, resetsAt: seconds(now) - 5 }, now),
    ).toBe("resetting now");
    expect(resetLabel({ usedPercent: 0 }, now)).toBeNull();
  });
  it("shows the weekly and short-term windows, with details on request", () => {
    render(<UsageMeter usage={usage()} now={now} />);
    const week = `Week 42% · resets ${weekday(now + 4 * 86400 * 1000)}`;
    // The details repeat these lines, so each shows at least once.
    expect(screen.getAllByText(week).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/^5h 10% · resets /).length).toBeGreaterThan(0);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(
      "42",
    );
    fireEvent.click(screen.getByRole("button", { name: "Usage details" }));
    expect(screen.getByText("Pro")).toBeTruthy();
    expect(screen.getByText("GPT-5.5 Codex")).toBeTruthy();
    expect(screen.getByText("12.50")).toBeTruthy();
  });
  it("is hidden when the account has no usage windows", () => {
    const { container } = render(<UsageMeter usage={null} />);
    expect(container.textContent).toBe("");
  });
  it("warns when a limit is reached", () => {
    const u = usage();
    expect(reachedMessage(u)).toBeNull();
    u.rateLimits.rateLimitReachedType = "rate_limit_reached";
    expect(reachedMessage(u)).toBe("Your Codex usage limit is reached.");
    render(<UsageMeter usage={u} now={now} />);
    expect(screen.getByText("Limit reached")).toBeTruthy();
  });
});
