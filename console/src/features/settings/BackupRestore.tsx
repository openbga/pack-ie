import { Copy } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router";
import { useEngineQuery } from "@/api/hooks";
import { Button } from "@/components/Button";
import { Section } from "@/components/Section";
import { useToast } from "@/components/Toast";
import layout from "@/styles/layout.module.css";
import {
  BOOTSTRAP_PLACEHOLDER,
  type RestorePlatform,
  restoreCommands,
  restorePlatform,
} from "./restoreCommands";
import styles from "./settings.module.css";

/** A command to run on the lab computer, selectable, with a button that copies it. */
function Command({ text, what }: { text: string; what: string }) {
  const toast = useToast();
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      toast.show({ tone: "success", title: "Command copied", description: what });
    } catch {
      toast.show({
        tone: "danger",
        title: "The command could not be copied",
        description: "Select the command and copy it instead.",
      });
    }
  }
  return (
    <div className={styles.command}>
      <code className={styles.commandText}>{text}</code>
      <Button variant="quiet" aria-label={`Copy command: ${what}`} onPress={() => void copy()}>
        <Copy aria-hidden="true" />
        Copy
      </Button>
    </div>
  );
}

function Step({ children }: { children: ReactNode }) {
  return <li className={styles.step}>{children}</li>;
}

/** Where to run the commands, and what they need: the elevation of the engine's platform. */
function terminalNote(platform: RestorePlatform): string {
  if (platform === "windows") {
    return "Open PowerShell as administrator on the lab computer and run these in order.";
  }
  if (platform === "macos" || platform === "linux") {
    return "Open a terminal on the lab computer and run these in order; the ones that need administrator rights start with sudo.";
  }
  return "Open a terminal on the lab computer, as administrator, and run these in order.";
}

/**
 * Restore is never done from the console: the commands for the lab computer, aimed at the
 * bootstrap file of the engine this console is attached to, from stopping the engine to checking
 * it again. Without that file's path the page says how to find it rather than assume one.
 */
export function BackupRestoreSection({ directory }: { directory: string | null }) {
  const settings = useEngineQuery(["getEngineSettings"], (api) => api.getEngineSettings());
  const info = useEngineQuery(["getEngineInfo"], (api) => api.getEngineInfo());
  const platform = restorePlatform(info.data?.platform);
  const bootstrap = settings.data?.bootstrapPath ?? null;
  const hosting = settings.data?.hosting ?? null;
  const commands = restoreCommands({ bootstrap, directory, hosting, platform });
  return (
    <Section title="Restore">
      <p>
        A restore is done on the lab computer with the engine stopped, never from the console.{" "}
        {terminalNote(platform)}
      </p>
      {settings.isPending || info.isPending ? (
        <p role="status" className={layout.hint}>
          Looking up the engine's bootstrap file…
        </p>
      ) : (
        <>
          {bootstrap === null ? (
            <p>
              The console cannot tell which bootstrap file this engine uses, and a command without{" "}
              <code className={layout.code}>--bootstrap</code> uses{" "}
              <code className={layout.code}>config/bootstrap.json</code> in the current folder,
              which may belong to another installation. Find the file first: run the command below
              and read its Bootstrap line (an engine started with{" "}
              <code className={layout.code}>packie run</code> uses the file it was started with),
              then put that path where the commands show{" "}
              <code className={layout.code}>{BOOTSTRAP_PLACEHOLDER}</code>.
            </p>
          ) : (
            <p>
              The commands use this engine's own bootstrap file,{" "}
              <code className={layout.code}>{bootstrap}</code>, so they act on the state this
              console is attached to.
            </p>
          )}
          <ol className={styles.steps}>
            {bootstrap === null && (
              <Step>
                <p>Find the bootstrap file of the installed service.</p>
                <Command text={commands.status} what="find the bootstrap file" />
              </Step>
            )}
            <Step>
              <p>Check the backup. This changes nothing.</p>
              <Command text={commands.verify} what="check the backup" />
            </Step>
            <Step>
              {commands.stop === null ? (
                <p>
                  Stop the engine: press Ctrl+C in the terminal where{" "}
                  <code className={layout.code}>packie run</code> is running. A restore refuses to
                  run while it is running.
                </p>
              ) : (
                <>
                  <p>Stop the engine; a restore refuses to run while it is running.</p>
                  <Command text={commands.stop} what="stop the engine" />
                </>
              )}
            </Step>
            <Step>
              <p>
                Restore the message store, audit log and configuration after verifying the backup.
                The replaced state moves to pre-restore-&lt;time&gt; in the state directory.
              </p>
              <Command text={commands.restore} what="restore the backup" />
            </Step>
            <Step>
              <p>
                {commands.stop === null
                  ? "Start the engine again in that terminal."
                  : "Start the engine again."}
              </p>
              <Command text={commands.start} what="start the engine" />
            </Step>
            <Step>
              <p>Check that the engine is running and healthy:</p>
              <ul className={layout.list}>
                <li>
                  <Link to="/dashboard">Dashboard</Link> shows the channels running.
                </li>
                <li>
                  <Link to="/alerts">Alerts</Link> has no{" "}
                  <code className={layout.code}>config_rejected</code> alert.
                </li>
                <li>
                  <Link to="/settings/versions">Settings → Versions</Link> shows the restored
                  configuration version.
                </li>
                <li>
                  <Link to="/audit">Audit</Link> shows a{" "}
                  <code className={layout.code}>backup_restore</code> event.
                </li>
              </ul>
            </Step>
          </ol>
          {platform === "macos" && (
            <p className={layout.hint}>
              The commands are those of the system service. If the service was installed for one
              login only (<code className={layout.code}>packie service install --user</code>), leave
              out sudo and add <code className={layout.code}>--user</code> to the service commands.
            </p>
          )}
          <p className={layout.hint}>
            Secret values are not in a backup. On a replacement computer, enter the missing secrets
            in <Link to="/settings/secrets">Settings → Secrets</Link> (or with{" "}
            <code className={layout.code}>packie secrets set &lt;name&gt;</code>) before the
            channels that need them can connect.
          </p>
        </>
      )}
    </Section>
  );
}
