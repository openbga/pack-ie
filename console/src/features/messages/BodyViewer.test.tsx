import { act, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageBody } from "@/api/types";
import { IdleProvider } from "@/lib/idle";
import { renderWithProviders } from "@/lib/test/render";
import { BodyViewer } from "./BodyViewer";
import { fieldsOf, HL7_TEXT, messageBody, stubApi } from "./fixtures";

const IDLE_MS = 15 * 60_000 + 1000;

function original(api = stubApi()) {
  const rendered = renderWithProviders(
    <IdleProvider>
      <BodyViewer
        messageId="msg-000001"
        which={{ kind: "original" }}
        name="Original message"
        hasFields
      />
    </IdleProvider>,
    { api },
  );
  return { ...rendered, api };
}

const goIdle = () => act(() => void vi.advanceTimersByTime(IDLE_MS));

/** A read that settles when the test says so. */
function deferred<T>() {
  let settle: (value: T) => void = () => {};
  const promise = new Promise<T>((resolve) => {
    settle = resolve;
  });
  return { promise, settle };
}

describe("BodyViewer when the console goes idle", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => vi.useRealTimers());

  it("closes and drops the body and the fields it holds", async () => {
    const { user } = original();
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    expect(await screen.findByText(/MSH\|/)).toBeTruthy();
    await user.click(screen.getByRole("radio", { name: "Fields" }));
    await user.click(screen.getByRole("button", { name: "Show content" }));
    expect(await screen.findByRole("rowheader", { name: "PID.3" })).toBeTruthy();

    goIdle();
    expect(screen.queryByText(/MSH\|/)).toBeNull();
    expect(screen.queryByText("MRN-00000")).toBeNull();
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.queryByRole("radiogroup")).toBeNull();
    expect(screen.getByRole("button", { name: "Show original message" })).toBeTruthy();
  });

  it("reads the body again, and the fields again, when they are asked for again", async () => {
    const { user, api } = original();
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    await screen.findByText(/MSH\|/);
    goIdle();
    // Nothing revived from before: the viewer is closed, text view, and asks the engine afresh.
    expect(api.getMessageBody).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    await screen.findByText(/MSH\|/);
    expect(api.getMessageBody).toHaveBeenCalledTimes(2);
    expect(api.getMessageFields).not.toHaveBeenCalled();
    await user.click(screen.getByRole("radio", { name: "Fields" }));
    expect(screen.queryByRole("table")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show content" }));
    await screen.findByRole("table");
    expect(api.getMessageFields).toHaveBeenCalledTimes(1);
  });

  it("ignores a body that arrives after the idle time", async () => {
    const late = deferred<MessageBody>();
    const { user, api } = original(stubApi({ getMessageBody: vi.fn(() => late.promise) }));
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    expect(await screen.findByText("Loading original message…")).toBeTruthy();

    goIdle();
    await act(async () => late.settle(messageBody({ content: HL7_TEXT })));
    expect(screen.queryByText(/MSH\|/)).toBeNull();
    expect(screen.getByRole("button", { name: "Show original message" })).toBeTruthy();
    // Opening again is a new read, shown when it arrives.
    api.getMessageBody.mockImplementation(async () => messageBody());
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    expect(await screen.findByText(/MSH\|/)).toBeTruthy();
    expect(api.getMessageBody).toHaveBeenCalledTimes(2);
  });

  it("ignores fields that arrive after the idle time", async () => {
    const late = deferred<ReturnType<typeof fieldsOf>>();
    const { user } = original(stubApi({ getMessageFields: vi.fn(() => late.promise) }));
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    await screen.findByText(/MSH\|/);
    await user.click(screen.getByRole("radio", { name: "Fields" }));
    await user.click(screen.getByRole("button", { name: "Show content" }));
    expect(await screen.findByText("Loading fields…")).toBeTruthy();

    goIdle();
    await act(async () => late.settle(fieldsOf()));
    expect(screen.queryByText("MRN-00000")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    await waitFor(() => expect(screen.getByRole("radio", { name: "Text" })).toBeTruthy());
    await user.click(screen.getByRole("radio", { name: "Fields" }));
    // The late answer did not become the fields of the reopened viewer.
    expect(screen.getByRole("button", { name: "Show content" })).toBeTruthy();
    expect(screen.queryByText("MRN-00000")).toBeNull();
  });

  it("does not let one viewer's content survive in another's", async () => {
    const { user } = renderWithProviders(
      <IdleProvider>
        <section aria-label="first">
          <BodyViewer messageId="msg-000001" which={{ kind: "original" }} name="Original message" />
        </section>
        <section aria-label="second">
          <BodyViewer
            messageId="msg-000001"
            which={{ kind: "transformed", deliveryId: "d-LIS" }}
            name="Transformed message"
          />
        </section>
      </IdleProvider>,
      { api: stubApi() },
    );
    const second = within(screen.getByRole("region", { name: "second" }));
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    await user.click(second.getByRole("button", { name: "Show transformed message" }));
    await screen.findAllByText(/MSH\|/);

    goIdle();
    await user.click(screen.getByRole("button", { name: "Show original message" }));
    await screen.findByText(/MSH\|/);
    // The other body stays closed: nothing reveals it.
    expect(second.queryByText(/MSH\|/)).toBeNull();
    expect(second.getByRole("button", { name: "Show transformed message" })).toBeTruthy();
  });
});
