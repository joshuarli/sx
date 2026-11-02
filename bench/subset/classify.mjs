import { readFileSync } from "node:fs";
import { parse } from "acorn";
import { ancestor, simple } from "acorn-walk";

const TOOLS = "tools";
const DISCOVERY = new Set(["searchTools", "describeTool", "describeNamespace", "ALL_TOOLS"]);
const MUTATING_METHODS = new Set(["push", "pop", "shift", "unshift", "splice", "sort", "reverse", "fill", "copyWithin", "set", "delete", "add", "clear"]);
const NONDETERMINISM = new Set(["Date", "setTimeout", "setInterval"]);

export const BLOCKERS_FOR_CALL_SET = ["dynamic-tool-access", "tools-escapes", "eval"];
export const BLOCKERS_AS_WRITTEN = ["unbounded-loop", "counted-loop", "recursion", "reassign", "property-write", "mutating-method", "try-catch", "throw", "class", "generator"];
export const BLOCKERS_WITH_REWRITING = ["unbounded-loop", "recursion", "class", "generator"];

function staticName(member) {
  if (!member.computed && member.property.type === "Identifier") return member.property.name;
  if (member.computed && member.property.type === "Literal" && typeof member.property.value === "string") return member.property.value;
  return null;
}

function isLiteral(node) {
  switch (node.type) {
    case "Literal":
      return true;
    case "TemplateLiteral":
      return node.expressions.length === 0;
    case "UnaryExpression":
      return node.operator === "-" && node.argument.type === "Literal";
    case "ArrayExpression":
      return node.elements.every((element) => element !== null && isLiteral(element));
    case "ObjectExpression":
      return node.properties.every((p) => p.type === "Property" && !p.computed && isLiteral(p.value));
    default:
      return false;
  }
}

function countedVariable(loop) {
  const init = loop.init;
  if (!init || init.type !== "VariableDeclaration" || init.declarations.length !== 1) return null;
  const counter = init.declarations[0].id;
  if (counter.type !== "Identifier") return null;
  const name = counter.name;
  const test = loop.test;
  const compares = test && test.type === "BinaryExpression" && ["<", "<=", ">", ">="].includes(test.operator);
  if (!compares || !(test.left.type === "Identifier" && test.left.name === name)) return null;
  const update = loop.update;
  const steps =
    update &&
    ((update.type === "UpdateExpression" && update.argument.type === "Identifier" && update.argument.name === name) ||
      (update.type === "AssignmentExpression" && ["+=", "-="].includes(update.operator) &&
        update.left.type === "Identifier" && update.left.name === name && update.right.type === "Literal"));
  if (!steps) return null;
  let touched = false;
  simple(loop.body, {
    AssignmentExpression(node) {
      if (node.left.type === "Identifier" && node.left.name === name) touched = true;
    },
    UpdateExpression(node) {
      if (node.argument.type === "Identifier" && node.argument.name === name) touched = true;
    },
  });
  return touched ? null : name;
}

function isToolsObject(node) {
  return node.type === "Identifier" && node.name === TOOLS;
}

export function classify(code) {
  let tree;
  try {
    tree = parse(`async function __script() {\n${code}\n}`, { ecmaVersion: "latest", sourceType: "module" });
  } catch (error) {
    return { parsed: false, error: String(error.message) };
  }
  const features = new Set();
  const calls = [];
  const library = new Set();
  ancestor(tree, {
    FunctionDeclaration(node) {
      if (node.generator) features.add("generator");
    },
    MemberExpression(node, _, ancestors) {
      if (!isToolsObject(node.object)) return;
      const parent = ancestors[ancestors.length - 2];
      if (!(parent && parent.type === "CallExpression" && parent.callee === node)) features.add("tools-escapes");
    },
    FunctionExpression(node) {
      if (node.generator) features.add("generator");
    },
    CallExpression(node, _, ancestors) {
      const callee = node.callee;
      if (callee.type === "MemberExpression" && isToolsObject(callee.object)) {
        const name = staticName(callee);
        if (name === null) features.add("dynamic-tool-access");
        else calls.push({ name, literal: node.arguments.length === 0 || isLiteral(node.arguments[0]) });
      }
      if (callee.type === "Identifier" && callee.name === "eval") features.add("eval");
      if (callee.type === "Identifier" && DISCOVERY.has(callee.name)) features.add("discovery");
      if (callee.type === "Identifier" && NONDETERMINISM.has(callee.name)) features.add("nondeterminism");
      if (callee.type === "MemberExpression" && !callee.computed && callee.property.type === "Identifier" && !isToolsObject(callee.object)) {
        const owner = callee.object.type === "Identifier" && /^[A-Z]/.test(callee.object.name) ? callee.object.name : "";
        library.add(`${owner}.${callee.property.name}`);
      }
      if (callee.type === "Identifier" && /^[A-Z]|^(parseInt|parseFloat|String|Number|Boolean|isNaN|encodeURIComponent)$/.test(callee.name)) {
        library.add(callee.name);
      }
      if (callee.type === "MemberExpression" && !callee.computed && callee.property.type === "Identifier") {
        if (MUTATING_METHODS.has(callee.property.name) && !isToolsObject(callee.object)) features.add("mutating-method");
        if (callee.object.type === "Identifier" && NONDETERMINISM.has(callee.object.name)) features.add("nondeterminism");
        if (callee.object.type === "Identifier" && callee.object.name === "Math" && callee.property.name === "random") {
          features.add("nondeterminism");
        }
      }
      if (callee.type === "Identifier") {
        const enclosing = ancestors.filter((a) => a.type === "FunctionDeclaration" && a.id && a.id.name === callee.name);
        if (enclosing.length > 0) features.add("recursion");
      }
    },
    NewExpression(node) {
      if (node.callee.type === "Identifier" && node.callee.name === "Function") features.add("eval");
      if (node.callee.type === "Identifier" && NONDETERMINISM.has(node.callee.name)) features.add("nondeterminism");
    },
    ImportExpression() {
      features.add("eval");
    },
    Identifier(node, _, ancestors) {
      if (DISCOVERY.has(node.name)) features.add("discovery");
      if (node.name !== TOOLS) return;
      const parent = ancestors[ancestors.length - 2];
      const usedAsCallee = parent && parent.type === "MemberExpression" && parent.object === node;
      if (!usedAsCallee) features.add("tools-escapes");
    },
    WhileStatement() {
      features.add("unbounded-loop");
    },
    DoWhileStatement() {
      features.add("unbounded-loop");
    },
    ForStatement(node) {
      features.add(countedVariable(node) === null ? "unbounded-loop" : "counted-loop");
    },
    AssignmentExpression(node) {
      if (node.left.type === "MemberExpression") features.add("property-write");
      else features.add("reassign");
    },
    UpdateExpression() {
      features.add("reassign");
    },
    TryStatement() {
      features.add("try-catch");
    },
    ThrowStatement() {
      features.add("throw");
    },
    ClassDeclaration() {
      features.add("class");
    },
    ClassExpression() {
      features.add("class");
    },
  });
  const has = (names) => names.some((name) => features.has(name));
  const callsTools = calls.length > 0 || features.has("dynamic-tool-access");
  const callSet = callsTools && !has(BLOCKERS_FOR_CALL_SET);
  return {
    parsed: true,
    features: [...features].sort(),
    calls,
    library: [...library].sort(),
    callsTools,
    tiers: {
      callSet,
      asWritten: callSet && !has(BLOCKERS_AS_WRITTEN),
      withRewriting: callSet && !has(BLOCKERS_WITH_REWRITING),
    },
  };
}

if (import.meta.main) {
  const [corpus] = process.argv.slice(2);
  for (const line of readFileSync(corpus, "utf8").split("\n")) {
    if (!line) continue;
    const script = JSON.parse(line);
    const { code, ...about } = script;
    console.log(JSON.stringify({ ...about, ...classify(code) }));
  }
}
