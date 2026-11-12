import { useConnection } from "@/api/hooks";
import styles from "./AppShell.module.css";

/**
 * The OS account the engine attributes this console's actions to (from its greeting): who the
 * audit log and the change history will name. Nothing while it is not known.
 */
export function SignedInAs() {
  const connection = useConnection();
  const actor = connection.data?.actor ?? null;
  if (actor === null) return null;
  return (
    <span className={styles.signedIn} title="The account the audit log records your actions under">
      Signed in as {actor}
    </span>
  );
}
