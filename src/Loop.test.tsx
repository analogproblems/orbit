// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { LoopPanel } from "./Loop";
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
    snapshot.reviews = [
      {
        ...snapshot.reviews[0],
        prId,
        automatic: true,
        commitStatus: {
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
