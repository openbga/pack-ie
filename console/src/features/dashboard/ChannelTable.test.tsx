import { act, fireEvent, screen, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { columnTexts } from "@/lib/test/grid";
import { mockGridLayout } from "@/lib/test/layout";
import { renderRoutes } from "@/lib/test/router";
import { channel, snapshot } from "../shared-operate/testFixtures";
import { applyOrder, ChannelTable } from "./ChannelTable";
import { type ChannelRow, rankChannels } from "./rank";

const row = (id: string, label = id): ChannelRow =>
  rankChannels(snapshot({ channels: [channel({ id, label })] }))[0] as ChannelRow;

describe("applyOrder", () => {
  it("keeps the held order, updates data, skips gone rows and puts new ones last", () => {
    const rows = [row("c"), row("a"), row("new"), row("b")];
    expect(applyOrder(rows, ["a", "gone", "b", "c"]).map((r) => r.channel.id)).toEqual([
      "a",
      "b",
      "c",
      "new",
    ]);
  });
});

let setRows: (rows: ChannelRow[]) => void = () => undefined;
function Harness({ initial }: { initial: ChannelRow[] }) {
  const [rows, set] = useState(initial);
  setRows = set;
  return <ChannelTable rows={rows} />;
}

const names = () => columnTexts(0, "Channels").map((text) => text?.replace(/ Bridge$/, ""));

beforeEach(() => mockGridLayout());

describe("ChannelTable", () => {
  it("holds the rows' places while the pointer is over the table, then follows the ranking", async () => {
    renderRoutes([{ path: "/d", element: <Harness initial={[row("one"), row("two")]} /> }], "/d");
    await screen.findByRole("grid", { name: "Channels" });
    expect(names()).toEqual(["one", "two"]);

    const frame = screen.getByRole("grid", { name: "Channels" }).parentElement as HTMLElement;
    fireEvent.pointerEnter(frame);
    act(() => setRows([row("two"), row("one")]));
    expect(names()).toEqual(["one", "two"]);

    fireEvent.pointerLeave(frame);
    expect(names()).toEqual(["two", "one"]);
  });

  it("holds the places while focus is in the table too", async () => {
    renderRoutes([{ path: "/d", element: <Harness initial={[row("one"), row("two")]} /> }], "/d");
    await screen.findByRole("grid", { name: "Channels" });
    const cell = within(screen.getByRole("grid", { name: "Channels" })).getAllByRole(
      "gridcell",
    )[0] as HTMLElement;
    act(() => cell.focus());
    act(() => setRows([row("two"), row("one")]));
    expect(names()).toEqual(["one", "two"]);
    act(() => cell.blur());
    expect(names()).toEqual(["two", "one"]);
  });
});
