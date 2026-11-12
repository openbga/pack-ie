import type { Counters } from "@/api/types";
import { type MiniColumn, MiniTable } from "@/components/MiniTable";
import { formatBytes, formatCount } from "@/lib/format";

interface CounterRow {
  id: string;
  direction: string;
  messages: number;
  bytes: number;
}

const COLUMNS: readonly MiniColumn<CounterRow>[] = [
  { id: "direction", header: "Direction", cell: (row) => row.direction },
  { id: "messages", header: "Messages", align: "end", cell: (row) => formatCount(row.messages) },
  { id: "bytes", header: "Bytes", align: "end", cell: (row) => formatBytes(row.bytes) },
];

/** Messages (where they apply) and bytes in each direction since the engine started. */
export function CountersTable({
  counters,
  label,
  messages = true,
}: {
  counters: Counters;
  label: string;
  /** False where messages do not apply (a serial port or a TCP socket carries bytes only). */
  messages?: boolean;
}) {
  const rows: CounterRow[] = [
    { id: "in", direction: "In", messages: counters.messagesIn, bytes: counters.bytesIn },
    { id: "out", direction: "Out", messages: counters.messagesOut, bytes: counters.bytesOut },
  ];
  const columns = messages ? COLUMNS : COLUMNS.filter((column) => column.id !== "messages");
  return <MiniTable label={label} columns={columns} rows={rows} getRowId={(row) => row.id} />;
}
