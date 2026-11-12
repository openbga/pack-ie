// A `calculate` formula as text, the way a person writes it: `$.4 * 0.0555`,
// `(OBX[1].5 + OBX[2].5) / 2`, `max($.4, 0)`. The engine's formula is a tree
// (docs/spec/configuration.md §8.2); `parseFormula` and `printFormula` turn one into the other
// without loss: a chain such as `a + b + c` is one `add` of three, and a bracket keeps a nested
// one apart, so what is shown is what the engine reads. An operand that needs more than a path
// (text steps, or the first record that matches a condition) is written `@name` and defined
// beside the formula; `splitOperands` finds and names them.
import type { Formula, FormulaOp } from "@/api/types";

const SYMBOLS: Record<string, FormulaOp> = { "+": "add", "-": "sub", "*": "mul", "/": "div" };
const SYMBOL_OF: Partial<Record<FormulaOp, string>> = {
  add: "+",
  sub: "-",
  mul: "*",
  div: "/",
};
const FUNCTIONS: ReadonlySet<string> = new Set<FormulaOp>(["min", "max", "abs"]);
const PRECEDENCE: Partial<Record<FormulaOp, number>> = { add: 1, sub: 1, mul: 2, div: 2 };

export type ParsedFormula = { formula: Formula } | { error: string };

interface Token {
  kind: "number" | "path" | "operand" | "symbol";
  text: string;
  at: number;
}

function tokenize(text: string): Token[] | string {
  const tokens: Token[] = [];
  let i = 0;
  while (i < text.length) {
    const char = text.charAt(i);
    if (/\s/.test(char)) {
      i += 1;
      continue;
    }
    const rest = text.slice(i);
    const number = /^(?:\d+(?:\.\d*)?|\.\d+)(?![\w$[\].])/.exec(rest);
    if (number) {
      tokens.push({ kind: "number", text: number[0], at: i });
      i += number[0].length;
      continue;
    }
    const operand = /^@[A-Za-z][A-Za-z0-9_]*/.exec(rest);
    if (operand) {
      tokens.push({ kind: "operand", text: operand[0], at: i });
      i += operand[0].length;
      continue;
    }
    const path = /^[A-Za-z$][A-Za-z0-9_$.[\]]*/.exec(rest);
    if (path) {
      tokens.push({ kind: "path", text: path[0], at: i });
      i += path[0].length;
      continue;
    }
    if ("+-*/(),".includes(char)) {
      tokens.push({ kind: "symbol", text: char, at: i });
      i += 1;
      continue;
    }
    return /\d/.test(char)
      ? `A number cannot run into letters or brackets, at character ${i + 1}.`
      : `Unexpected "${char}" at character ${i + 1}. Use numbers, paths, + - * / and brackets.`;
  }
  return tokens;
}

class Parser {
  private position = 0;
  /** Nodes built from an unbracketed chain, which a further operator of the same kind extends. */
  private readonly chains = new WeakSet<object>();

  constructor(
    private readonly tokens: Token[],
    private readonly operands: Readonly<Record<string, Formula>>,
  ) {}

  private peek(): Token | undefined {
    return this.tokens[this.position];
  }

  private fail(expected: string): never {
    const token = this.peek();
    throw new FormulaSyntax(
      token
        ? `Expected ${expected} at character ${token.at + 1}, found "${token.text}".`
        : `Expected ${expected} at the end of the formula.`,
    );
  }

  private take(text: string): boolean {
    const token = this.peek();
    if (token?.kind === "symbol" && token.text === text) {
      this.position += 1;
      return true;
    }
    return false;
  }

  parse(): Formula {
    const formula = this.expression();
    if (this.peek()) this.fail("an operator or the end of the formula");
    return formula;
  }

  private chain(operators: string, next: () => Formula): Formula {
    let left = next();
    for (;;) {
      const token = this.peek();
      if (token?.kind !== "symbol" || !operators.includes(token.text)) return left;
      this.position += 1;
      const op = SYMBOLS[token.text] as FormulaOp;
      const right = next();
      if ("op" in left && left.op === op && this.chains.has(left)) {
        left.args.push(right);
      } else {
        left = { op, args: [left, right] };
        this.chains.add(left);
      }
    }
  }

  private expression(): Formula {
    return this.chain("+-", () => this.term());
  }

  private term(): Formula {
    return this.chain("*/", () => this.factor());
  }

  private factor(): Formula {
    const token = this.peek();
    if (token === undefined) return this.fail("a number, a path or a bracket");
    this.position += 1;
    if (token.kind === "number") return { constant: token.text };
    if (token.kind === "operand") return this.operand(token);
    if (token.kind === "path") return this.pathOrCall(token);
    if (token.text === "-") return this.negativeNumber(token);
    if (token.text === "(") return this.bracket();
    this.position -= 1;
    return this.fail("a number, a path or a bracket");
  }

  private negativeNumber(sign: Token): Formula {
    const number = this.tokens[this.position];
    if (number?.kind === "number" && number.at === sign.at + 1) {
      this.position += 1;
      return { constant: `-${number.text}` };
    }
    throw new FormulaSyntax(
      `A minus sign can only start a number, at character ${sign.at + 1}. Write 0 - x or x * -1.`,
    );
  }

  private bracket(): Formula {
    const inner = this.expression();
    if (!this.take(")")) this.fail("a closing bracket");
    // A bracket closes the chain: a further operator of the same kind nests instead of joining.
    this.chains.delete(inner);
    return inner;
  }

  private operand(token: Token): Formula {
    const operand = this.operands[token.text.slice(1)];
    if (operand === undefined) {
      throw new FormulaSyntax(
        `No operand is named ${token.text}, at character ${token.at + 1}. Define it under Operands.`,
      );
    }
    return operand;
  }

  private pathOrCall(token: Token): Formula {
    if (FUNCTIONS.has(token.text) && this.take("(")) return this.call(token.text as FormulaOp);
    return { from: token.text, steps: [] };
  }

  private call(op: FormulaOp): Formula {
    const args = [this.expression()];
    while (this.take(",")) args.push(this.expression());
    if (!this.take(")")) this.fail("a comma or a closing bracket");
    if (op === "abs" && args.length !== 1) {
      throw new FormulaSyntax("abs takes one value, for example abs($.4).");
    }
    if (op !== "abs" && args.length < 2) {
      throw new FormulaSyntax(`${op} takes at least two values, for example ${op}($.4, 0).`);
    }
    return { op, args };
  }
}

class FormulaSyntax extends Error {}

export function parseFormula(
  text: string,
  operands: Readonly<Record<string, Formula>> = {},
): ParsedFormula {
  if (text.trim() === "") return { error: "Enter a formula, for example $.4 * 0.0555." };
  const tokens = tokenize(text);
  if (typeof tokens === "string") return { error: tokens };
  try {
    return { formula: new Parser(tokens, operands).parse() };
  } catch (cause) {
    if (cause instanceof FormulaSyntax) return { error: cause.message };
    throw cause;
  }
}

/** Whether an operand needs more than a path, so that it is named and defined beside the formula. */
function isNamedOperand(formula: Formula): boolean {
  return "find" in formula || ("from" in formula && formula.steps.length > 0);
}

interface PrintOptions {
  /** Shows × and ÷ for the signs of a sentence. */
  pretty?: boolean;
  /** The name of a named operand; without it the operand prints as `@operand`. */
  nameOf?: (operand: Formula) => string;
}

/** The text that parses back to `formula` (with the operands named by `nameOf`). */
export function printFormula(formula: Formula, options: PrintOptions = {}): string {
  if ("constant" in formula) return formula.constant;
  if (isNamedOperand(formula)) return `@${options.nameOf?.(formula) ?? "operand"}`;
  if ("from" in formula) return formula.from;
  const symbol = SYMBOL_OF[formula.op];
  if (symbol === undefined) {
    return `${formula.op}(${formula.args.map((arg) => printFormula(arg, options)).join(", ")})`;
  }
  const own = PRECEDENCE[formula.op] ?? 0;
  const shown = options.pretty ? ({ "*": "×", "/": "÷", "-": "−" }[symbol] ?? symbol) : symbol;
  return formula.args
    .map((arg, index) => {
      const text = printFormula(arg, options);
      if (!("op" in arg) || PRECEDENCE[arg.op] === undefined) return text;
      const theirs = PRECEDENCE[arg.op] ?? 0;
      // A tighter operator needs no bracket. The first of a chain may be a looser one of the
      // same strength (`a + b - c`) but never the same operator (that would be flattened).
      const bare = theirs > own || (theirs === own && index === 0 && arg.op !== formula.op);
      return bare ? text : `(${text})`;
    })
    .join(` ${shown} `);
}

/** Every path a formula reads straight from the message, in order and without repeats. */
export function formulaPaths(formula: Formula): string[] {
  if ("constant" in formula || "find" in formula) return [];
  if ("from" in formula) return [formula.from];
  return [...new Set(formula.args.flatMap(formulaPaths))];
}

const NAME_WORD = /^[A-Za-z][A-Za-z0-9_]*$/;

/** A readable name for an operand: the code a `find` looks for (`@na`), else `match` or `value`. */
function nameHint(operand: Formula): string {
  if ("find" in operand) {
    const where = operand.where;
    if ("operator" in where && where.operator === "equals" && NAME_WORD.test(where.value)) {
      return where.value.toLowerCase();
    }
    return "match";
  }
  return "value";
}

/**
 * The formula as text, and the operands it names: every operand with text steps or a `find`
 * gets a short name (the same operand twice gets one name), in the order they appear.
 */
export function splitOperands(formula: Formula): {
  text: string;
  operands: { name: string; operand: Formula }[];
} {
  const operands: { name: string; operand: Formula }[] = [];
  const nameOf = (operand: Formula): string => {
    const key = JSON.stringify(operand);
    const known = operands.find((each) => JSON.stringify(each.operand) === key);
    if (known) return known.name;
    const base = nameHint(operand);
    let name = base;
    for (let n = 2; operands.some((each) => each.name === name); n += 1) name = `${base}${n}`;
    operands.push({ name, operand });
    return name;
  };
  const text = printFormula(formula, { nameOf });
  return { text, operands };
}
