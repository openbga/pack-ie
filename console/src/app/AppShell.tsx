import { CircleHelp } from "lucide-react";
import { lazy, Suspense, useId, useRef, useState } from "react";
import { Outlet } from "react-router";
import { useSnapshotSync } from "@/api/hooks";
import { Button } from "@/components/Button";
import { LoadingState } from "@/components/LoadingState";
import styles from "./AppShell.module.css";
import { NodeStatus } from "./NodeStatus";
import { Sidebar } from "./Sidebar";
import { SignedInAs } from "./SignedInAs";
import { APP_NAME, usePageHeading } from "./usePageHeading";

const PageHelp = lazy(() => import("./help/PageHelp").then((m) => ({ default: m.PageHelp })));

/**
 * The frame of every screen: a skip link, the top bar with the node's state and the help
 * button, the side navigation, the screen and the help drawer. It keeps the snapshot current
 * for every page below it.
 */
export function AppShell() {
  useSnapshotSync();
  const main = useRef<HTMLElement>(null);
  const helpTrigger = useRef<HTMLButtonElement>(null);
  const helpId = useId();
  const [helpOpen, setHelpOpen] = useState(false);
  function closeHelp() {
    setHelpOpen(false);
    helpTrigger.current?.focus();
  }
  usePageHeading(main);
  return (
    <div className={styles.shell} data-help-open={helpOpen}>
      {/* A button, not a `#main` link: the hash router owns the URL fragment. */}
      <button type="button" className={styles.skipLink} onClick={() => main.current?.focus()}>
        Skip to main content
      </button>
      <header className={styles.topBar}>
        <span className={styles.appName}>{APP_NAME}</span>
        <div className={styles.topBarEnd}>
          <NodeStatus />
          <SignedInAs />
          <Button
            ref={helpTrigger}
            variant="quiet"
            aria-expanded={helpOpen}
            aria-controls={helpId}
            onPress={() => (helpOpen ? closeHelp() : setHelpOpen(true))}
          >
            <CircleHelp aria-hidden="true" />
            Help
          </Button>
        </div>
      </header>
      <Sidebar />
      <main id="main" ref={main} tabIndex={-1} className={styles.main}>
        <Outlet />
      </main>
      <div id={helpId} className={styles.help} hidden={!helpOpen}>
        {/* Mounted only while open, so each opening starts on the page's topic with focus inside. */}
        {helpOpen && (
          <Suspense fallback={<LoadingState />}>
            <PageHelp onClose={closeHelp} />
          </Suspense>
        )}
      </div>
    </div>
  );
}
