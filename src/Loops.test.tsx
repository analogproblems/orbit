// @vitest-environment jsdom
import "./test-setup";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { Loops } from "./Loops";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
import { autopilotSummary, type Snapshot } from "./types";
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
  function Page() {
    const [selected, onSelect] = useState<string | null>(null);
    return <Loops {...go} selected={selected} onSelect={onSelect} />;
  }
  render(<Page />);
  return go;
}
const list = (name: string) => screen.getByRole("list", { name });
const row = (title: string, name = "Loops") =>
  within(list(name)).getByText(title).closest("li") as HTMLElement;
const finishedRow = (title: string) => row(title, "Finished loops");

describe("needs you list", () => {
  it("lists each open item with the one action that resolves it", () => {
    const snapshot = freshDemo();
    const action = vi.fn().mockResolvedValue({ ok: true });
    useOrbit.setState({ action });
    const go = show(snapshot);
    expect(screen.getByRole("heading", { name: "Needs you" })).toBeTruthy();
    const needs = list("Needs you");
    expect(within(needs).getByText("Round limit reached.")).toBeTruthy();
    fireEvent.click(
      within(needs).getByRole("button", { name: "Continue loop" }),
    );
    expect(action).toHaveBeenCalledWith("continue_loop", {
      prId: snapshot.prs[1].id,
      rounds: 2,
    });
    fireEvent.click(
      within(needs).getByRole("button", { name: "Open repositories" }),
    );
    expect(go.onRepositories).toHaveBeenCalled();
  });
  it("offers Approve or deny, Publish and Open chat by kind", () => {
    const snapshot = freshDemo();
    const [clean] = snapshot.reviews.filter((r) => r.id.startsWith("demo-145"));
    snapshot.reviews.push({
      ...clean,
      id: "held",
      status: "ready",
      error: "Not published automatically.",
    });
    snapshot.approvals = [
      {
        id: 7,
        reviewId: clean.id,
        method: "item/fileChange/requestApproval",
        params: {},
      },
    ];
    snapshot.attention = [
      {
        key: "approval",
        kind: "approval",
        prId: clean.prId,
        reviewId: clean.id,
        approvalId: 7,
        repo: null,
        reason: "The reviewer asks to change files.",
      },
      {
        key: "held",
        kind: "held",
        prId: clean.prId,
        reviewId: "held",
        approvalId: null,
        repo: null,
        reason: "Not published automatically.",
      },
      {
        key: "failed",
        kind: "failed",
        prId: clean.prId,
        reviewId: clean.id,
        approvalId: null,
        repo: null,
        reason: "Codex exited.",
      },
    ];
    const go = show(snapshot);
    fireEvent.click(screen.getByRole("button", { name: "Approve or deny" }));
    expect(go.onApproval).toHaveBeenCalledWith(clean.id, 7);
    fireEvent.click(screen.getByRole("button", { name: "Publish" }));
    expect(go.onPublish).toHaveBeenCalledWith(
      expect.objectContaining({ id: "held" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Open chat" }));
    expect(go.onChoose).toHaveBeenLastCalledWith(snapshot.prs[4], clean);
  });
  it("opens a stuck pull request on GitHub, and in Orbit from its row", () => {
    const snapshot = freshDemo();
    const pr = snapshot.prs[4];
    snapshot.attention = [
      {
        key: `stuck:${pr.id}:2026-10-30T12:00:00.000Z`,
        kind: "stuck",
        prId: pr.id,
        reviewId: null,
        approvalId: null,
        repo: null,
        reason:
          "It has waited on a person for more than a day. bob hasn't reviewed abc4444 yet.",
      },
    ];
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    const go = show(snapshot);
    const needs = list("Needs you");
    expect(within(needs).getByText(`Stuck · ${pr.id}`)).toBeTruthy();
    fireEvent.click(
      within(needs).getByRole("button", { name: "Open on GitHub" }),
    );
    expect(open).toHaveBeenCalledWith(pr.url, "_blank", "noopener,noreferrer");
    fireEvent.click(within(needs).getByText(pr.title));
    expect(go.onChoose).toHaveBeenCalledWith(pr, undefined);
    open.mockRestore();
  });
  it("shows no list when nothing needs you", () => {
    show({ ...freshDemo(), attention: [] });
    expect(screen.queryByRole("heading", { name: "Needs you" })).toBeNull();
  });
});

describe("loops", () => {
  it("groups rounds by pull request with running and finished loops apart", () => {
    const snapshot = freshDemo();
    show(snapshot);
    const [, limited, waiting, , clean] = snapshot.prs;
    expect(
      within(row(limited.title)).getByText(/Needs you · Round 4 of 4/),
    ).toBeTruthy();
    expect(
      within(row(limited.title)).getByText("Blocking findings 3 → 2 → 2 → 1"),
    ).toBeTruthy();
    expect(
      within(row(waiting.title)).getByText(/Waiting for author · Round 2 of 4/),
    ).toBeTruthy();
    // Finished loops are folded away until opened.
    const fold = screen.getByRole("button", { name: "Finished (1)" });
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(fold);
    expect(fold.getAttribute("aria-expanded")).toBe("true");
    expect(
      within(finishedRow(clean.title)).getByText("Blocking findings 4 → 1 → 0"),
    ).toBeTruthy();
    // Manual reviews and PRs without automatic rounds are not loops.
    for (const name of ["Loops", "Finished loops"])
      expect(within(list(name)).queryByText(snapshot.prs[0].title)).toBeNull();
  });
  it("counts a PR that left the inbox as finished", () => {
    const snapshot = freshDemo();
    snapshot.loopStates = snapshot.loopStates!.map((l) =>
      l.prId === snapshot.prs[2].id ? { ...l, closed: true } : l,
    );
    snapshot.historyPrs = [snapshot.prs[2]];
    snapshot.prs = snapshot.prs.filter((p) => p !== snapshot.prs[2]);
    show(snapshot);
    fireEvent.click(screen.getByRole("button", { name: /Finished \(2\)/ }));
    expect(
      within(finishedRow(snapshot.historyPrs[0].title)).getByText(
        /Left the inbox/,
      ),
    ).toBeTruthy();
  });
  it("moves a loop into Needs you and out again when resolved", () => {
    const snapshot = freshDemo();
    const waiting = snapshot.prs[2];
    snapshot.attention = [];
    show(snapshot);
    expect(screen.queryByRole("heading", { name: "Needs you" })).toBeNull();
    const item = {
      key: `loop:${waiting.id}:The loop is not converging.`,
      kind: "loop" as const,
      prId: waiting.id,
      reviewId: null,
      approvalId: null,
      repo: null,
      reason: "The loop is not converging.",
    };
    const needs = (attention: (typeof item)[]) =>
      act(() =>
        useOrbit.setState({
          snapshot: {
            ...snapshot,
            attention,
            loopStates: snapshot.loopStates!.map((l) =>
              l.prId === waiting.id
                ? {
                    ...l,
                    status: attention.length
                      ? "needs_you"
                      : "waiting_for_author",
                    reason: attention.length ? item.reason : null,
                  }
                : l,
            ),
          },
        }),
      );
    needs([item]);
    expect(
      within(list("Needs you")).getByText("The loop is not converging."),
    ).toBeTruthy();
    needs([]);
    expect(screen.queryByRole("heading", { name: "Needs you" })).toBeNull();
    expect(
      within(row(waiting.title)).getByText(/Waiting for author/),
    ).toBeTruthy();
  });
  it("shows a loop's rounds in order and opens a round's chat", () => {
    const snapshot = freshDemo();
    const go = show(snapshot);
    const pr = snapshot.prs[4];
    fireEvent.click(screen.getByRole("button", { name: /Finished/ }));
    fireEvent.click(within(list("Finished loops")).getByText(pr.title));
    const rounds = list("Rounds");
    const labels = within(rounds)
      .getAllByText(/^Round \d/)
      .map((e) => e.textContent);
    expect(labels).toEqual([
      "Round 1 · 4 blocking findings",
      "Round 2 · 1 blocking finding",
      "Round 3 · No blocking findings",
    ]);
    expect(within(rounds).getByText("Findings P1 4 · P2 1")).toBeTruthy();
    expect(
      within(rounds).getByText(
        "Since the last round: 1 still open · 3 fixed · 1 declined (accepted)",
      ),
    ).toBeTruthy();
    expect(
      within(rounds).getAllByRole("button", { name: /View comment on GitHub/ }),
    ).toHaveLength(3);
    fireEvent.click(
      within(rounds).getByRole("button", { name: "Open round 2 chat" }),
    );
    expect(go.onChoose).toHaveBeenCalledWith(
      pr,
      expect.objectContaining({ id: "demo-145-round-2" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "All loops" }));
    expect(screen.getByRole("heading", { name: "Review loops" })).toBeTruthy();
  });
  it("labels every loop state", () => {
    const snapshot = freshDemo();
    const statuses = [
      "reviewing",
      "waiting_for_author",
      "needs_you",
      "stopped",
    ] as const;
    snapshot.loopStates = statuses.map((status, i) => ({
      ...snapshot.loopStates![i % 3],
      prId: snapshot.prs[i].id,
      status,
    }));
    show(snapshot);
    for (const [i, label] of [
      "Reviewing",
      "Waiting for author",
      "Needs you",
      "Stopped",
    ].entries())
      expect(
        within(row(snapshot.prs[i].title)).getByText(
          new RegExp(`· ${label} ·`),
        ),
      ).toBeTruthy();
  });
  it("summarises autopilot for the header", () => {
    const snapshot = freshDemo();
    expect(autopilotSummary(snapshot)).toBe(
      "Autopilot · 2 loops running · 3 need you",
    );
    snapshot.attention = [];
    snapshot.loopStates = snapshot.loopStates!.slice(2);
    expect(autopilotSummary(snapshot)).toBe("Autopilot · 0 loops running");
  });
});
