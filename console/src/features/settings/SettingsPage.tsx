import { Link } from "react-aria-components";
import { useParams } from "react-router";
import { EmptyState } from "@/components/EmptyState";
import { Page, PageHeader } from "@/components/Page";
import { Tabs } from "@/components/Tabs";
import { VersionHistory } from "@/features/config/VersionHistory";
import { AboutTab } from "./AboutTab";
import { BackupTab } from "./BackupTab";
import { EngineTab } from "./EngineTab";
import { HelpTab } from "./HelpTab";
import { NotificationsTab } from "./NotificationsTab";
import { PrivacyTab } from "./PrivacyTab";
import { RetentionTab } from "./RetentionTab";
import { SecretsTab } from "./SecretsTab";

const TABS = [
  { id: "engine", label: "Engine" },
  { id: "retention", label: "Retention" },
  { id: "secrets", label: "Secrets" },
  { id: "versions", label: "Versions" },
  { id: "backup", label: "Backup" },
  { id: "notifications", label: "Notifications" },
  { id: "privacy", label: "Privacy" },
  { id: "about", label: "About" },
  { id: "help", label: "Help" },
] as const;

type TabId = (typeof TABS)[number]["id"];

/** The tabs' ids, which are the `:tab` of `/settings/:tab`. */
export const SETTINGS_TAB_IDS: readonly string[] = TABS.map((tab) => tab.id);

function TabContent({ tab }: { tab: TabId }) {
  switch (tab) {
    case "engine":
      return <EngineTab />;
    case "retention":
      return <RetentionTab />;
    case "secrets":
      return <SecretsTab />;
    case "versions":
      return <VersionHistory />;
    case "backup":
      return <BackupTab />;
    case "notifications":
      return <NotificationsTab />;
    case "privacy":
      return <PrivacyTab />;
    case "about":
      return <AboutTab />;
    case "help":
      return <HelpTab />;
  }
}

/**
 * Settings: peer views of the engine's configuration, one URL per tab (`/settings/:tab`).
 */
export function SettingsPage() {
  const { tab } = useParams();
  const selected = TABS.find((each) => each.id === tab);
  return (
    <Page>
      <PageHeader title="Settings" />
      {selected === undefined ? (
        <EmptyState
          title="This settings page does not exist"
          description="Choose one of the settings pages instead."
          action={<Link href="/settings/engine">Go to Engine settings</Link>}
        />
      ) : (
        <Tabs
          label="Settings"
          selectedId={selected.id}
          tabs={TABS.map((each) => ({ ...each, href: `/settings/${each.id}` }))}
        >
          <TabContent tab={selected.id} />
        </Tabs>
      )}
    </Page>
  );
}
