// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { DecisionsPage, roundStrip } from "./Decisions";
import { demoDecisions } from "./demoDecisions";
import {
  gist,
  health,
  pushback,
  searchHistories,
  worthALook,
  type Decisions,
} from "./decisions";
import { useOrbit } from "./store";
import type { ActionResult } from "./types";
afterEach(cleanup);

function answering(decisions: Decisions | Error) {
  const original = useOrbit.getState().action;
  const calls: string[] = [];
  const action = vi.fn(async (command: string) => {
    calls.push(command);
    if (decisions instanceof Error)
      return { ok: false, error: decisions.message } as ActionResult<unknown>;
    return {
      ok: true,
      data: command === "get_decisions" ? decisions : undefined,
    } as ActionResult<unknown>;
  });
  useOrbit.setState({ action: action as unknown as typeof original });
  return { calls, restore: () => useOrbit.setState({ action: original }) };
}
const byNumber = (n: number) =>
  demoDecisions().prs.find((p) => p.number === n)!;

describe("decisions summaries", () => {
  it("puts flags that need a decision first", () => {
    const rows = worthALook(demoDecisions().prs);
    expect(rows.map((r) => r.flag.attention)).toEqual([
      "act",
      "act",
      "look",
      "look",
    ]);
    expect(rows[0].pr.number).toBe(318);
  });

  it("lists each declined finding with the reviewer's answer", () => {
    const rows = pushback(demoDecisions().prs);
    expect(
      rows.map((r) => [r.pr.number, r.trajectory.shortId, r.ruling?.status]),
    ).toEqual([
      [318, "F2", "declined_disputed"],
      [142, "F2", "declined_accepted"],
    ]);
    expect(rows[0].reason).toContain("nightly rotation job");
  });

  it("counts rounds, first-round cleans and where findings ended up", () => {
    const h = health(demoDecisions().prs);
    expect(h.reviewed).toBe(5);
    expect(h.rounds).toBe(8);
    expect(h.cleanFirst).toBe(2);
    expect(h.findings).toBe(6);
    expect(h.fixed).toBe(2);
    expect(h.declined).toBe(2);
    expect(h.disputed).toBe(1);
    expect(h.openAtMerge).toBe(2);
    expect(h.roundsPerPr.map((r) => r.count)).toEqual([3, 1, 1, 0, 0]);
    expect(h.byPriority[1]).toMatchObject({
      total: 3,
      counts: { fixed: 1, declined_disputed: 1, open: 1 },
    });
  });

  it("shows blocking findings round by round and the change in a line", () => {
    expect(roundStrip(byNumber(142))).toBe("1 → 1 → 0");
    expect(roundStrip(byNumber(77))).toBe("0");
    expect(gist(byNumber(142))).toBe(
      "Saving defaults keeps every repository's overrides; only the defaults change.",
    );
    expect(gist(byNumber(75))).toMatch(/^Routine update/);
  });

  it("searches titles, numbers and finding titles", () => {
    const prs = demoDecisions().prs;
    expect(searchHistories(prs, "webhook").map((p) => p.number)).toEqual([318]);
    expect(searchHistories(prs, "orbit#139").map((p) => p.number)).toEqual([
      139,
    ]);
    expect(
      searchHistories(prs, "retry loop gives").map((p) => p.number),
    ).toEqual([139]);
    expect(searchHistories(prs, "  ")).toHaveLength(prs.length);
  });
});

describe("decisions page", () => {
  it("shows the story, opens a chapter and goes back", async () => {
    const { calls, restore } = answering(demoDecisions());
    render(<DecisionsPage />);
    expect(
      await screen.findByText("Keep repository overrides when saving defaults"),
    ).toBeTruthy();
    expect(calls).toContain("get_decisions");
    expect(
      screen.getByText(
        "Merged while F1 (The retry loop never gives up) was still open.",
      ),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByText("Keep repository overrides when saving defaults"),
    );
    expect(await screen.findByText("Part of #30 · stage 2")).toBeTruthy();
    expect(screen.getByText("Round 3 · Clean")).toBeTruthy();
    const findings = screen.getByRole("table");
    expect(within(findings).getByText("Fixed in 4c1d2e9")).toBeTruthy();
    expect(
      within(findings).getByText(/^Declined: the settings round-trip test/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "All pull requests" }));
    expect(await screen.findByText("Worth a look")).toBeTruthy();
    restore();
  });

  it("switches to pushback and review health", async () => {
    const { restore } = answering(demoDecisions());
    render(<DecisionsPage />);
    await screen.findByText("Show build times per branch");
    fireEvent.click(screen.getByRole("tab", { name: "Pushback" }));
    expect(
      await screen.findByText(
        "The builder said: the nightly rotation job expires the old secret after a day",
      ),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Review health" }));
    expect(await screen.findByText("Clean on the first round")).toBeTruthy();
    expect(screen.getByText("Rounds per pull request")).toBeTruthy();
    expect(screen.getAllByRole("table")).toHaveLength(2);
    restore();
  });

  it("narrows by repository and search", async () => {
    const { restore } = answering(demoDecisions());
    render(<DecisionsPage />);
    await screen.findByText("Show build times per branch");
    fireEvent.change(screen.getByPlaceholderText("Search pull requests…"), {
      target: { value: "audit" },
    });
    await waitFor(() =>
      expect(screen.queryByText("Show build times per branch")).toBeNull(),
    );
    expect(screen.getByText("Paginate the audit log")).toBeTruthy();
    restore();
  });

  it("says when nothing has been read and when loading fails", async () => {
    let { restore } = answering({ prs: [], problems: [] });
    render(<DecisionsPage />);
    expect(await screen.findByText("No pull requests read yet")).toBeTruthy();
    restore();
    cleanup();
    ({ restore } = answering(new Error("Invalid repository name.")));
    render(<DecisionsPage />);
    expect(await screen.findByText("Couldn't load the decisions")).toBeTruthy();
    restore();
  });
});
