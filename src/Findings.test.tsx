// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { Findings } from "./Findings";
import { demoFindings, freshDemo } from "./demo";
import type { Review } from "./types";
afterEach(cleanup);
const review = (changes: Partial<Review>): Review => ({
  ...freshDemo().reviews[0],
  ...changes,
});
describe("structured findings", () => {
  it("lists each finding with its severity, location, and status", () => {
    const findings = demoFindings("r");
    findings[1] = { ...findings[1], path: null, endLine: 80 };
    render(<Findings review={review({ findings })} />);
    expect(
      screen.getByText("Verdict: changes requested · 2 findings"),
    ).toBeTruthy();
    expect(screen.getAllByText("P2")).toHaveLength(2);
    expect(screen.getByText("src/config/repositories.ts:48")).toBeTruthy();
    expect(screen.getByText("Outside the repository")).toBeTruthy();
    expect(screen.getAllByText("Open")).toHaveLength(2);
    expect(screen.getByText(/End-to-end tests/)).toBeTruthy();
  });
  it("labels unstructured runs and hides findings until a run finishes", () => {
    const { rerender } = render(
      <Findings review={review({ findings: null })} />,
    );
    expect(screen.getByText("Unstructured run")).toBeTruthy();
    rerender(
      <Findings review={review({ status: "reviewing", extracting: true })} />,
    );
    expect(screen.queryByText("Unstructured run")).toBeNull();
    expect(screen.queryByText(/Verdict/)).toBeNull();
  });
});
