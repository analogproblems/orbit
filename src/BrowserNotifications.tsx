import { useState } from "react";
import { Button, Text, VStack } from "@astryxdesign/core";
import {
  browserNotices,
  turnOnBrowserNotices,
  type BrowserNotices,
} from "./notices";
import { accessLink } from "./store";

const said: Record<
  Exclude<BrowserNotices, "off" | "needsHomeScreen">,
  string
> = {
  on: "This browser shows them too, while Orbit’s page is open in it.",
  blocked:
    "This browser blocks Orbit’s notifications. Allow them in its settings for this site, then reload the page.",
  needsHttps:
    "To show them in this browser too, open Orbit’s https link through Tailscale.",
  unsupported: "This browser can’t show notifications.",
};

/** Under Settings' notifications switch, in a browser using an Orbit:
 * whether this browser shows them, and the button that asks it to. */
export function BrowserNotifications() {
  const [state, setState] = useState(browserNotices);
  const [asking, setAsking] = useState(false);
  if (state === "needsHomeScreen") return <HomeScreen />;
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

/** On an iPhone or iPad, in Safari: add Orbit to the Home Screen. Orbit
 * there starts without what Safari kept, so it asks once for the access
 * link (`PasteLink`), which this copies. */
function HomeScreen() {
  const [copied, setCopied] = useState<string | null>(null);
  const link = accessLink();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(link ?? "");
      setCopied("Copied. Paste it when Orbit on the Home Screen asks for it.");
    } catch {
      setCopied(
        "This browser wouldn’t copy it. Paste the link you opened Orbit with instead.",
      );
    }
  };
  return (
    <VStack gap={1}>
      <Text color="secondary" type="supporting">
        To show them on an iPhone or iPad, add this page to the Home Screen
        (Share, then Add to Home Screen), then open Orbit from there. It asks
        once for Orbit’s access link{link ? ", which you can copy here" : ""}.
      </Text>
      {link && (
        <Button label="Copy the access link" onClick={() => void copy()} />
      )}
      {copied && (
        <Text color="secondary" type="supporting">
          {copied}
        </Text>
      )}
    </VStack>
  );
}
