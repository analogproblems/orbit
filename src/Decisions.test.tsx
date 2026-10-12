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
  areas,
  gist,
  health,
  inArea,
  pushback,
  rulebook,
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
  it("puts flags and marked decisions that need a decision first", () => {
    const prs = demoDecisions().prs;
    const rows = worthALook(prs);
    expect(
      rows.map((r) => [r.attention, r.pr.number, r.fromWriteUp, r.label]),
    ).toEqual([
      ["act", 318, false, "Disputed decline"],
      ["act", 318, true, "Pushback"],
      ["act", 139, false, "Merged with a finding open"],
      ["look", 77, true, "Trade-offs"],
      ["look", 318, false, "Not checked"],
      ["look", 318, true, "What was checked"],
      ["look", 75, false, "Merged before a review"],
    ]);
    expect(rows[1].reason).toBe(
      "Decide whether an old secret may stay valid when the nightly job is off.",
    );
    // In one area, its pull requests' flags and its own decisions.
    expect(
      worthALook(inArea(prs, "builds table"), "Builds Table").map((r) => [
        r.pr.number,
        r.fromWriteUp,
      ]),
    ).toEqual([[77, true]]);
  });

  it("gathers the rules by area and counts what each area gained", () => {
    const prs = demoDecisions().prs;
    expect(
      rulebook(prs).map((g) => [g.area, g.rules.map((r) => r.rule.statement)]),
    ).toEqual([
      [
        "Webhooks",
        [
          "Receivers accept the previous secret for one day after it changes.",
          "A webhook's signature is checked in a way that doesn't reveal how much of it matched.",
        ],
      ],
      [
        "Settings",
        [
          "A repository's own settings always win over the defaults, including for repositories added later.",
        ],
      ],
    ]);
    expect(rulebook(prs, "settings").map((g) => g.area)).toEqual(["Settings"]);
    expect(
      areas(prs).map((a) => [
        a.name,
        a.prs,
        a.decisions,
        a.rules,
        a.challenges,
        a.pushbacks,
        a.attention,
      ]),
    ).toEqual([
      ["Settings", 1, 5, 1, 1, 1, 0],
      ["Webhooks", 1, 5, 2, 0, 1, 2],
      ["Builds Table", 1, 2, 0, 0, 0, 1],
    ]);
    expect(areas(prs)[1].repos).toEqual(["acme/api"]);
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
    // And what the write-up says.
    expect(searchHistories(prs, "regularly").map((p) => p.number)).toEqual([
      318,
    ]);
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

  it("shows a chapter's write-up, its findings in plain words and where each was said", async () => {
    const { restore } = answering(demoDecisions());
    render(<DecisionsPage />);
    fireEvent.click(
      await screen.findByText("Sign webhook payloads with a rotating secret"),
    );
    expect(await screen.findByText("In plain words")).toBeTruthy();
    expect(
      screen.getByText(
        /^Webhooks are now signed with a secret that changes every night/,
      ),
    ).toBeTruthy();
    expect(screen.getByText("Rules it set")).toBeTruthy();
    const pushback = screen.getByRole("button", {
      name: /The builder relies on the nightly job/,
    });
    expect(pushback.closest("li")?.textContent).toContain(
      "Decide whether an old secret may stay valid when the nightly job is off.",
    );
    const findings = screen.getByRole("table");
    expect(
      within(findings).getByText(
        "An old secret could keep working forever if the nightly job stopped.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("Written by Codex (Codex · demo)")).toBeTruthy();
    const opened = vi.spyOn(window, "open").mockReturnValue(null);
    fireEvent.click(pushback);
    expect(opened).toHaveBeenCalledWith(
      "https://github.com/acme/api/pull/318#issuecomment-2002",
      "_blank",
      "noopener,noreferrer",
    );
    opened.mockRestore();
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

  it("shows the rulebook, and an area's pull requests from the areas", async () => {
    const { restore } = answering(demoDecisions());
    render(<DecisionsPage />);
    await screen.findByText("Show build times per branch");
    fireEvent.click(screen.getByRole("tab", { name: "Rulebook" }));
    const rule = await screen.findByText(
      "Receivers accept the previous secret for one day after it changes.",
    );
    expect(rule.closest("li")?.textContent).toContain(
      "api#318 · Sign webhook payloads with a rotating secret",
    );
    expect(screen.getByText("2 rules")).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "Areas" }));
    const webhooks = await screen.findByRole("button", { name: /^Webhooks/ });
    expect(webhooks.closest("li")?.textContent).toContain(
      "1 pull request · 2 rules · 0 challenges · 1 pushback · 2 worth a look",
    );
    fireEvent.click(webhooks);
    expect(
      await screen.findByRole("tab", { name: "Story", selected: true }),
    ).toBeTruthy();
    await waitFor(() =>
      expect(screen.queryByText("Show build times per branch")).toBeNull(),
    );
    expect(
      screen.getByRole("combobox", { name: "Area" }).textContent,
    ).toContain("Webhooks");
    expect(
      screen.getAllByText("Sign webhook payloads with a rotating secret")
        .length,
    ).toBeGreaterThan(0);
    restore();
  });

  it("says where rules come from before anything is written up", async () => {
    const plain = demoDecisions();
    for (const pr of plain.prs) pr.writeup = null;
    const { restore } = answering(plain);
    const settings = useOrbit.getState().snapshot.settings;
    useOrbit.setState((state) => ({
      snapshot: {
        ...state.snapshot,
        settings: {
          ...state.snapshot.settings,
          writeUps: { enabled: false, writer: "codex" },
        },
      },
    }));
    render(<DecisionsPage />);
    await screen.findByText("Show build times per branch");
    expect(screen.queryByRole("combobox", { name: "Area" })).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Rulebook" }));
    expect(
      await screen.findByText(/^Rules come from write-ups. Turn them on/),
    ).toBeTruthy();
    useOrbit.setState((state) => ({
      snapshot: { ...state.snapshot, settings },
    }));
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
