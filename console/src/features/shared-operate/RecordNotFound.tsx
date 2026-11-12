import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { Page, PageHeader } from "@/components/Page";

/** A record page whose id is not in the configuration (removed, or a stale link). */
export function RecordNotFound({
  what,
  listLabel,
  listHref,
}: {
  /** Capitalized singular, such as "Channel". */
  what: string;
  listLabel: string;
  listHref: string;
}) {
  return (
    <Page>
      <PageHeader
        title={`${what} not found`}
        breadcrumbs={[
          { label: listLabel, href: listHref },
          { label: `${what} not found`, href: `${listHref}#not-found` },
        ]}
      />
      <EmptyState
        title={`This ${what.toLowerCase()} is not in the configuration`}
        description="It may have been removed by a configuration change, or the link is out of date."
        action={<Button href={listHref}>Back to {listLabel.toLowerCase()}</Button>}
      />
    </Page>
  );
}
