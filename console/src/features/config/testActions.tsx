// Steps of the config form shared by several test files.
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { expect } from "vitest";
import type { EngineApi } from "@/api/engine";
import type { ConfigDocument, MessageChannelConfig } from "@/api/types";
import { renderRoutes } from "@/lib/test/router";
import { ChannelEditPage } from "./ChannelEditPage";
import { EndpointEditPage } from "./EndpointEditPage";
import { stubApi } from "./testFixtures";

type StubApi = ReturnType<typeof stubApi>;

export const channelRoutes = [
  { path: "/channels/new", element: <ChannelEditPage /> },
  { path: "/channels/:id/edit", element: <ChannelEditPage /> },
];

export const endpointRoutes = [
  { path: "/endpoints/new", element: <EndpointEditPage /> },
  { path: "/endpoints/:id/edit", element: <EndpointEditPage /> },
];

export const textbox = (scope: HTMLElement, name: RegExp) =>
  within(scope).getByRole("textbox", { name });

/** The channel's own label; destinations have one too. */
export const channelLabel = () => textbox(group("General"), /^Label/);

/** A form group (a fieldset) by its legend. */
export const group = (name: string, scope: HTMLElement = document.body) =>
  within(scope).getByRole("group", { name });

/** Chooses `option` in the select whose button is labelled `label` within `scope`. */
export async function pick(
  user: UserEvent,
  scope: HTMLElement,
  label: string | RegExp,
  option: string | RegExp,
) {
  const name = typeof label === "string" ? new RegExp(label) : label;
  await user.click(await within(scope).findByRole("button", { name }));
  await user.click(await screen.findByRole("option", { name: option }));
}

/** Types the reason a change asks for (the apply review, a deletion, a rollback, a sign-off). */
export async function typeNote(user: UserEvent, note = "Synthetic change") {
  await user.type(screen.getByRole("textbox", { name: /^(Change note|Reason for)/ }), note);
}

/** Adds a copy rule to a destination, which is what makes it more than a pass-through. */
export async function addCopyRule(user: UserEvent, destination = 1, from = "R.4", to = "OBX.5") {
  await user.click(
    screen.getByRole("button", { name: `Add destination ${destination} mapping rule` }),
  );
  const rules = screen.getAllByRole("group", {
    name: new RegExp(`^Destination ${destination} mapping rule \\d`),
  });
  const rule = rules.at(-1) as HTMLElement;
  await user.type(within(rule).getByRole("textbox", { name: /^From/ }), from);
  await user.type(within(rule).getByRole("textbox", { name: /^To$/ }), to);
}

/**
 * Chooses an endpoint in the searchable picker labelled `label` (`Endpoint`, `Endpoint A`):
 * a click opens the whole list, then the option is chosen by its label.
 */
export async function pickEndpoint(
  user: UserEvent,
  scope: HTMLElement,
  label: string,
  option: string | RegExp,
) {
  await user.click(within(scope).getByRole("combobox", { name: new RegExp(label) }));
  await user.click(await screen.findByRole("option", { name: option }));
}

/** Removes a destination the channel already has, which asks first, and confirms. */
export async function removeSavedDestination(user: UserEvent, number: number) {
  await user.click(screen.getByRole("button", { name: `Remove destination ${number}` }));
  await user.click(await screen.findByRole("button", { name: "Remove destination" }));
}

/** Renders the channel editor on a new channel and chooses its kind. */
export async function openNewChannel<A extends EngineApi>(kind: "Bridge" | "Message", api: A) {
  const rendered = renderRoutes(channelRoutes, "/channels/new", { api });
  await screen.findByRole("button", { name: /Kind/ });
  await pick(rendered.user, document.body, "Kind", kind);
  return { api, ...rendered };
}

/** Renders the channel editor on a saved channel and waits for its heading. */
export async function openChannel(id: string, heading: string, api = stubApi()) {
  const rendered = renderRoutes(channelRoutes, `/channels/${id}/edit`, { api });
  await screen.findByRole("heading", { level: 1, name: heading });
  return { api, ...rendered };
}

/** The sample message channel, which has one destination with two mapping rules. */
export const openResults = (api = stubApi()) => openChannel("results", "Results to LIS", api);

/** Renders the endpoint editor on a new endpoint, ready to choose its kind. */
export async function openNewEndpoint<A extends EngineApi>(api: A) {
  const rendered = renderRoutes(endpointRoutes, "/endpoints/new", { api });
  await screen.findByRole("button", { name: /Kind/ });
  return { api, ...rendered };
}

/** Renders the endpoint editor on a saved endpoint and waits for its heading. */
export async function openEndpoint<A extends EngineApi>(id: string, heading: string, api: A) {
  const rendered = renderRoutes(endpointRoutes, `/endpoints/${id}/edit`, { api });
  await screen.findByRole("heading", { level: 1, name: heading });
  return { api, ...rendered };
}

/** Fills a new TCP client endpoint with the values the tests expect to find in the document. */
export async function fillTcpClient(user: UserEvent) {
  await pick(user, document.body, "Kind", "TCP client");
  await user.type(screen.getByRole("textbox", { name: /^Label/ }), "Chem LIS");
  await user.type(screen.getByRole("textbox", { name: /^Host/ }), "lis.example.test");
  await user.type(screen.getByRole("textbox", { name: /^Port/ }), "3002");
}

/**
 * Fills the label, endpoint and (when given) protocol of destination `number`, and returns
 * its group.
 */
export async function fillDestination(
  user: UserEvent,
  label: string,
  endpoint: string | RegExp,
  protocol?: string,
  number = 1,
) {
  const destination = group(`Destination ${number}`);
  await user.type(textbox(destination, /^Label/), label);
  await pickEndpoint(user, destination, "Endpoint", endpoint);
  if (protocol) await pick(user, destination, "Protocol", protocol);
  return destination;
}

/** The labels of the choices of the select `label` within `scope`; leaves the list closed. */
export async function optionsOf(user: UserEvent, scope: HTMLElement, label: string) {
  await user.click(within(scope).getByRole("button", { name: new RegExp(label) }));
  const names = (await screen.findAllByRole("option")).map((option) => option.textContent);
  await user.keyboard("{Escape}");
  return names;
}

/** Clicks Validate and returns the message channel at `index` of the document the engine got. */
export async function validatedChannel(user: UserEvent, api: StubApi, index = 1) {
  await user.click(screen.getByRole("button", { name: "Validate" }));
  await waitFor(() => expect(api.validateConfig).toHaveBeenCalled());
  const [document] = api.validateConfig.mock.calls.at(-1) ?? [];
  return (document as ConfigDocument).channels[index] as MessageChannelConfig;
}

/** Saves and applies with a note, and returns the document the engine was asked to apply. */
export async function applyAndRead(user: UserEvent, api: StubApi) {
  await user.click(screen.getByRole("button", { name: "Save and apply" }));
  await typeNote(user);
  await user.click(await screen.findByRole("button", { name: /^Apply version/ }));
  await waitFor(() => expect(api.applyConfig).toHaveBeenCalledTimes(1));
  const [document] = api.applyConfig.mock.calls[0] ?? [];
  return document;
}

/** The message channel `id` of an applied document. */
export function messageChannel(document: { channels: unknown[] } | undefined, id: string) {
  const channels = (document?.channels ?? []) as { id: string; kind: string }[];
  return channels.find((channel) => channel.id === id) as MessageChannelConfig | undefined;
}

/** Clicks the finding in the validation results and expects focus to land on its field. */
export async function followFinding(user: UserEvent, message: string, field: () => HTMLElement) {
  const issues = screen.getByRole("region", { name: "Validation results" });
  await user.click(within(issues).getByRole("button", { name: message }));
  await waitFor(() => expect(document.activeElement).toBe(field()));
}
