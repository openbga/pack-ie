import type { UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Link } from "react-aria-components";
import type {
  EmailTarget,
  EngineError,
  EngineSettings,
  NotificationSettings,
  SecretInfo,
  WebhookTarget,
} from "@/api/types";
import { Alert } from "@/components/Alert";
import { EmptyState } from "@/components/EmptyState";
import { Facts } from "@/components/Facts";
import { QueryContent } from "@/components/QueryContent";
import { Section } from "@/components/Section";
import { StatusBadge } from "@/components/StatusBadge";
import { alertSeverityLabel } from "@/lib/status";
import styles from "@/styles/layout.module.css";

/** A target in words: the engine names them `email` and `webhook`. */
export function targetName(target: string): string {
  if (target === "email") return "E-mail";
  return target === "webhook" ? "Webhook" : target;
}

/**
 * Where a webhook is, without its path or query: they can carry a token. Null when the
 * address cannot be read, so that nothing of it is shown.
 */
function webhookOrigin(url: string): string | null {
  try {
    const { protocol, host } = new URL(url);
    return host === "" ? null : `${protocol}//${host}`;
  } catch {
    return null;
  }
}

/** The targets the bootstrap lists, in words: none when it lists none, undefined when it is not known. */
export function listedTargets(
  settings: NotificationSettings | null | undefined,
): string[] | undefined {
  if (settings === undefined) return undefined;
  const { email, webhook } = settings ?? { email: null, webhook: null };
  return [
    ...(email ? [`E-mail relay ${email.host}:${email.port}`] : []),
    ...(webhook
      ? [`Webhook at ${webhookOrigin(webhook.url) ?? "an address the console cannot read"}`]
      : []),
  ];
}

const SECURITY_LABELS: Record<EmailTarget["security"], string> = {
  starttls: "STARTTLS (encrypted after connecting)",
  tls: "TLS (encrypted from the first byte)",
  none: "No encryption (this computer only)",
};

function severitiesText(severities: readonly string[]): string {
  return severities
    .map((each) =>
      each === "critical" || each === "warning" || each === "info"
        ? alertSeverityLabel(each)
        : each,
    )
    .join(", ");
}

/** Whether a secret a target names is in the secret store, and what to do when it is not. */
function SecretValue({
  name,
  secrets,
}: {
  name: string;
  secrets: UseQueryResult<SecretInfo[], EngineError>;
}) {
  const found = secrets.data?.find((secret) => secret.name === name);
  let state: ReactNode = <span className={styles.hint}>Checking the secret store…</span>;
  let problem: string | null = null;
  if (secrets.isError) {
    state = <span className={styles.hint}>Could not check the secret store.</span>;
  } else if (secrets.data !== undefined && found === undefined) {
    state = <StatusBadge tone="warning">Missing</StatusBadge>;
    problem = "Not in the secret store: a send fails until it is stored.";
  } else if (found?.hasValue === false) {
    state = <StatusBadge tone="warning">No value</StatusBadge>;
    problem = "In the secret store without a value: a send fails until one is set.";
  } else if (found !== undefined) {
    state = <StatusBadge tone="neutral">In the secret store</StatusBadge>;
  }
  return (
    <>
      <span className={styles.code}>{name}</span> {state}
      {problem !== null && (
        <p>
          {problem} <Link href="/settings/secrets">Open Settings → Secrets</Link>
        </p>
      )}
    </>
  );
}

type Secrets = UseQueryResult<SecretInfo[], EngineError>;

function EmailFacts({ email, secrets }: { email: EmailTarget; secrets: Secrets }) {
  return (
    <Facts
      items={[
        {
          label: "Relay",
          value: <span className={styles.code}>{`${email.host}:${email.port}`}</span>,
        },
        { label: "Security", value: SECURITY_LABELS[email.security] },
        { label: "Sender", value: email.from },
        { label: "Recipients", value: email.to.join(", ") },
        { label: "Login name", value: email.username ?? "None: the relay is used without a login" },
        {
          label: "Password secret",
          value:
            email.passwordSecret === null ? (
              "None"
            ) : (
              <SecretValue name={email.passwordSecret} secrets={secrets} />
            ),
        },
        { label: "Alert severities sent", value: severitiesText(email.severities) },
      ]}
    />
  );
}

function WebhookFacts({ webhook, secrets }: { webhook: WebhookTarget; secrets: Secrets }) {
  const origin = webhookOrigin(webhook.url);
  return (
    <Facts
      items={[
        {
          label: "Address",
          value: (
            <>
              {origin === null ? (
                "The console cannot read the address"
              ) : (
                <span className={styles.code}>{origin}</span>
              )}
              <p className={styles.hint}>
                Only the host is shown: the rest of the address can carry a token.
              </p>
            </>
          ),
        },
        {
          label: "Bearer secret",
          value:
            webhook.bearerSecret === null ? (
              "None: no Authorization header is sent"
            ) : (
              <SecretValue name={webhook.bearerSecret} secrets={secrets} />
            ),
        },
        { label: "Alert severities sent", value: severitiesText(webhook.severities) },
        { label: "Timeout", value: `${webhook.timeoutSeconds} seconds` },
      ]}
    />
  );
}

function Listed({ settings, secrets }: { settings: NotificationSettings; secrets: Secrets }) {
  return (
    <div className={styles.stack}>
      <Facts
        items={[
          {
            label: "Rate limit",
            value: `One message per alert in ${settings.rateLimitMinutes} minutes, for all targets together`,
          },
        ]}
      />
      <h3 className={styles.subheading}>E-mail</h3>
      {settings.email ? (
        <EmailFacts email={settings.email} secrets={secrets} />
      ) : (
        <p className={styles.hint}>Not configured: no e-mail is sent.</p>
      )}
      <h3 className={styles.subheading}>Webhook</h3>
      {settings.webhook ? (
        <WebhookFacts webhook={settings.webhook} secrets={secrets} />
      ) : (
        <p className={styles.hint}>Not configured: no webhook is called.</p>
      )}
    </div>
  );
}

function Body({
  settings,
  secrets,
}: {
  settings: NotificationSettings | null | undefined;
  secrets: Secrets;
}) {
  if (settings === undefined) {
    return (
      <p className={styles.hint}>
        The console could not read the engine's bootstrap file, so the targets cannot be listed
        here. The engine still knows its own: send a test message to see which ones answer.
      </p>
    );
  }
  if (settings === null || (!settings.email && !settings.webhook)) {
    return (
      <EmptyState
        title="Notifications are off"
        description="No e-mail relay or webhook is configured in the bootstrap file. To send alerts, add notifications.email or notifications.webhook to bootstrap.json on the lab computer and restart the engine."
      />
    );
  }
  return <Listed settings={settings} secrets={secrets} />;
}

/** The bootstrap's notification targets, read-only, with the secrets they name checked against the store. */
export function NotificationTargets({
  settings,
  secrets,
}: {
  settings: UseQueryResult<EngineSettings, EngineError>;
  secrets: Secrets;
}) {
  return (
    <Section title="Targets">
      <div className={styles.stack}>
        <Alert tone="info" compact>
          These settings are read when the engine starts; changing them needs a restart. To change
          them, edit the notifications section of bootstrap.json on the lab computer and restart the
          engine. Passwords and tokens are kept in Settings → Secrets; only their names are shown.
        </Alert>
        <QueryContent query={settings} what="notification settings">
          {(data) => <Body settings={data.notifications} secrets={secrets} />}
        </QueryContent>
      </div>
    </Section>
  );
}
