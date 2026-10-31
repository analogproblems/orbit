// @vitest-environment jsdom
import "./test-setup";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: invokeMock,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { AlwaysOnOrbit } from "./AlwaysOnOrbit";
import { RemoteAccess } from "./RemoteAccess";
import { useOrbit } from "./store";

afterEach(() => {
  cleanup();
  invokeMock.mockReset();
});
const link = `https://box.example.ts.net/#token=${"ab".repeat(32)}`;

it("uses the always-on Orbit at the link you paste", async () => {
  useOrbit.setState({ alwaysOn: null });
  invokeMock.mockRejectedValueOnce(
    "Orbit can't reach the always-on Orbit at box.example.ts.net.",
  );
  render(<AlwaysOnOrbit />);
  const use = screen.getByRole("button", { name: "Use it" });
  expect((use as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(screen.getByLabelText(/Its link/), {
    target: { value: ` ${link} ` },
  });
  fireEvent.click(use);
  expect(invokeMock).toHaveBeenCalledWith("use_always_on", { link });
  expect(
    await screen.findByText(
      "Orbit can't reach the always-on Orbit at box.example.ts.net.",
    ),
  ).toBeTruthy();
});

it("shows the always-on Orbit in use, and stops using it", () => {
  useOrbit.setState({
    alwaysOn: { host: "box.example.ts.net", chosen: true },
  });
  invokeMock.mockResolvedValue(null);
  render(<AlwaysOnOrbit />);
  expect(
    screen.getByText(
      "This app shows the always-on Orbit at box.example.ts.net. Everything you do here happens there.",
    ),
  ).toBeTruthy();
  expect(screen.queryByLabelText(/Its link/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Stop using it" }));
  expect(invokeMock).toHaveBeenCalledWith("stop_always_on", {});
});

it("explains Orbit without a window on this computer", () => {
  useOrbit.setState({
    alwaysOn: { host: "127.0.0.1:4317", chosen: false },
  });
  render(<AlwaysOnOrbit />);
  expect(screen.getByText(/Orbit without a window is running/)).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
});

it("leaves browser access to the always-on Orbit", () => {
  useOrbit.setState({
    alwaysOn: { host: "box.example.ts.net", chosen: true },
  });
  render(<RemoteAccess />);
  expect(screen.getByText(/run orbit-server link there/)).toBeTruthy();
  expect(invokeMock).not.toHaveBeenCalled();
  expect(screen.queryByRole("switch")).toBeNull();
});
