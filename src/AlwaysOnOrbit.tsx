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
  // The command running: once it works, the app starts again, so it stays
  // busy until then.
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // A browser already shows the Orbit it was opened from.
  if (remote) return null;
  const run = async (command: string, args: Record<string, unknown> = {}) => {
    setBusy(command);
    setError(null);
    const result = await action(command, args, { quiet: true });
    if (!result.ok) {
      setBusy(null);
      setError(result.error);
    }
  };
  const use = () => {
    if (link.trim()) void run("use_always_on", { link: link.trim() });
  };
  const move = () => {
    if (link.trim()) void run("move_to_always_on", { link: link.trim() });
  };
  const linkField = (label: string, description: string, button: string) => (
    <>
      <TextInput
        label={label}
        description={description}
        placeholder="https://…/#token=…"
        value={link}
        onChange={setLink}
        onEnter={use}
        isDisabled={busy !== null}
        autoComplete="off"
        width="100%"
      />
      <VStack gap={1}>
        <Button
          label={button}
          variant="primary"
          isLoading={busy === "use_always_on"}
          isDisabled={!link.trim() || busy !== null}
          onClick={use}
        />
        <Text color="secondary" type="supporting">
          Orbit checks that it answers, then starts again showing it.
        </Text>
      </VStack>
    </>
  );
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
            {linkField(
              "Its link",
              "Run orbit-server link on that computer and paste the link it shows.",
              "Use it",
            )}
            <VStack gap={1}>
              <Button
                label="Move to it"
                isLoading={busy === "move_to_always_on"}
                isDisabled={!link.trim() || busy !== null}
                onClick={move}
              />
              <Text color="secondary" type="supporting">
                Or take this app’s repositories, settings, reviews, history and
                Bit’s notes there, if it has none of its own yet. It can take a
                few minutes while it downloads your repositories. This app keeps
                its own copy, with autopilot off.
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
            {alwaysOn.pending && (
              <Banner
                status="warning"
                title="Orbit can’t tell yet whether it moved"
                description="The always-on Orbit didn’t answer after the move. Until it does, this app runs no Orbit of its own, so nothing is done twice, and it asks again each time it starts."
              />
            )}
            {linkField(
              "Its new link",
              "If its link stops working, such as after orbit-server link --new, paste the new one here.",
              "Use the new link",
            )}
            <VStack gap={1}>
              <Button
                label="Stop using it"
                isLoading={busy === "stop_always_on"}
                isDisabled={busy !== null}
                onClick={() => void run("stop_always_on")}
              />
              <Text color="secondary" type="supporting">
                {alwaysOn.pending
                  ? "Only if you know it didn’t move: Orbit starts again with this app’s own repositories, settings and history, with autopilot off."
                  : "Orbit starts again with this app’s own repositories, settings and history, as you left them."}
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
