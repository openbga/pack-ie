/**
 * Test support: the strict shape of the runtime document (docs/spec/configuration.md sections 4
 * to 8). The engine rejects unknown fields and fields without a default that are missing, so a
 * document the console sends must pass this. Values are not checked, only names and presence.
 */

import { isRecord } from "../util";
import { STRUCTS } from "./specStructs";

function conforms(value: unknown, struct: string, where: string): string[] {
  const schema = STRUCTS[struct];
  if (schema === undefined) return [`${where}: no schema for ${struct}`];
  if (!isRecord(value)) return [`${where}: not an object`];
  const problems = Object.keys(value)
    .filter((key) => !schema.known.includes(key))
    .map((key) => `${where}: unknown field ${key}`);
  for (const key of schema.required ?? []) {
    if (value[key] === undefined) problems.push(`${where}: missing ${key}`);
  }
  return problems;
}

const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

/** A filter expression or condition: a comparison, or a combination of expressions. */
function expressionProblems(value: unknown, where: string): string[] {
  if (!isRecord(value)) return [`${where}: not an object`];
  for (const key of ["all", "any"]) {
    if (value[key] !== undefined) {
      return list(value[key]).flatMap((e, i) => expressionProblems(e, `${where}.${key}[${i}]`));
    }
  }
  if (value.not !== undefined) return expressionProblems(value.not, `${where}.not`);
  return conforms(value, "comparison", where);
}

/** The `steps` of a rule, part or operand: each step is checked by its `step` name. */
function stepsProblems(steps: unknown, where: string): string[] {
  if (steps === undefined) return [];
  return list(steps).flatMap((step, i) =>
    conforms(step, `step_${isRecord(step) ? String(step.step) : ""}`, `${where}[${i}]`),
  );
}

const OPERATORS = ["add", "sub", "mul", "div", "min", "max"];

/** A `calculate` formula: a constant, an operand, `abs`, or an operator over formulas. */
function formulaProblems(formula: unknown, where: string): string[] {
  if (typeof formula === "string" || typeof formula === "number") return [];
  if (!isRecord(formula)) return [`${where}: not a formula`];
  if (formula.find !== undefined) {
    return [
      ...conforms(formula, "formula_find", where),
      ...expressionProblems(formula.where, `${where}.where`),
      ...stepsProblems(formula.steps, `${where}.steps`),
    ];
  }
  if (formula.from !== undefined) {
    return [
      ...conforms(formula, "formula_from", where),
      ...stepsProblems(formula.steps, `${where}.steps`),
    ];
  }
  const keys = Object.keys(formula);
  const key = keys[0];
  if (keys.length !== 1 || key === undefined) return [`${where}: not a formula`];
  const inner = formula[key];
  if (key === "abs") return formulaProblems(inner, `${where}.abs`);
  if (!OPERATORS.includes(key)) return [`${where}: unknown operator ${key}`];
  return list(inner).flatMap((f, i) => formulaProblems(f, `${where}.${key}[${i}]`));
}

/** The `require` preconditions of a mapping: `{count, min, max}` or `{same}`. */
function requireProblems(requirements: unknown, where: string): string[] {
  return list(requirements).flatMap((item, i) =>
    conforms(
      item,
      isRecord(item) && item.same !== undefined ? "require_same" : "require_count",
      `${where}[${i}]`,
    ),
  );
}

function ruleProblems(rule: unknown, where: string): string[] {
  const op = isRecord(rule) ? String(rule.op) : "";
  const own = conforms(rule, op, where);
  if (!isRecord(rule)) return own;
  const nested = [
    ...stepsProblems(rule.steps, `${where}.steps`),
    ...(rule.formula === undefined ? [] : formulaProblems(rule.formula, `${where}.formula`)),
    ...list(rule.parts).flatMap((part, i) => [
      ...conforms(part, "part", `${where}.parts[${i}]`),
      ...stepsProblems(isRecord(part) ? part.steps : undefined, `${where}.parts[${i}].steps`),
    ]),
    ...(rule.condition === undefined
      ? []
      : expressionProblems(rule.condition, `${where}.condition`)),
    ...["then", "else", "rules"].flatMap((key) =>
      list(rule[key]).flatMap((r, i) => ruleProblems(r, `${where}.${key}[${i}]`)),
    ),
  ];
  return [...own, ...nested];
}

function mappingProblems(mapping: unknown, where: string): string[] {
  if (mapping === null || mapping === undefined) return [];
  const rules = isRecord(mapping) ? list(mapping.rules) : [];
  return [
    ...conforms(mapping, "mapping", where),
    ...requireProblems(isRecord(mapping) ? mapping.require : undefined, `${where}.require`),
    ...rules.flatMap((rule, i) => ruleProblems(rule, `${where}.rules[${i}]`)),
  ];
}

function settingsProblems(protocol: unknown, settings: unknown, where: string): string[] {
  if (settings === undefined) return protocol === "au" ? [`${where}: AU needs settings`] : [];
  const base = conforms(settings, String(protocol), where);
  if (protocol !== "au" || !isRecord(settings)) return base;
  return [
    ...base,
    ...(settings.framing === undefined
      ? []
      : conforms(settings.framing, "au_framing", `${where}.framing`)),
    ...(settings.format === undefined
      ? []
      : conforms(settings.format, "au_format", `${where}.format`)),
  ];
}

function endpointProblems(endpoint: unknown, where: string): string[] {
  if (!isRecord(endpoint)) return [`${where}: not an object`];
  const settings = isRecord(endpoint.settings) ? endpoint.settings : {};
  const auth = isRecord(settings.auth) ? settings.auth : null;
  const secrets = (auth === null ? [] : ["password", "private_key", "passphrase"])
    .filter((key) => isRecord(auth) && auth[key] !== undefined && auth[key] !== null)
    .flatMap((key) => conforms(auth?.[key], "secret", `${where}.settings.auth.${key}`));
  return [
    ...conforms(endpoint, "endpoint", where),
    ...conforms(settings, String(endpoint.kind), `${where}.settings`),
    ...(auth === null
      ? []
      : conforms(auth, auth.method === "key" ? "key" : "password", `${where}.settings.auth`)),
    ...secrets,
  ];
}

function deliveryProblems(delivery: unknown, where: string): string[] {
  if (delivery === undefined) return [];
  const retry = isRecord(delivery) ? delivery.retry : undefined;
  return [
    ...conforms(delivery, "delivery", where),
    ...(retry === undefined ? [] : conforms(retry, "retry", `${where}.retry`)),
  ];
}

function destinationProblems(destination: unknown, where: string): string[] {
  const d = isRecord(destination) ? destination : {};
  return [
    ...conforms(destination, "destination", where),
    ...settingsProblems(d.protocol, d.settings, `${where}.settings`),
    ...list(d.filters).flatMap((e, i) => expressionProblems(e, `${where}.filters[${i}]`)),
    ...mappingProblems(d.mapping, `${where}.mapping`),
    ...deliveryProblems(d.delivery, `${where}.delivery`),
  ];
}

function channelProblems(channel: unknown, where: string): string[] {
  if (!isRecord(channel)) return [`${where}: not an object`];
  const nested = (key: string, struct: string) =>
    channel[key] === undefined ? [] : conforms(channel[key], struct, `${where}.${key}`);
  const source = isRecord(channel.source) ? channel.source : {};
  return [
    ...conforms(channel, String(channel.kind), where),
    ...nested("connection", "connection"),
    ...nested("buffer", "buffer"),
    ...nested("retry", "retry"),
    ...nested("tap", "tap"),
    ...(isRecord(channel.tap) && channel.tap.retention !== undefined
      ? conforms(channel.tap.retention, "tapRetention", `${where}.tap.retention`)
      : []),
    ...nested("retention", "retention"),
    ...(channel.source === undefined
      ? []
      : [
          ...conforms(source, "source", `${where}.source`),
          ...settingsProblems(source.protocol, source.settings, `${where}.source.settings`),
        ]),
    ...list(channel.filters).flatMap((e, i) => expressionProblems(e, `${where}.filters[${i}]`)),
    ...deliveryProblems(channel.delivery, `${where}.delivery`),
    ...list(channel.destinations).flatMap((d, j) =>
      destinationProblems(d, `${where}.destinations[${j}]`),
    ),
  ];
}

/** Every way `document` departs from the specified runtime document; none when it conforms. */
export function documentProblems(document: unknown): string[] {
  if (!isRecord(document)) return ["document: not an object"];
  return [
    ...conforms(document, "document", "document"),
    ...(document.alerts === undefined ? [] : conforms(document.alerts, "alerts", "alerts")),
    ...list(document.endpoints).flatMap((e, i) => endpointProblems(e, `endpoints[${i}]`)),
    ...list(document.channels).flatMap((c, i) => channelProblems(c, `channels[${i}]`)),
  ];
}
