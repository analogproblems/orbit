import { useState } from "react";
import { Button, Text, VStack } from "@astryxdesign/core";
import {
  browserNotices,
  turnOnBrowserNotices,
  type BrowserNotices,
} from "./notices";

const said: Record<Exclude<BrowserNotices, "off">, string> = {
  on: "This browser shows them too, while Orbit’s page is open in it.",
  blocked:
    "This browser blocks Orbit’s notifications. Allow them in its settings for this site, then reload the page.",
  needsHttps:
    "To show them in this browser too, open Orbit’s https link through Tailscale.",
  needsHomeScreen:
    "To show them on an iPhone or iPad, add this page to the Home Screen, then open Orbit from there.",
  unsupported: "This browser can’t show notifications.",
};

/** Under Settings' notifications switch, in a browser using an Orbit:
 * whether this browser shows them, and the button that asks it to. */
export function BrowserNotifications() {
  const [state, setState] = useState(browserNotices);
  const [asking, setAsking] = useState(false);
  if (state !== "off") {
    return (
      <Text color="secondary" type="supporting">
        {said[state]}
      </Text>
    );
  }
  const ask = async () => {
    setAsking(true);
    try {
      setState(await turnOnBrowserNotices());
    } finally {
      setAsking(false);
    }
  };
  return (
    <VStack gap={1}>
      <Button
        label="Show them in this browser"
        isLoading={asking}
        onClick={() => void ask()}
      />
      <Text color="secondary" type="supporting">
        Your browser asks you first. They show while Orbit’s page is open in it.
      </Text>
    </VStack>
  );
}
