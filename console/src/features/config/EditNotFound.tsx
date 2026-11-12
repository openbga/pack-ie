import { Button } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/Page";

/** An edit page whose record is not in the configuration (removed, or a stale link). */
export function EditNotFound({
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
    <>
      <PageHeader
        title={`${what} not found`}
        breadcrumbs={[
          { label: listLabel, href: listHref },
          { label: `${what} not found`, href: `${listHref}#not-found` },
        ]}
      />
      <EmptyState
        title={`This ${what.toLowerCase()} was not found`}
        description="It may have been removed by a configuration change."
        action={<Button href={listHref}>Back to {listLabel.toLowerCase()}</Button>}
      />
    </>
  );
}
