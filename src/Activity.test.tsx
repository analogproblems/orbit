// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Activity } from "./Activity";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
afterEach(cleanup);
describe("needs you list", () => {
  it("lists each open item and opens where it is resolved", () => {
    const snapshot = freshDemo();
    useOrbit.setState({ snapshot });
    const onChoose = vi.fn();
    const onRepositories = vi.fn();
    render(
      <Activity
        onChoose={onChoose}
        onRepositories={onRepositories}
        onSettings={vi.fn()}
      />,
    );
    expect(screen.getByRole("heading", { name: "Needs you" })).toBeTruthy();
    expect(screen.getByText("Round limit reached.")).toBeTruthy();
    fireEvent.click(screen.getByText(snapshot.prs[1].title));
    expect(onChoose).toHaveBeenCalledWith(snapshot.prs[1], undefined);
    fireEvent.click(screen.getByText("acme/dashboard"));
    expect(onRepositories).toHaveBeenCalled();
  });
  it("shows no list when nothing needs you", () => {
    useOrbit.setState({ snapshot: { ...freshDemo(), attention: [] } });
    render(
      <Activity
        onChoose={vi.fn()}
        onRepositories={vi.fn()}
        onSettings={vi.fn()}
      />,
    );
    expect(screen.queryByRole("heading", { name: "Needs you" })).toBeNull();
  });
});
