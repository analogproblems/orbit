// @vitest-environment jsdom
import "./test-setup";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { Live, activity, elapsed, repoColors } from "./Live";
import { demoWorkspace, freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { Review, Snapshot } from "./types";
afterEach(cleanup);

function show(snapshot: Snapshot) {
  useOrbit.setState({ snapshot });
  const go = {
    onChoose: vi.fn(),
    onRepositories: vi.fn(),
    onSettings: vi.fn(),
    onApproval: vi.fn(),
    onPublish: vi.fn(),
    onOperations: vi.fn(),
  };
  render(<Live {...go} />);
  return go;
}
const now = new Date(2026, 9, 7, 15, 30).getTime();
const at = (hoursAgo: number) =>
  new Date(now - hoursAgo * 3600_000).toISOString();
function review(id: string, prId: string, createdAt: string): Review {
  return {
    ...freshDemo().reviews[0],
    id,
    prId,
    createdAt,
    status: "posted",
  };
}

describe("activity", () => {
  const reviews = [
    review("a", "acme/orbit#1", at(0.2)),
    review("b", "acme/orbit#1", at(0.4)),
    review("c", "acme/api#2", at(0.1)),
    review("d", "acme/api#2", at(3)),
    review("e", "acme/api#3", at(30)),
    { ...review("chat", "acme/api#3", at(0.1)), isReview: false },
  ];
  it("counts reviews per hour by repository, oldest hour first", () => {
    const buckets = activity(reviews, "hour", "reviews", now);
    expect(buckets).toHaveLength(24);
    expect(buckets.at(-1)).toMatchObject({
      total: 3,
      counts: { "acme/orbit": 2, "acme/api": 1 },
    });
    expect(buckets.at(-4)?.total).toBe(1);
    expect(buckets.reduce((n, b) => n + b.total, 0)).toBe(4);
  });
  it("counts each PR once per bucket and spans 14 days per day", () => {
    expect(activity(reviews, "hour", "prs", now).at(-1)?.total).toBe(2);
    const days = activity(reviews, "day", "prs", now);
    expect(days).toHaveLength(14);
    expect(days.at(-1)?.total).toBe(2);
    expect(days.at(-2)?.counts).toEqual({ "acme/api": 1 });
  });
});

describe("activity across clock changes", () => {
  // Node applies a changed TZ to later Date calls.
  beforeEach(() => {
    vi.stubEnv("TZ", "America/New_York");
  });
  afterEach(() => {
    vi.unstubAllEnvs();
  });
  const utc = (iso: string) => Date.parse(iso);
  function steady(starts: number[], step = 3600_000) {
    expect(new Set(starts).size).toBe(starts.length);
    starts.slice(1).forEach((t, i) => expect(t - starts[i]).toBe(step));
  }
  it("keeps 24 distinct hours when the clocks spring forward", () => {
    // 2026-03-08: 2 AM EST jumps to 3 AM EDT (07:00Z).
    const now = utc("2026-03-08T16:30:00Z");
    const before = review("a", "acme/orbit#1", "2026-03-08T06:30:00Z");
    const after = review("b", "acme/orbit#1", "2026-03-08T07:30:00Z");
    const old = review("c", "acme/orbit#1", "2026-03-07T16:59:00Z");
    const buckets = activity([before, after, old], "hour", "reviews", now);
    expect(buckets).toHaveLength(24);
    steady(buckets.map((b) => b.start));
    expect(buckets.at(-1)!.start).toBe(utc("2026-03-08T16:00:00Z"));
    expect(buckets[0].start).toBe(utc("2026-03-07T17:00:00Z"));
    const hit = buckets.filter((b) => b.total);
    expect(hit.map((b) => b.start)).toEqual([
      utc("2026-03-08T06:00:00Z"),
      utc("2026-03-08T07:00:00Z"),
    ]);
  });
  it("keeps the repeated hour when the clocks fall back", () => {
    // 2026-11-01: 2 AM EDT falls back to 1 AM EST (06:00Z); now is the
    // second 1:30 AM.
    const now = utc("2026-11-01T06:30:00Z");
    const first = review("a", "acme/orbit#1", "2026-11-01T05:15:00Z");
    const second = review("b", "acme/api#2", "2026-11-01T06:15:00Z");
    const buckets = activity([first, second], "hour", "reviews", now);
    steady(buckets.map((b) => b.start));
    expect(buckets.at(-1)).toMatchObject({
      start: utc("2026-11-01T06:00:00Z"),
      counts: { "acme/api": 1 },
    });
    expect(buckets.at(-2)).toMatchObject({
      start: utc("2026-11-01T05:00:00Z"),
      counts: { "acme/orbit": 1 },
    });
  });
  it("counts a short or long day as one day", () => {
    for (const iso of ["2026-03-15T12:00:00Z", "2026-11-08T12:00:00Z"]) {
      const starts = activity([], "day", "reviews", utc(iso)).map(
        (b) => b.start,
      );
      expect(starts).toHaveLength(14);
      expect(new Set(starts.map((t) => new Date(t).getHours()))).toEqual(
        new Set([0]),
      );
      starts
        .slice(1)
        .forEach((t, i) =>
          expect(new Date(t).getDate()).not.toBe(new Date(starts[i]).getDate()),
        );
    }
  });
});

describe("live page", () => {
  it("keeps each repository's colour as others come and go", () => {
    const s = freshDemo();
    const colorOf = repoColors(s);
    expect(colorOf("acme/orbit").color).not.toBe(colorOf("acme/api").color);
    s.settings.repositories.reverse();
    expect(repoColors(s)("acme/orbit").series).toBe("acme/orbit");
    expect(colorOf("someone/else").series).toBe("Other");
  });
  it("puts what needs you first, with its action", () => {
    const action = vi.fn().mockResolvedValue({ ok: true });
    useOrbit.setState({ action });
    const s = freshDemo();
    s.codexReady = false;
    const go = show(s);
    const panel = screen.getByRole("region", { name: "Needs you" });
    expect(
      within(panel).getByRole("heading", { name: "6 things need you" }),
    ).toBeTruthy();
    expect(within(panel).getByText("Codex is disconnected")).toBeTruthy();
    fireEvent.click(
      within(panel).getByRole("button", { name: "Continue loop" }),
    );
    expect(action).toHaveBeenCalledWith("continue_loop", {
      prId: s.prs[1].id,
      rounds: 2,
    });
    fireEvent.click(
      within(panel).getAllByRole("button", { name: "Open settings" })[0],
    );
    expect(go.onSettings).toHaveBeenCalled();
    // A failing default branch leads to the Operations page, as do
    // security alerts.
    expect(
      within(panel).getAllByRole("button", { name: "Open operations" }),
    ).toHaveLength(3);
    fireEvent.click(
      within(panel).getAllByRole("button", { name: "Open operations" })[0],
    );
    expect(go.onOperations).toHaveBeenCalled();
    cleanup();
    // So do runs waiting for a runner.
    s.attention = [
      {
        key: "problem:waits:1",
        kind: "runs_waiting",
        prId: null,
        reviewId: null,
        approvalId: null,
        repo: "acme/api",
        reason:
          "A run of Release in acme/api has waited more than 30 minutes for a runner.",
      },
    ];
    const again = show(s);
    const waiting = screen.getByRole("region", { name: "Needs you" });
    expect(waiting.textContent).toContain("waited more than 30 minutes");
    fireEvent.click(
      within(waiting).getByRole("button", { name: "Open operations" }),
    );
    expect(again.onOperations).toHaveBeenCalled();
  });
  it("says all clear when nothing needs you", () => {
    const s = freshDemo();
    s.attention = [];
    show(s);
    expect(screen.getByText("All clear. Nothing needs you.")).toBeTruthy();
    expect(screen.getByText("Nothing running")).toBeTruthy();
  });
  it("lists running reviews with a timer and opens their chat", () => {
    const s = freshDemo();
    const r = s.reviews[0];
    r.status = "reviewing";
    r.createdAt = new Date(Date.now() - 125_000).toISOString();
    const go = show(s);
    const list = screen.getByRole("list", { name: "Running reviews" });
    expect(within(list).getByText(s.prs[0].title)).toBeTruthy();
    expect(within(list).getByLabelText(/^Running for 2:0\d$/)).toBeTruthy();
    fireEvent.click(within(list).getByRole("button"));
    expect(go.onChoose).toHaveBeenCalledWith(s.prs[0], r);
  });
  it("charts the demo history and describes a bar on focus", () => {
    show(demoWorkspace());
    const chart = screen.getByRole("img", { name: "Reviews per hour" });
    const bars = chart.querySelectorAll("rect[tabindex]");
    expect(bars).toHaveLength(24);
    fireEvent.click(screen.getByRole("radio", { name: "Per day" }));
    const days = screen
      .getByRole("img", { name: "Reviews per day" })
      .querySelectorAll("rect[tabindex]");
    expect(days).toHaveLength(14);
    const busiest = [...days].find((b) =>
      / · \d+ reviews · /.test(b.getAttribute("aria-label") ?? ""),
    )!;
    fireEvent.focus(busiest);
    expect(screen.getByText(busiest.getAttribute("aria-label")!)).toBeTruthy();
  });
});

describe("elapsed", () => {
  it("shows minutes and seconds, then hours", () => {
    expect(elapsed(5_000)).toBe("0:05");
    expect(elapsed(125_000)).toBe("2:05");
    expect(elapsed(3_725_000)).toBe("1:02:05");
  });
});
