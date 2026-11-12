import type { ConfigDocument } from "@/api/types";
import { Facts } from "@/components/Facts";
import styles from "@/styles/layout.module.css";
import config from "./config.module.css";
import { channelSettings, endpointSettings, type Setting } from "./settingsText";

/** Collections up to this size start open; longer ones start closed under their heading. */
const OPEN_UP_TO = 12;

function Items({
  title,
  items,
}: {
  title: string;
  items: { id: string; label: string; settings: Setting[] }[];
}) {
  return (
    <details className={config.disclosure} open={items.length <= OPEN_UP_TO}>
      <summary>
        {title} ({items.length})
      </summary>
      {items.length === 0 ? (
        <p className={styles.hint}>None in this version.</p>
      ) : (
        <ul className={config.items} aria-label={title}>
          {items.map((item) => (
            <li key={item.id}>
              <h4 className={config.itemName}>
                {item.label} <span className={styles.code}>{item.id}</span>
              </h4>
              <Facts layout="compact" items={item.settings} />
            </li>
          ))}
        </ul>
      )}
    </details>
  );
}

/** The endpoints and channels of a configuration, read-only, with key settings in words. */
export function VersionDocument({ document }: { document: ConfigDocument }) {
  return (
    <div className={styles.stack}>
      <Items
        title="Endpoints"
        items={document.endpoints.map((endpoint) => ({
          id: endpoint.id,
          label: endpoint.label,
          settings: endpointSettings(endpoint),
        }))}
      />
      <Items
        title="Channels"
        items={document.channels.map((channel) => ({
          id: channel.id,
          label: channel.label,
          settings: channelSettings(channel, document),
        }))}
      />
    </div>
  );
}
