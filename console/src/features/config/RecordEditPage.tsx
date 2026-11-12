import type { ReactNode } from "react";
import { useParams } from "react-router";
import { useEngineQuery } from "@/api/hooks";
import type { ConfigDocument } from "@/api/types";
import { Page } from "@/components/Page";
import { QueryContent } from "@/components/QueryContent";

export interface EditorProps {
  doc: ConfigDocument;
  /** The record being edited; undefined for a new one. */
  id: string | undefined;
  onReload: () => Promise<unknown>;
}

/**
 * The page of a record editor: reads the configuration and opens the editor for the record in
 * the address (`/:id/edit`), or for a new one.
 */
export function RecordEditPage({
  what,
  Editor,
}: {
  what: "channel" | "endpoint";
  Editor: (props: EditorProps) => ReactNode;
}) {
  const { id } = useParams();
  const config = useEngineQuery(["getConfig"], (api) => api.getConfig());
  return (
    <Page>
      <QueryContent
        query={config}
        what="configuration"
        pageTitle={`${id === undefined ? "New" : "Edit"} ${what}`}
      >
        {(doc) => (
          // One editor per record: its form, identity and delete target start from the same one.
          <Editor key={id ?? "new"} doc={doc} id={id} onReload={() => config.refetch()} />
        )}
      </QueryContent>
    </Page>
  );
}
