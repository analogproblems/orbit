// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Theme } from "@astryxdesign/core";
import { neutralTheme } from "@astryxdesign/theme-neutral/built";
import { TimeZoneSelector } from "./TimeZone";
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

/** This device keeps `zone`. */
function onDevice(zone: string) {
  const real = Intl.DateTimeFormat.prototype.resolvedOptions;
  vi.spyOn(Intl.DateTimeFormat.prototype, "resolvedOptions").mockImplementation(
    function (this: Intl.DateTimeFormat) {
      return { ...real.call(this), timeZone: zone };
    },
  );
}
function mount(props: Partial<Parameters<typeof TimeZoneSelector>[0]> = {}) {
  const onChange = vi.fn();
  render(
    <Theme theme={neutralTheme} mode="dark">
      <TimeZoneSelector
        value=""
        clockZone="Europe/London"
        elsewhere={false}
        onChange={onChange}
        {...props}
      />
    </Theme>,
  );
  return { onChange };
}
const box = () => screen.getByRole("combobox", { name: /^Time zone/ });
const options = async () =>
  (await screen.findAllByRole("option")).map((o) => o.textContent);

describe("the brief's time zone", () => {
  it("keeps this computer's clock until you choose a zone", async () => {
    onDevice("Europe/London");
    const { onChange } = mount();
    expect(screen.getByText("This computer (Europe/London)")).toBeTruthy();
    expect(
      screen.getByText("The brief comes at its time in this time zone."),
    ).toBeTruthy();
    // Only one suggestion: this device is the computer Orbit runs on.
    await userEvent.click(box());
    expect(await options()).toEqual(["This computer (Europe/London)"]);
    await userEvent.clear(box());
    await userEvent.type(box(), "new york");
    expect(await options()).toEqual(["America/New York"]);
    await userEvent.click(screen.getByRole("option"));
    expect(onChange).toHaveBeenCalledWith("America/New_York");
  });

  it("offers your device's zone when Orbit runs on a computer set to another", async () => {
    onDevice("America/New_York");
    const { onChange } = mount({ clockZone: "Etc/UTC", elsewhere: true });
    expect(screen.getByText("Orbit’s computer (Etc/UTC)")).toBeTruthy();
    expect(
      screen.getByText(
        "Orbit’s computer keeps Etc/UTC time, and this device keeps America/New York. Choose yours, so the brief comes at your time.",
      ),
    ).toBeTruthy();
    await userEvent.click(box());
    expect(await options()).toEqual([
      "Orbit’s computer (Etc/UTC)",
      "This device (America/New York)",
    ]);
    await userEvent.click(
      screen.getByRole("option", { name: "This device (America/New York)" }),
    );
    expect(onChange).toHaveBeenCalledWith("America/New_York");
  });

  it("goes back to the computer's clock, and says nothing once you chose", async () => {
    onDevice("America/New_York");
    const { onChange } = mount({
      value: "Asia/Tokyo",
      clockZone: "Etc/UTC",
      elsewhere: true,
    });
    expect(screen.getByText("Asia/Tokyo")).toBeTruthy();
    expect(
      screen.getByText("The brief comes at its time in this time zone."),
    ).toBeTruthy();
    await userEvent.click(box());
    expect(await options()).toEqual([
      "Orbit’s computer (Etc/UTC)",
      "This device (America/New York)",
      "Asia/Tokyo",
    ]);
    await userEvent.click(
      screen.getByRole("option", { name: "Orbit’s computer (Etc/UTC)" }),
    );
    expect(onChange).toHaveBeenCalledWith("");
  });
});
