import { useCallback, useEffect, useRef, useState } from "react";
import { ToggleButton, ToggleButtonGroup } from "react-aria-components";
import { useEngineApi } from "@/api";
import type {
  BodyFormat,
  BodySelector,
  EngineError,
  MessageBody,
  MessageFields,
} from "@/api/types";
import { Alert } from "@/components/Alert";
import { Button } from "@/components/Button";
import { ErrorState } from "@/components/ErrorState";
import { LoadingState } from "@/components/LoadingState";
import { useToast } from "@/components/Toast";
import { toEngineError } from "@/lib/errors";
import { formatBytes } from "@/lib/format";
import { useOnIdle } from "@/lib/idle";
import styles from "./BodyViewer.module.css";
import { FieldsView } from "./FieldsView";
import { scrollRegion } from "./scrollRegion";

type Load =
  | { status: "loading" }
  | { status: "error"; error: EngineError }
  | { status: "ready"; body: MessageBody };

type Loads = Partial<Record<BodyFormat, Load>>;

/**
 * Reads one body from the engine, on request only, in the format asked for. The renderings
 * live in this component's state and nowhere else: they are not in the query cache, the
 * address or the window title, and `forget` drops them, also those still being read: an answer
 * that arrives after `forget` (or after the viewer is gone) is ignored.
 */
function useBody(messageId: string, which: BodySelector) {
  const api = useEngineApi();
  const [loads, setLoads] = useState<Loads>({});
  // Counts the times what was read has been dropped; a read settles only in its own generation.
  const generation = useRef(0);
  const deliveryId = which.kind === "transformed" ? which.deliveryId : null;
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );
  const load = useCallback(
    (format: BodyFormat) => {
      const selector: BodySelector =
        deliveryId === null ? { kind: "original" } : { kind: "transformed", deliveryId };
      const started = generation.current;
      const settle = (result: Load) => {
        if (generation.current === started)
          setLoads((current) => ({ ...current, [format]: result }));
      };
      setLoads((current) => ({ ...current, [format]: { status: "loading" } }));
      api
        .getMessageBody({ messageId, which: selector, as: format })
        .then((body) => settle({ status: "ready", body }))
        .catch((error: unknown) => settle({ status: "error", error: toEngineError(error) }));
    },
    [api, messageId, deliveryId],
  );
  const forget = useCallback(() => {
    generation.current += 1;
    setLoads({});
  }, []);
  return { loads, load, forget };
}

type FieldsLoad =
  | { status: "loading" }
  | { status: "error"; error: EngineError }
  | { status: "ready"; fields: MessageFields };

/**
 * Reads the parsed fields of a message, on request only: they are message content, and the
 * engine records the view in the audit log. Held in this component's state like a body, and
 * dropped the same way, with a read still in flight.
 */
function useFields(messageId: string) {
  const api = useEngineApi();
  const [load, setLoad] = useState<FieldsLoad | undefined>(undefined);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );
  const read = useCallback(() => {
    const started = generation.current;
    const settle = (result: FieldsLoad) => {
      if (generation.current === started) setLoad(result);
    };
    setLoad({ status: "loading" });
    api
      .getMessageFields(messageId)
      .then((fields) => settle({ status: "ready", fields }))
      .catch((error: unknown) => settle({ status: "error", error: toEngineError(error) }));
  }, [api, messageId]);
  const forget = useCallback(() => {
    generation.current += 1;
    setLoad(undefined);
  }, []);
  return { load, read, forget };
}

/** What the viewer shows a body as: the engine's text or hex, or the fields it read. */
type View = BodyFormat | "fields";

interface BodyViewerProps {
  messageId: string;
  which: BodySelector;
  /** Names the body in labels, such as "Original message". */
  name: string;
  /**
   * Offers the "Fields" view: the fields the engine read from this body, fetched by an explicit
   * "Show content" and held in memory, drawn only here. Set for the original message.
   */
  hasFields?: boolean;
}

/**
 * One stored body, opened by an explicit action (a store query served to this window only,
 * spec §6) as text with control characters made visible, or as a hex dump; the engine renders
 * both, and as the fields the engine read. Message bodies are PHI: they are
 * drawn here as text and nowhere else. When the console goes idle the viewer closes and
 * drops everything it read, also what was still being read; opening it again asks the engine
 * again, and the engine records that view.
 */
export function BodyViewer({ messageId, which, name, hasFields = false }: BodyViewerProps) {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<View>("text");
  // The fields are read separately, so only text and hex are loaded here; text stands for fields.
  const format: BodyFormat = view === "hex" ? "hex" : "text";
  const { loads, load, forget } = useBody(messageId, which);
  const parsed = useFields(messageId);
  const current = loads[format];
  function close() {
    forget();
    parsed.forget();
    setOpen(false);
  }
  useOnIdle(() => {
    close();
    setView("text");
  });
  useEffect(() => {
    if (open && view !== "fields" && current === undefined) load(format);
  }, [open, view, current, format, load]);

  if (!open) {
    return (
      <div className={styles.viewer}>
        <div className={styles.tools}>
          <Button onPress={() => setOpen(true)}>Show {name.toLowerCase()}</Button>
        </div>
        <p className={styles.empty}>
          Opening a message reads it from the engine's store for this window only. It is not kept in
          the address or the window title. The engine records each view in the audit log with your
          account.
        </p>
      </div>
    );
  }
  return (
    <div className={styles.viewer}>
      <Tools
        name={name}
        view={view}
        hasFields={hasFields}
        onView={setView}
        body={current?.status === "ready" ? current.body : null}
        onReload={() => load(format)}
        onHide={close}
      />
      <Content
        name={name}
        view={view}
        fields={parsed}
        load={current}
        onRetry={() => load(format)}
      />
    </div>
  );
}

interface ToolsProps {
  name: string;
  view: View;
  hasFields: boolean;
  onView: (view: View) => void;
  body: MessageBody | null;
  onReload: () => void;
  onHide: () => void;
}

function Tools({ name, view, hasFields, onView, body, onReload, onHide }: ToolsProps) {
  // What is copied with the fields shown is the text.
  const format = view === "hex" ? "hex" : "text";
  const toast = useToast();
  async function copy() {
    if (body === null) return;
    try {
      await navigator.clipboard.writeText(body.content);
      toast.show({ tone: "success", title: `${name} copied as ${format}` });
    } catch {
      toast.show({ tone: "danger", title: `${name} could not be copied` });
    }
  }
  return (
    <div className={styles.tools}>
      <ToggleButtonGroup
        aria-label={`${name} view`}
        selectionMode="single"
        disallowEmptySelection
        selectedKeys={[view]}
        onSelectionChange={(keys) => onView(viewOf(keys))}
        className={styles.group}
      >
        <ToggleButton id="text" className={styles.toggle}>
          Text
        </ToggleButton>
        <ToggleButton id="hex" className={styles.toggle}>
          Hex
        </ToggleButton>
        {hasFields && (
          <ToggleButton id="fields" className={styles.toggle}>
            Fields
          </ToggleButton>
        )}
      </ToggleButtonGroup>
      {body !== null && <span className={styles.size}>{formatBytes(body.byteLength)}</span>}
      <Button isDisabled={body === null} onPress={() => void copy()}>
        Copy as {format}
      </Button>
      <Button onPress={onReload}>Reload</Button>
      <Button variant="quiet" onPress={onHide}>
        Hide
      </Button>
    </div>
  );
}

/** The one view chosen in the toggle group, whatever the group reports. */
function viewOf(keys: Set<unknown> | "all"): View {
  if (keys !== "all" && keys.has("hex")) return "hex";
  if (keys !== "all" && keys.has("fields")) return "fields";
  return "text";
}

interface ContentProps {
  name: string;
  view: View;
  fields: ReturnType<typeof useFields>;
  load: Load | undefined;
  onRetry: () => void;
}

/** The Fields view: asks before reading, since the engine records it in the audit log. */
function FieldsContent({ name, fields }: { name: string; fields: ContentProps["fields"] }) {
  const { load, read } = fields;
  if (load === undefined) {
    return (
      <div className={styles.tools}>
        <Button onPress={read}>Show content</Button>
        <span className={styles.empty}>
          Reads the parsed fields of this message from the engine. The engine records this in the
          audit log with your account.
        </span>
      </div>
    );
  }
  if (load.status === "loading") return <LoadingState label="Loading fields…" />;
  if (load.status === "error") {
    return <ErrorState title="The fields could not be loaded" error={load.error} onRetry={read} />;
  }
  return <FieldsView fields={load.fields} name={name} />;
}

function Content({ name, view, fields, load, onRetry }: ContentProps) {
  if (view === "fields") return <FieldsContent name={name} fields={fields} />;
  if (load === undefined || load.status === "loading") {
    return <LoadingState label={`Loading ${name.toLowerCase()}…`} />;
  }
  if (load.status === "error") {
    return (
      <ErrorState
        title={`The ${name.toLowerCase()} could not be loaded`}
        error={load.error}
        onRetry={onRetry}
      />
    );
  }
  const { body } = load;
  const format = view === "hex" ? "hex" : "text";
  return (
    <>
      {body.truncated && (
        <Alert tone="warning" title="This message is cut short" compact>
          It is {formatBytes(body.byteLength)}, more than the engine shows. Only the beginning is
          displayed; all of it is stored.
        </Alert>
      )}
      <div {...scrollRegion(`${name} as ${format}`)} className={styles.scroll}>
        {body.byteLength === 0 ? (
          <p className={styles.empty}>This message is empty.</p>
        ) : (
          <pre className={styles.dump} data-view={format}>
            {body.content}
          </pre>
        )}
      </div>
    </>
  );
}
