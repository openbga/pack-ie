import { useEngineQuery } from "@/api/hooks";
import styles from "@/styles/layout.module.css";
import { BackupNowSection } from "./BackupNow";
import { BackupRestoreSection } from "./BackupRestore";
import { BACKUP_REFRESH_MS, BackupStatusSection } from "./BackupStatus";

/** Settings → Backup (docs/spec/backup.md): status, a backup on request, and how to restore. */
export function BackupTab() {
  const status = useEngineQuery(["getBackupStatus"], (api) => api.getBackupStatus(), {
    refetchInterval: BACKUP_REFRESH_MS,
  });
  return (
    <div className={styles.stack}>
      <BackupStatusSection query={status} />
      <BackupNowSection />
      <BackupRestoreSection directory={status.data?.lastSuccess?.directory ?? null} />
    </div>
  );
}
