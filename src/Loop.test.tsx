// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { LoopPanel, ReadinessPanel } from "./Loop";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
afterEach(cleanup);
describe("review loop panel", () => {
  it("explains why the loop needs a person and offers to continue it", () => {
    const snapshot = freshDemo();
    const action = vi.fn(async () => ({ ok: true, data: undefined }) as never);
    useOrbit.setState({ snapshot, action });
    const prId = snapshot.prs[1].id;
    render(<LoopPanel prId={prId} />);
    expect(screen.getByText("Loop: Needs you")).toBeTruthy();
    expect(screen.getByText("Round 4 of 4")).toBeTruthy();
    expect(screen.getByText("Round limit reached.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue loop" }));
    expect(action).toHaveBeenCalledWith("continue_loop", { prId, rounds: 2 });
  });
  it("offers Stop while the loop runs and shows nothing without a loop", () => {
    const snapshot = freshDemo();
    snapshot.loopStates![0] = {
      ...snapshot.loopStates![0],
      status: "waiting_for_author",
      reason: null,
      rounds: 1,
    };
    const action = vi.fn(async () => ({ ok: true, data: undefined }) as never);
    useOrbit.setState({ snapshot, action });
    const { container } = render(<LoopPanel prId={snapshot.prs[0].id} />);
    expect(container.textContent).toBe("");
    render(<LoopPanel prId={snapshot.prs[1].id} />);
    fireEvent.click(screen.getByRole("button", { name: "Stop loop" }));
    expect(action).toHaveBeenCalledWith("stop_loop", {
      prId: snapshot.prs[1].id,
    });
  });
  it("says when GitHub refused the status check", () => {
    const snapshot = freshDemo();
    snapshot.settings.autoPost = true;
    const prId = snapshot.prs[1].id;
    snapshot.commitChecks = [
      {
        prId,
        repo: snapshot.prs[1].repo,
        sha: snapshot.prs[1].headSha,
        refused: {
          state: "failure",
          description: "Round 4 · 1 blocking finding",
          error: "Resource not accessible by integration",
        },
      },
    ];
    useOrbit.setState({ snapshot });
    render(<LoopPanel prId={prId} />);
    expect(
      screen.getByText(
        "GitHub did not accept the status check: Resource not accessible by integration",
      ),
    ).toBeTruthy();
  });
});
describe("merging panel", () => {
  it("says what is in the way, first things first", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    render(<ReadinessPanel prId={snapshot.prs[1].id} />);
    expect(screen.getByText("Not clear yet")).toBeTruthy();
    // Orbit hasn't told its agent anything yet.
    expect(
      screen.queryByRole("button", { name: "Open Orbit's comment" }),
    ).toBeNull();
  });
  it("opens Orbit's comment only once it gives the answer on GitHub", () => {
    const snapshot = freshDemo();
    const told = snapshot.ops!.find((t) => t.ready)!;
    const prId = snapshot.prs[1].id;
    // A comment only about CI, and one not on GitHub yet.
    snapshot.ops = [
      { ...told, prId, ready: undefined },
      { ...told, prId, url: "" },
    ];
    useOrbit.setState({ snapshot });
    render(<ReadinessPanel prId={prId} />);
    expect(screen.getByText("Not clear yet")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Open Orbit's comment" }),
    ).toBeNull();
    expect(
      screen.getByText("Orbit's review loop needs you: Round limit reached."),
    ).toBeTruthy();
    expect(
      screen.getByText("Orbit hasn't read the checks on abc1111 yet."),
    ).toBeTruthy();
  });
  it("says where it is clear to merge, and nothing where Orbit has no answer", () => {
    const snapshot = freshDemo();
    snapshot.readiness![1] = {
      ...snapshot.readiness![1],
      baseSha: "7d41e09" + "0".repeat(33),
      clear: true,
      waits: [],
    };
    useOrbit.setState({ snapshot });
    const { container } = render(<ReadinessPanel prId={snapshot.prs[0].id} />);
    expect(container.textContent).toBe("");
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    expect(screen.getByText("Clear to merge")).toBeTruthy();
    expect(
      screen.getByText("Clear to merge at abc4444 against main at 7d41e09."),
    ).toBeTruthy();
  });
  it("opens the comment where Orbit tells the agent", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    render(<ReadinessPanel prId={snapshot.prs[4].id} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Open Orbit's comment" }),
    );
    expect(open).toHaveBeenCalledWith(
      `${snapshot.prs[4].url}#issuecomment-902`,
      "_blank",
      "noopener,noreferrer",
    );
    open.mockRestore();
  });
});
