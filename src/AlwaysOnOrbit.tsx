import { useState } from "react";
import {
  Banner,
  Button,
  Card,
  Heading,
  Text,
  TextInput,
  VStack,
} from "@astryxdesign/core";
import { desktop, remote, useOrbit } from "./store";

/** Settings card for showing an always-on Orbit, one that runs on a
 * computer you keep on, in the desktop app instead of running one here. */
export function AlwaysOnOrbit() {
  const action = useOrbit((state) => state.action);
  const alwaysOn = useOrbit((state) => state.alwaysOn);
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A browser already shows the Orbit it was opened from.
  if (remote) return null;
  const run = async (command: string, args: Record<string, unknown> = {}) => {
    setBusy(true);
    setError(null);
    const result = await action(command, args, { quiet: true });
    // Once it works, the app starts again, so it stays busy until then.
    if (!result.ok) {
      setBusy(false);
      setError(result.error);
    }
  };
  const use = () => {
    if (link.trim()) void run("use_always_on", { link: link.trim() });
  };
  return (
    <Card>
      <VStack gap={4}>
        <Heading level={3}>Use an always-on Orbit</Heading>
        {!desktop && (
          <Text color="secondary">
            Show an Orbit that runs on a computer you keep on, instead of
            running one in the app. Available in the desktop app.
          </Text>
        )}
        {desktop && !alwaysOn && (
          <>
            <Text color="secondary">
              Show an Orbit that runs on a computer you keep on, instead of
              running one in this app. Everything you do here then happens
              there, and its notifications show here. It has its own
              repositories, settings and history. This app’s own stay as they
              are, for when you switch back.
            </Text>
            <TextInput
              label="Its link"
              description="Run orbit-server link on that computer and paste the link it shows."
              placeholder="https://…/#token=…"
              value={link}
              onChange={setLink}
              onEnter={use}
              isDisabled={busy}
              autoComplete="off"
              width="100%"
            />
            <VStack gap={1}>
              <Button
                label="Use it"
                variant="primary"
                isLoading={busy}
                isDisabled={!link.trim()}
                onClick={use}
              />
              <Text color="secondary" type="supporting">
                Orbit checks that it answers, then starts again showing it.
              </Text>
            </VStack>
          </>
        )}
        {desktop && alwaysOn?.chosen && (
          <>
            <Text>
              This app shows the always-on Orbit at {alwaysOn.host}. Everything
              you do here happens there.
            </Text>
            <VStack gap={1}>
              <Button
                label="Stop using it"
                isLoading={busy}
                onClick={() => void run("stop_always_on")}
              />
              <Text color="secondary" type="supporting">
                Orbit starts again with this app’s own repositories, settings
                and history, as you left them.
              </Text>
            </VStack>
          </>
        )}
        {desktop && alwaysOn && !alwaysOn.chosen && (
          <Text color="secondary">
            Orbit without a window is running on this computer with this app’s
            data folder, so the app shows it. Only one Orbit can use a data
            folder at a time. To run the app’s own, stop that one, then open the
            app again.
          </Text>
        )}
        {error && <Banner status="error" title={error} />}
      </VStack>
    </Card>
  );
}
