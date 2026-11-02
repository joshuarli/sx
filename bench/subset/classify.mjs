import { readFileSync } from "node:fs";
import { parse } from "acorn";

const SAFE_INTEGER = 2 ** 53;
const ITERATION_METHODS = new Set(["map", "filter", "flatMap", "find", "some", "every", "forEach"]);
const BUILTIN_METHODS = new Set([
  ...ITERATION_METHODS, "reduce", "includes", "indexOf", "join", "slice", "concat", "sort", "reverse", "push",
  "startsWith", "endsWith", "split", "trim", "toLowerCase", "toUpperCase", "padStart", "padEnd", "replace",
  "replaceAll", "toString", "sort",
]);
const FRESH_METHODS = new Set(["map", "filter", "flatMap", "slice", "concat", "split", "sort", "reverse"]);
const MUTATING_METHODS = new Set(["pop", "shift", "unshift", "splice", "fill", "copyWithin", "set", "add", "delete", "clear"]);
const MATH_FUNCTIONS = new Set(["floor", "ceil", "round", "trunc", "abs", "max", "min"]);
const REGEXP_METHODS = new Set(["match", "matchAll", "test", "exec", "search"]);
const DATE_METHODS = new Set([
  "toISOString", "getTime", "getFullYear", "getMonth", "getDate", "getDay", "getHours", "getMinutes", "getSeconds",
  "getMilliseconds", "toDateString", "toUTCString", "toJSON", "setDate", "setHours",
]);
const LOCALE_METHODS = new Set(["toLocaleString", "toLocaleDateString", "toLocaleTimeString", "localeCompare", "toLocaleUpperCase", "toLocaleLowerCase"]);
const DISCOVERY = new Set(["searchTools", "describeTool", "describeNamespace", "ALL_TOOLS"]);
const BOUNDED_BUILTINS = new Set(["retry", "loop", "parMap", "waitUntil", "sleep", "next", "done"]);
const CLASS_GLOBALS = new Set(["Set", "Map", "WeakMap", "WeakSet", "Symbol", "BigInt", "Uint8Array", "Buffer", "ArrayBuffer", "Proxy", "Reflect"]);
const NONDETERMINISTIC_GLOBALS = new Set(["setTimeout", "setInterval", "setImmediate", "clearTimeout", "clearInterval", "performance", "crypto", "queueMicrotask"]);
const ALLOWED_GLOBALS = new Set(["tools", "console", "JSON", "Math", "Object", "Promise", "String", "parseInt", "Number", "Error", "Array", "Date", "RegExp", ...BOUNDED_BUILTINS]);
const UNDEFINED_GLOBALS = new Set(["undefined"]);
const NUMBER_GLOBALS = new Set(["NaN", "Infinity"]);
const OPERATOR_BINARY = new Set(["**", "&", "|", "^", "<<", ">>", ">>>", "in"]);
const OPERATOR_ASSIGN = new Set(["**=", "&=", "|=", "^=", "<<=", ">>=", ">>>=", "||=", "&&=", "??="]);
const PLAIN_ASSIGN = new Set(["=", "+=", "-=", "*=", "/=", "%="]);
const REWRITTEN_FEATURES = ["reassign", "property-write", "mutating-method", "try-catch", "throw", "counted-loop"];
const RESIDUE = new Set(["content", "structuredContent", "isError"]);
const INTEGER_KEY = /^(0|[1-9][0-9]*)$/;

export const CODES_FOR_CALL_SET = ["ts-tools-escape", "ts-eval"];

function camel(words) {
  return words.replace(/_([a-z0-9])/g, (_, c) => c.toUpperCase());
}

export function toolPath(name) {
  const tail = name.split("__").slice(2).join("__");
  const cut = tail.indexOf("_");
  if (cut < 0) return { namespace: tail, member: tail };
  return { namespace: tail.slice(0, cut), member: camel(tail.slice(cut + 1)) };
}

function parseProgram(code) {
  return parse(`async function __script() {\n${code}\n}`, { ecmaVersion: "latest", sourceType: "module", ranges: true, locations: true });
}

function children(node) {
  const found = [];
  for (const key of Object.keys(node)) {
    if (key === "type" || key === "loc" || key === "range" || key === "start" || key === "end") continue;
    const value = node[key];
    if (Array.isArray(value)) {
      for (const item of value) if (item && typeof item.type === "string") found.push(item);
    } else if (value && typeof value.type === "string") found.push(value);
  }
  return found;
}

function text(source, node) {
  return source.slice(node.range[0], node.range[1]);
}

function isStringKey(property, name) {
  if (!property) return false;
  if (!property.computed && property.type === "Identifier") return property.name === name;
  return property.computed && property.type === "Literal" && property.value === name;
}

function isMember(node, name) {
  return node.type === "MemberExpression" && isStringKey(node.property, name);
}

export function rewriteApi(code) {
  let tree;
  try {
    tree = parseProgram(code);
  } catch {
    return code;
  }
  const prefix = "async function __script() {\n";
  const source = `${prefix}${code}\n}`;
  const declared = new Set();
  const edits = [];
  const walk = (node) => {
    if ((node.type === "FunctionDeclaration" || node.type === "ClassDeclaration") && node.id) declared.add(node.id.name);
    if (node.type === "VariableDeclarator" && node.id.type === "Identifier") declared.add(node.id.name);
    for (const child of children(node)) walk(child);
  };
  walk(tree);
  const valueOf = (node) => {
    let current = node.type === "ChainExpression" ? node.expression : node;
    if (isMember(current, "result")) current = current.object;
    return isMember(current, "structuredContent") ? current.object : node;
  };
  const contentMap = (node) => {
    if (node.type !== "CallExpression" || !isMember(node.callee, "map")) return null;
    const receiver = node.callee.object;
    if (isMember(receiver, "content")) return receiver.object;
    if (receiver.type === "LogicalExpression" && isMember(receiver.left, "content")) return receiver.left.object;
    return null;
  };
  const visit = (node, parent) => {
    if (node.type === "MemberExpression" && !node.computed && node.object.type === "Identifier" && node.object.name === "tools"
      && node.property.type === "Identifier" && node.property.name.startsWith("mcp__")) {
      const { namespace, member } = toolPath(node.property.name);
      edits.push({ range: node.range, text: `tools.${namespace}.${member}` });
    } else if (node.type === "MemberExpression" && isStringKey(node.property, "isError")) {
      edits.push({ range: node.range, text: "false" });
    } else if (node.type === "MemberExpression" && isStringKey(node.property, "structuredContent")) {
      const outer = parent && parent.type === "MemberExpression" && parent.object === node && isStringKey(parent.property, "result") ? parent : null;
      edits.push({ range: (outer ?? node).range, inner: node.object.range });
    } else if (isMember(node, "text") && node.object.type === "MemberExpression" && node.object.computed
      && node.object.property.type === "Literal" && node.object.property.value === 0 && isMember(node.object.object, "content")) {
      edits.push({ range: node.range, inner: node.object.object.object.range });
    } else if (node.type === "LogicalExpression" && (node.operator === "??" || node.operator === "||")
      && isMember(node.right, "content") && source.slice(...valueOf(node.left).range) === source.slice(...node.right.object.range)) {
      edits.push({ range: node.range, inner: valueOf(node.left).range });
    } else if (node.type === "LogicalExpression" && (node.operator === "??" || node.operator === "||")
      && isMember(node.left, "content") && source.slice(...valueOf(node.right).range) === source.slice(...node.left.object.range)) {
      edits.push({ range: node.range, inner: valueOf(node.right).range });
    } else if (node.type === "CallExpression" && isMember(node.callee, "join") && contentMap(node.callee.object)) {
      edits.push({ range: node.range, inner: contentMap(node.callee.object).range });
    } else if (node.type === "MemberExpression" && !node.computed && node.object.type === "Identifier" && node.object.name === "tools"
      && node.property.type === "Identifier" && !node.property.name.startsWith("mcp__") && parent && parent.type === "CallExpression" && parent.callee === node) {
      edits.push({ range: node.range, text: `tools.pi.${node.property.name}` });
    } else if (node.type === "CallExpression" && node.callee.type === "Identifier" && node.callee.name === "text" && !declared.has("text")) {
      edits.push({ range: node.callee.range, text: "console.log" });
    }
    for (const child of children(node)) visit(child, node);
  };
  visit(tree, null);
  const emit = (start, end) => {
    let out = "";
    let cursor = start;
    for (const edit of edits.filter((e) => e.range[0] >= start && e.range[1] <= end).sort((a, b) => a.range[0] - b.range[0] || b.range[1] - a.range[1])) {
      if (edit.range[0] < cursor) continue;
      out += source.slice(cursor, edit.range[0]);
      out += edit.text ?? emit(edit.inner[0], edit.inner[1]);
      cursor = edit.range[1];
    }
    return out + source.slice(cursor, end);
  };
  const rewritten = emit(prefix.length, source.length - 2);
  return rewritten;
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

function toolCall(node) {
  if (node.type !== "CallExpression") return null;
  const callee = node.callee;
  if (callee.type !== "MemberExpression" || callee.computed || callee.object.type !== "MemberExpression") return null;
  const space = callee.object;
  if (space.computed || space.object.type !== "Identifier" || space.object.name !== "tools") return null;
  if (space.property.type !== "Identifier" || callee.property.type !== "Identifier") return null;
  return `${space.property.name}.${callee.property.name}`;
}

class Binding {
  constructor(name, kind, fn, declarator) {
    this.name = name;
    this.kind = kind;
    this.fn = fn;
    this.declarator = declarator;
    this.refs = [];
  }
}

function containsNode(root, predicate) {
  let found = false;
  const visit = (node) => {
    if (found) return;
    if (predicate(node)) {
      found = true;
      return;
    }
    for (const child of children(node)) visit(child);
  };
  visit(root);
  return found;
}

function canonical(node, renames) {
  if (Array.isArray(node)) return `[${node.map((item) => canonical(item, renames)).join(",")}]`;
  if (node === null || typeof node !== "object") return JSON.stringify(node);
  if (node.type === "Identifier" && renames.has(node.name)) return `P(${renames.get(node.name)})`;
  const parts = [node.type];
  for (const key of Object.keys(node).sort()) {
    if (["loc", "range", "start", "end", "type", "raw"].includes(key)) continue;
    parts.push(`${key}=${canonical(node[key], renames)}`);
  }
  return `(${parts.join(" ")})`;
}

function mentions(node, name) {
  return containsNode(node, (n) => n.type === "Identifier" && n.name === name);
}

function comparatorBody(fn) {
  if (fn.body.type !== "BlockStatement") return fn.body;
  if (fn.body.body.length === 1 && fn.body.body[0].type === "ReturnStatement") return fn.body.body[0].argument;
  return null;
}

function acceptedComparator(fn) {
  if (!fn || (fn.type !== "ArrowFunctionExpression" && fn.type !== "FunctionExpression")) return "shape";
  if (fn.params.length !== 2 || fn.params.some((p) => p.type !== "Identifier")) return "shape";
  const [a, b] = fn.params.map((p) => p.name);
  const body = comparatorBody(fn);
  if (!body) return "shape";
  if (containsNode(body, (n) => n.type === "CallExpression" && n.callee.type === "MemberExpression" && isStringKey(n.callee.property, "localeCompare"))) return "locale";
  if (body.type !== "BinaryExpression" || body.operator !== "-") return "shape";
  for (const [left, right] of [[a, b], [b, a]]) {
    if (mentions(body.left, left) && !mentions(body.left, right) && mentions(body.right, right) && !mentions(body.right, left)) {
      if (canonical(body.left, new Map([[left, 0]])) === canonical(body.right, new Map([[right, 0]]))) return null;
    }
  }
  return "shape";
}

export function classify(rawCode) {
  const code = rewriteApi(rawCode);
  let tree;
  try {
    tree = parseProgram(code);
  } catch (error) {
    if (/^\s*(import|export)\b/m.test(code)) {
      return {
        parsed: true, features: [], calls: [], library: [], callsTools: /\btools\b/.test(code), codes: { "ts-import-export": 1 }, sites: [],
        tiers: { callSet: false, asWritten: false, accepted: false },
      };
    }
    return { parsed: false, error: String(error.message) };
  }
  const features = new Set();
  const codes = new Map();
  const calls = [];
  const library = new Set();
  const sites = [];
  let current = null;
  const flag = (id, node = current) => {
    codes.set(id, (codes.get(id) ?? 0) + 1);
    if (node && node.loc) sites.push({ code: id, line: node.loc.start.line - 1, text: code.split("\n")[node.loc.start.line - 2]?.trim().slice(0, 160) });
  };

  const scopes = [];
  const bindings = [];
  const fnStack = [];
  const path = [];
  const unresolved = new Map();
  const declaredFunctions = new Set();
  const asyncFunctions = new Set();
  const catches = [];
  const loops = [];
  const functionNodes = [];

  const lookup = (name) => {
    for (let i = scopes.length - 1; i >= 0; i--) if (scopes[i].has(name)) return scopes[i].get(name);
    return null;
  };

  const declarePattern = (scope, pattern, kind, declarator) => {
    if (!pattern) return;
    switch (pattern.type) {
      case "Identifier": {
        const binding = new Binding(pattern.name, kind, fnStack[fnStack.length - 1] ?? null, declarator);
        scope.set(pattern.name, binding);
        bindings.push(binding);
        return;
      }
      case "ObjectPattern":
        for (const property of pattern.properties) declarePattern(scope, property.type === "RestElement" ? property.argument : property.value, kind, declarator);
        return;
      case "ArrayPattern":
        for (const element of pattern.elements) declarePattern(scope, element, kind, declarator);
        return;
      case "AssignmentPattern":
        declarePattern(scope, pattern.left, kind, declarator);
        return;
      case "RestElement":
        declarePattern(scope, pattern.argument, kind, declarator);
        return;
      default:
    }
  };

  const hoist = (scope, statements) => {
    for (const statement of statements) {
      if (statement.type === "VariableDeclaration") {
        for (const declarator of statement.declarations) declarePattern(scope, declarator.id, statement.kind, declarator);
      } else if (statement.type === "ClassDeclaration" && statement.id) {
        const binding = new Binding(statement.id.name, "class", fnStack[fnStack.length - 1] ?? null, statement);
        scope.set(statement.id.name, binding);
        bindings.push(binding);
      } else if (statement.type === "FunctionDeclaration" && statement.id) {
        const binding = new Binding(statement.id.name, "function", fnStack[fnStack.length - 1] ?? null, statement);
        scope.set(statement.id.name, binding);
        bindings.push(binding);
        declaredFunctions.add(statement.id.name);
        if (statement.async) asyncFunctions.add(statement.id.name);
      }
    }
  };

  const isFunctionNode = (node) =>
    node.type === "FunctionDeclaration" || node.type === "FunctionExpression" || node.type === "ArrowFunctionExpression";

  const parentOf = (offset = 1) => path[path.length - 1 - offset];

  const visit = (node) => {
    const parent = path[path.length - 1] ?? null;
    path.push(node);
    current = node;
    enter(node, parent);
    for (const child of children(node)) visit(child);
    leave(node);
    path.pop();
  };

  const enter = (node, parent) => {
    switch (node.type) {
      case "FunctionDeclaration":
      case "FunctionExpression":
      case "ArrowFunctionExpression": {
        functionNodes.push({ node, path: path.slice() });
        if (node.generator) flag("ts-generator");
        const scope = new Map();
        scopes.push(scope);
        fnStack.push(node);
        if (node.id && node.type === "FunctionExpression") declarePattern(scope, node.id, "function", node);
        for (const param of node.params) declarePattern(scope, param, "param", node);
        if (node.body.type === "BlockStatement") hoist(scope, node.body.body);
        if (node.type === "ArrowFunctionExpression" && parent && parent.type === "VariableDeclarator" && parent.init === node && parent.id.type === "Identifier") {
          declaredFunctions.add(parent.id.name);
          if (node.async) asyncFunctions.add(parent.id.name);
        }
        break;
      }
      case "BlockStatement": {
        if (!isFunctionNode(parent)) {
          const scope = new Map();
          scopes.push(scope);
          hoist(scope, node.body);
        }
        break;
      }
      case "ForStatement":
      case "ForOfStatement":
      case "ForInStatement": {
        const scope = new Map();
        scopes.push(scope);
        const head = node.type === "ForStatement" ? node.init : node.left;
        if (head && head.type === "VariableDeclaration") for (const d of head.declarations) declarePattern(scope, d.id, head.kind, d);
        loops.push(node);
        break;
      }
      case "WhileStatement":
      case "DoWhileStatement":
        loops.push(node);
        break;
      case "CatchClause": {
        const scope = new Map();
        scopes.push(scope);
        if (node.param) declarePattern(scope, node.param, "catch", node);
        catches.push({ clause: node, path: path.slice() });
        break;
      }
      case "Identifier":
        reference(node, parent);
        break;
      default:
    }
    check(node, parent);
  };

  const leave = (node) => {
    switch (node.type) {
      case "FunctionDeclaration":
      case "FunctionExpression":
      case "ArrowFunctionExpression":
        scopes.pop();
        fnStack.pop();
        break;
      case "BlockStatement":
        if (!isFunctionNode(path[path.length - 2])) scopes.pop();
        break;
      case "ForStatement":
      case "ForOfStatement":
      case "ForInStatement":
        scopes.pop();
        loops.pop();
        break;
      case "WhileStatement":
      case "DoWhileStatement":
        loops.pop();
        break;
      case "CatchClause":
        scopes.pop();
        break;
      default:
    }
  };

  const isBindingPosition = (node, parent) => {
    if (!parent) return false;
    if (parent.type === "MemberExpression" && parent.property === node && !parent.computed) return true;
    if (parent.type === "Property" && parent.key === node && !parent.computed && parent.value !== node) return true;
    if (parent.type === "VariableDeclarator" && parent.id === node) return true;
    if (isFunctionNode(parent) && (parent.id === node || parent.params.includes(node))) return true;
    if (parent.type === "CatchClause" && parent.param === node) return true;
    if (parent.type === "LabeledStatement" || parent.type === "BreakStatement" || parent.type === "ContinueStatement") return true;
    return false;
  };

  const reference = (node, parent) => {
    if (isBindingPosition(node, parent)) return;
    const inPattern = path.slice(0, -1).reverse().find((n) => ["ObjectPattern", "ArrayPattern", "AssignmentPattern", "RestElement"].includes(n.type));
    if (inPattern && patternPosition(node)) return;
    const binding = lookup(node.name);
    if (binding) {
      binding.refs.push({ node, path: path.slice(), fns: fnStack.slice(), loops: loops.slice() });
      return;
    }
    if (!unresolved.has(node.name)) unresolved.set(node.name, []);
    unresolved.get(node.name).push({ node, path: path.slice() });
  };

  const patternPosition = (node) => {
    for (let i = path.length - 2; i >= 0; i--) {
      const ancestor = path[i];
      const child = path[i + 1];
      if (ancestor.type === "VariableDeclarator") return ancestor.id === child;
      if (isFunctionNode(ancestor)) return ancestor.params.includes(child);
      if (ancestor.type === "AssignmentPattern" && ancestor.right === child) return false;
      if (ancestor.type === "Property" && ancestor.key === child && !ancestor.computed && ancestor.value !== child) return true;
      if (ancestor.type === "ForOfStatement" || ancestor.type === "ForInStatement") return ancestor.left === child;
      if (ancestor.type === "CatchClause") return ancestor.param === child;
      if (ancestor.type === "AssignmentExpression") return ancestor.left === child && false;
    }
    return false;
  };

  const isInlineCallback = (fn, fnPath) => {
    const parent = fnPath[fnPath.length - 2];
    if (!parent || parent.type !== "CallExpression" || !parent.arguments.includes(fn)) return false;
    const callee = parent.callee;
    if (callee.type === "MemberExpression" && !callee.computed && callee.property.type === "Identifier") {
      return BUILTIN_METHODS.has(callee.property.name);
    }
    return callee.type === "Identifier" && BOUNDED_BUILTINS.has(callee.name);
  };

  const check = (node, parent) => {
    switch (node.type) {
      case "ImportDeclaration":
      case "ExportNamedDeclaration":
      case "ExportDefaultDeclaration":
      case "ExportAllDeclaration":
      case "ImportExpression":
        flag("ts-import-export");
        if (node.type === "ImportExpression") flag("ts-eval");
        break;
      case "ClassDeclaration":
      case "ClassExpression":
      case "ThisExpression":
        flag("ts-class");
        break;
      case "MetaProperty":
      case "TaggedTemplateExpression":
        flag("ts-unsupported");
        break;
      case "YieldExpression":
        flag("ts-generator");
        break;
      case "WhileStatement":
      case "DoWhileStatement":
        features.add("unbounded-loop");
        flag("ts-while");
        break;
      case "ForStatement":
        checkCountedFor(node);
        break;
      case "ForInStatement":
        flag("ts-for-in");
        break;
      case "ForOfStatement":
        if (node.await) flag("ts-generator");
        break;
      case "SwitchStatement":
        flag("ts-switch");
        break;
      case "LabeledStatement":
      case "WithStatement":
      case "DebuggerStatement":
        flag("ts-label");
        break;
      case "TryStatement":
        features.add("try-catch");
        if (node.finalizer) flag("ts-finally");
        break;
      case "ThrowStatement":
        features.add("throw");
        checkThrow(node);
        break;
      case "VariableDeclaration":
        if (node.kind === "var") flag("ts-unsupported");
        for (const d of node.declarations) {
          if (!d.init && !(parent && (parent.type === "ForOfStatement" || parent.type === "ForInStatement"))
            && !(d.id.type === "Identifier" && definitelyAssigned(node, parent, d.id.name))) flag("ts-uninitialized");
        }
        break;
      case "AssignmentPattern":
      case "RestElement":
        flag("ts-param");
        break;
      case "SequenceExpression":
        flag("ts-operator");
        break;
      case "TemplateLiteral":
        break;
      case "Literal":
        checkLiteral(node);
        break;
      case "UnaryExpression":
        if (node.operator === "typeof") checkTypeof(node, parent);
        else if (node.operator === "~" || node.operator === "delete") flag("ts-operator");
        else if (node.operator === "void") flag("ts-undefined");
        break;
      case "BinaryExpression":
        checkBinary(node);
        break;
      case "ObjectExpression":
        checkObject(node);
        break;
      case "SpreadElement":
        if (parent && (parent.type === "CallExpression" || parent.type === "NewExpression")) flag("ts-spread");
        break;
      case "ChainExpression":
        checkChain(node, parent);
        break;
      case "CallExpression":
        checkCall(node, parent);
        break;
      case "NewExpression":
        checkNew(node, parent);
        break;
      case "AwaitExpression":
        checkAwait(node);
        break;
      case "MemberExpression":
        checkMember(node, parent);
        break;
      case "UpdateExpression":
        features.add("reassign");
        break;
      case "AssignmentExpression":
        features.add(node.left.type === "MemberExpression" ? "property-write" : "reassign");
        if (OPERATOR_ASSIGN.has(node.operator)) flag("ts-operator");
        break;
      case "FunctionDeclaration":
      case "FunctionExpression":
      case "ArrowFunctionExpression":
        break;
      default:
    }
  };

  const exits = (statement) => ["ReturnStatement", "ThrowStatement", "BreakStatement", "ContinueStatement"].includes(statement.type);

  const assignsAll = (statement, name) => {
    switch (statement.type) {
      case "ExpressionStatement": {
        const e = statement.expression;
        return e.type === "AssignmentExpression" && e.operator === "=" && e.left.type === "Identifier" && e.left.name === name
          && !mentions(e.right, name);
      }
      case "BlockStatement":
        for (const inner of statement.body) {
          if (exits(inner) || assignsAll(inner, name)) return true;
        }
        return false;
      case "IfStatement":
        return Boolean(statement.alternate) && assignsAll(statement.consequent, name) && assignsAll(statement.alternate, name);
      case "TryStatement":
        return assignsAll(statement.block, name) && (!statement.handler || assignsAll(statement.handler.body, name));
      default:
        return exits(statement);
    }
  };

  const definitelyAssigned = (declaration, container, name) => {
    const list = container && Array.isArray(container.body) ? container.body : null;
    if (!list) return false;
    const at = list.indexOf(declaration);
    for (const statement of list.slice(at + 1)) {
      if (!mentions(statement, name)) continue;
      return assignsAll(statement, name);
    }
    return true;
  };

  const checkLiteral = (node) => {
    if (node.regex) flag("ts-regexp");
    if (node.bigint) flag("ts-unsupported-class");
    if (typeof node.value === "number" && /^[0-9_xXoObBa-fA-F]+$/.test(node.raw ?? "") && !/^0[xX]/.test(node.raw) === !/[a-fA-F]/.test(node.raw)) {
      try {
        if (BigInt(node.raw.replace(/_/g, "")) > BigInt(SAFE_INTEGER)) flag("ts-unsafe-integer");
      } catch {
        return;
      }
    }
  };

  const checkBinary = (node) => {
    const op = node.operator;
    if (OPERATOR_BINARY.has(op)) flag("ts-operator");
    else if (op === "instanceof") flag("ts-instanceof");
    else if ((op === "===" || op === "!==") && [node.left, node.right].some((side) => side.type === "Literal" && side.value === "undefined")) flag("ts-undefined");
    else if (op === "==" || op === "!=") {
      const isNull = (side) => side.type === "Literal" && side.value === null && side.raw === "null";
      if (!isNull(node.left) && !isNull(node.right)) flag("ts-loose-equality");
    }
  };

  const checkTypeof = (node, parent) => {
    if (node.argument.type === "ChainExpression") flag("ts-optional-chain");
    if (parent && parent.type === "BinaryExpression" && [parent.left, parent.right].some((side) => side.type === "Literal" && side.value === "undefined")) flag("ts-undefined");
  };

  const checkObject = (node) => {
    const seen = new Set();
    for (const property of node.properties) {
      if (property.type === "SpreadElement") {
        flag("ts-spread");
        continue;
      }
      if (property.kind !== "init" || property.method) flag("ts-class");
      if (property.computed) {
        flag("ts-key");
        continue;
      }
      const key = property.key.type === "Identifier" ? property.key.name : String(property.key.value);
      if (INTEGER_KEY.test(key) || seen.has(key)) flag("ts-key");
      seen.add(key);
    }
  };

  const checkNew = (node, parent) => {
    const callee = node.callee;
    if (callee.type === "Identifier" && callee.name === "Error") {
      if (!(parent && parent.type === "ThrowStatement")) flag("ts-class");
    } else if (callee.type === "Identifier" && callee.name === "Date") flag("ts-date");
    else if (callee.type === "Identifier" && callee.name === "Function") flag("ts-eval");
    else if (callee.type === "Identifier" && callee.name === "RegExp") flag("ts-regexp");
    else flag("ts-unsupported-class");
  };

  const checkThrow = (node) => {
    const argument = node.argument;
    if (argument.type === "NewExpression" && argument.callee.type === "Identifier" && argument.callee.name === "Error" && argument.arguments.length === 1
      && argument.arguments[0].type !== "SpreadElement") return;
    if (argument.type === "Identifier") {
      const handler = [...path].reverse().find((n) => n.type === "CatchClause");
      if (handler && handler.param && handler.param.type === "Identifier" && handler.param.name === argument.name) return;
    }
    flag("ts-throw");
  };

  const checkCountedFor = (node) => {
    features.add("counted-loop");
    const { init, test, update } = node;
    if (!init || !test || !update) {
      features.add("unbounded-loop");
      flag("ts-while");
      return;
    }
    const reject = () => flag("ts-counted-loop");
    if (init.type !== "VariableDeclaration" || init.kind !== "let" || init.declarations.length !== 1) return reject();
    const declarator = init.declarations[0];
    if (declarator.id.type !== "Identifier" || !declarator.init) return reject();
    const name = declarator.id.name;
    if (declarator.init.type === "Literal" && typeof declarator.init.value === "number" && /[.eE]/.test(declarator.init.raw)) return reject();
    if (test.type !== "BinaryExpression" || !["<", "<=", ">", ">="].includes(test.operator)) return reject();
    if (!(test.left.type === "Identifier" && test.left.name === name)) return reject();
    const ascending = test.operator === "<" || test.operator === "<=";
    let stepOk = false;
    if (update.type === "UpdateExpression" && update.argument.type === "Identifier" && update.argument.name === name) {
      stepOk = ascending ? update.operator === "++" : update.operator === "--";
    } else if (update.type === "AssignmentExpression" && update.left.type === "Identifier" && update.left.name === name
      && update.right.type === "Literal" && Number.isInteger(update.right.value) && update.right.value > 0 && !/[.eE]/.test(update.right.raw)) {
      stepOk = ascending ? update.operator === "+=" : update.operator === "-=";
    }
    if (!stepOk) return reject();
    let touched = false;
    const roots = new Set();
    const collectRoots = (n) => {
      if (n.type === "Identifier") roots.add(n.name);
      if (n.type === "CallExpression") touched = true;
      for (const child of children(n)) collectRoots(child);
    };
    collectRoots(test.right);
    if (touched) return reject();
    const assigns = (name2) => containsNode(node.body, (n) => {
      const targetRoot = (target) => {
        let t = target;
        while (t.type === "MemberExpression") t = t.object;
        return t.type === "Identifier" && t.name === name2;
      };
      if (n.type === "AssignmentExpression") return targetRoot(n.left);
      if (n.type === "UpdateExpression") return targetRoot(n.argument);
      if (n.type === "CallExpression" && n.callee.type === "MemberExpression" && targetRoot(n.callee.object)
        && n.callee.property.type === "Identifier" && (MUTATING_METHODS.has(n.callee.property.name) || n.callee.property.name === "push" || n.callee.property.name === "sort" || n.callee.property.name === "reverse")) return true;
      return false;
    });
    if (assigns(name)) return reject();
    for (const root of roots) if (root !== name && assigns(root)) return reject();
    if (roots.has(name)) return reject();
    return undefined;
  };

  const chainOk = (chain, parent) => {
    if (!parent) return false;
    switch (parent.type) {
      case "LogicalExpression":
        if (parent.operator === "??") return parent.left === chain;
        if (parent.operator === "||") return parent.left === chain;
        return parent.left === chain && inTest(parent);
      case "BinaryExpression": {
        const other = parent.left === chain ? parent.right : parent.left;
        if (parent.operator === "==" || parent.operator === "!=") return isNullLiteral(other);
        if (parent.operator === "===" || parent.operator === "!==") return other.type === "Literal" && other.value !== null && other.value !== undefined && !other.regex;
        return false;
      }
      case "VariableDeclarator":
        return parent.init === chain && parent.id.type === "Identifier" && boundChainUsesOk(parent.id.name, parent);
      case "Property": {
        const object = path[path.indexOf(chain) - 2];
        const call = path[path.indexOf(chain) - 3];
        return parent.value === chain && object && object.type === "ObjectExpression" && call && call.type === "CallExpression" && toolCall(call) !== null && call.arguments[0] === object;
      }
      case "IfStatement":
      case "ConditionalExpression":
        return parent.test === chain;
      case "UnaryExpression":
        return parent.operator === "!";
      default:
        return false;
    }
  };

  const boundChainUsesOk = (name, declarator) => {
    const scopeRoot = [...path].reverse().find((n) => n.type === "BlockStatement" || n.type === "Program") ?? path[0];
    let ok = true;
    const visitUse = (node, ancestors) => {
      if (node.type === "Identifier" && node.name === name && node !== declarator.id) {
        const parent = ancestors[ancestors.length - 1];
        const isBinding = parent && ((parent.type === "MemberExpression" && parent.property === node && !parent.computed) || (parent.type === "Property" && parent.key === node && !parent.computed && parent.value !== node));
        if (!isBinding && !useOk(node, ancestors)) ok = false;
      }
      for (const child of children(node)) visitUse(child, [...ancestors, node]);
    };
    const useOk = (node, ancestors) => {
      const parent = ancestors[ancestors.length - 1];
      const grand = ancestors[ancestors.length - 2];
      if (!parent) return false;
      if (parent.type === "LogicalExpression") return parent.operator === "??" ? parent.left === node : parent.operator === "||" ? parent.left === node : parent.left === node && grand && (grand.type === "IfStatement" || grand.type === "ConditionalExpression") && grand.test === parent;
      if (parent.type === "BinaryExpression") {
        const other = parent.left === node ? parent.right : parent.left;
        if (parent.operator === "==" || parent.operator === "!=") return isNullLiteral(other);
        if (parent.operator === "===" || parent.operator === "!==") return other.type === "Literal" && other.value !== null && !other.regex;
        return false;
      }
      if (parent.type === "IfStatement" || parent.type === "ConditionalExpression") return parent.test === node;
      if (parent.type === "UnaryExpression") return parent.operator === "!";
      if (parent.type === "Property") {
        const call = ancestors[ancestors.length - 3];
        return grand && grand.type === "ObjectExpression" && call && call.type === "CallExpression" && toolCall(call) !== null && call.arguments[0] === grand;
      }
      return false;
    };
    visitUse(scopeRoot, []);
    return ok;
  };

  const isNullLiteral = (n) => n.type === "Literal" && n.value === null && n.raw === "null";

  const inTest = (logical) => {
    const index = path.indexOf(logical);
    const above = path[index - 1];
    if (!above) return false;
    if ((above.type === "IfStatement" || above.type === "ConditionalExpression") && above.test === logical) return true;
    if (above.type === "UnaryExpression" && above.operator === "!") return true;
    if (above.type === "LogicalExpression") return inTest(above);
    return false;
  };

  const checkChain = (node, parent) => {
    if (!chainOk(node, parent)) flag("ts-optional-chain");
  };

  const checkMember = (node, parent) => {
    const property = !node.computed && node.property.type === "Identifier" ? node.property.name : null;
    if (property !== null && RESIDUE.has(property)) flag("ts-codemode-residue");
    if (node.object.type === "Identifier" && node.object.name === "Number" && property === "isInteger") return;
    if (node.object.type === "Identifier" && ["Number"].includes(node.object.name) && property !== null && parent && !(parent.type === "CallExpression" && parent.callee === node)) {
      flag("ts-unknown-builtin");
    }
    if (node.object.type === "Identifier" && node.object.name === "tools") {
      const up = parent;
      const okShape = !node.computed && up && up.type === "MemberExpression" && up.object === node && !up.computed;
      if (!okShape) flag("ts-tools-escape");
    } else if (node.object.type === "MemberExpression" && node.object.object.type === "Identifier" && node.object.object.name === "tools") {
      const call = parent && parent.type === "CallExpression" && parent.callee === node;
      if (node.computed || !call) flag("ts-tools-escape");
    }
  };

  const awaitedOrParallel = (call) => {
    const index = path.indexOf(call);
    const above = path[index - 1];
    if (above && above.type === "AwaitExpression") return true;
    let child = call;
    for (let i = index - 1; i >= 0; i--) {
      const ancestor = path[i];
      if (ancestor.type === "ArrayExpression" || ancestor.type === "SpreadElement") {
        child = ancestor;
        continue;
      }
      if (ancestor.type === "ArrowFunctionExpression" && ancestor.body === child) {
        child = ancestor;
        continue;
      }
      if (ancestor.type === "BlockStatement" || ancestor.type === "ReturnStatement") {
        child = ancestor;
        continue;
      }
      if (ancestor.type === "CallExpression" && ancestor.arguments.includes(child)) {
        if (isPromiseAll(ancestor)) return true;
        if (ancestor.callee.type === "Identifier" && BOUNDED_BUILTINS.has(ancestor.callee.name)) return true;
        if (ancestor.callee.type === "MemberExpression" && isStringKey(ancestor.callee.property, "map")) {
          child = ancestor;
          continue;
        }
        return false;
      }
      return false;
    }
    return false;
  };

  const isPromiseAll = (call) =>
    call.callee.type === "MemberExpression" && call.callee.object.type === "Identifier" && call.callee.object.name === "Promise"
    && (isStringKey(call.callee.property, "all") || isStringKey(call.callee.property, "allSettled"));

  const checkAwait = (node) => {
    const argument = node.argument;
    if (argument.type !== "CallExpression") {
      if (!(argument.type === "AwaitExpression")) flag("ts-promise-value");
      return;
    }
    if (argument.callee.type === "ArrowFunctionExpression" || argument.callee.type === "FunctionExpression") flag("ts-function-value");
  };

  const checkCall = (node, parent) => {
    const callee = node.callee;
    const tool = toolCall(node);
    if (tool !== null) {
      calls.push({ name: tool, literal: node.arguments.length === 0 || isLiteral(node.arguments[0]) });
      if (!awaitedOrParallel(node)) flag("ts-promise-value");
      return;
    }
    if (callee.type === "ArrowFunctionExpression" || callee.type === "FunctionExpression") {
      flag("ts-function-value");
      return;
    }
    if (callee.type === "Identifier") {
      const name = callee.name;
      library.add(name);
      if (DISCOVERY.has(name)) flag("ts-tools-escape");
      else if (name === "eval" || name === "Function") flag("ts-eval");
      else if (name === "require") flag("ts-import-export");
      else if (name === "RegExp") flag("ts-regexp");
      else if (NONDETERMINISTIC_GLOBALS.has(name)) flag("ts-nondeterministic");
      else if (name === "String") {
        if (node.arguments.length !== 1) flag("ts-unknown-builtin");
      } else if (name === "parseInt") {
        const radix = node.arguments[1];
        if (!(node.arguments.length === 2 && radix.type === "Literal" && radix.value === 10)) flag("ts-unknown-builtin");
      } else if (BOUNDED_BUILTINS.has(name) && !lookup(name)) {
        checkBounded(node, name);
      } else if (!lookup(name) && !declaredFunctions.has(name)) {
        if (name === "Number") flag("ts-unknown-builtin");
        else flag("ts-unknown-builtin");
      } else if (declaredFunctions.has(name) && asyncFunctions.has(name) && !awaitedOrParallel(node)) {
        flag("ts-promise-value");
      } else if (!declaredFunctions.has(name)) {
        flag("ts-function-value");
      }
      return;
    }
    if (callee.type !== "MemberExpression") {
      flag("ts-unknown-builtin");
      return;
    }
    const method = !callee.computed && callee.property.type === "Identifier" ? callee.property.name : callee.computed && callee.property.type === "Literal" ? String(callee.property.value) : null;
    const objectName = callee.object.type === "Identifier" ? callee.object.name : null;
    library.add(`${objectName && /^[A-Z]/.test(objectName) ? objectName : ""}.${method}`);
    if (method === null) {
      flag("ts-unknown-builtin");
      return;
    }
    if (objectName === "console") {
      if (method !== "log") flag("ts-console");
      else if (inParallelBranch()) flag("ts-log-in-parallel");
      return;
    }
    if (objectName === "JSON") {
      if (method === "stringify") {
        const extra = node.arguments.slice(1);
        const okIndent = extra.length === 0 || (extra.length === 2 && isNullLiteral(extra[0]) && extra[1].type === "Literal" && typeof extra[1].value === "number" && Number.isInteger(extra[1].value));
        if (!okIndent) flag("ts-json");
      } else if (method === "parse") {
        flag("ts-json-parse-type");
      } else flag("ts-unknown-builtin");
      return;
    }
    if (objectName === "Math") {
      if (method === "random") flag("ts-nondeterministic");
      else if (!MATH_FUNCTIONS.has(method)) flag("ts-unknown-builtin");
      else if (node.arguments.some((a) => a.type === "SpreadElement")) flag("ts-spread");
      return;
    }
    if (objectName === "Date") {
      flag(method === "now" ? "ts-nondeterministic" : "ts-date");
      return;
    }
    if (objectName === "Number") {
      if (method !== "isInteger") flag("ts-unknown-builtin");
      return;
    }
    if (objectName === "Promise") {
      if (!isPromiseAll(node)) flag("ts-promise-value");
      return;
    }
    if (objectName === "Object") {
      if (method === "fromEntries") return;
      if (method === "assign") flag("ts-mutating-method");
      else if (method === "keys") checkKeys(node);
      else if (method === "entries" || method === "values") flag("ts-map-order");
      else flag("ts-unknown-builtin");
      return;
    }
    if (objectName === "Array") {
      if (method !== "isArray") flag("ts-unknown-builtin");
      return;
    }
    if (REGEXP_METHODS.has(method)) {
      flag("ts-regexp");
      return;
    }
    if (DATE_METHODS.has(method)) {
      flag("ts-date");
      return;
    }
    if (LOCALE_METHODS.has(method)) {
      flag("ts-locale");
      return;
    }
    if (MUTATING_METHODS.has(method)) {
      features.add("mutating-method");
      if (method !== "set" && method !== "add" && method !== "delete" && method !== "clear") flag("ts-mutating-method");
      else flag("ts-unsupported-class");
      return;
    }
    if (!BUILTIN_METHODS.has(method)) {
      flag("ts-unknown-builtin");
      return;
    }
    checkBuiltinMethod(node, method);
  };

  const checkKeys = (node) => {
    const index = path.indexOf(node);
    const above = path[index - 1];
    const sortedOrCounted = above && above.type === "MemberExpression" && above.object === node && !above.computed
      && above.property.type === "Identifier" && (above.property.name === "length" || above.property.name === "sort");
    if (!sortedOrCounted) {
      flag("ts-map-order");
      return;
    }
    if (above.property.name === "sort") {
      const call = path[index - 2];
      if (!(call && call.type === "CallExpression" && call.callee === above && call.arguments.length === 0)) flag("ts-map-order");
    }
  };

  const inParallelBranch = () => {
    for (let i = path.length - 1; i >= 0; i--) {
      const ancestor = path[i];
      if (!isFunctionNode(ancestor)) continue;
      const call = path[i - 1];
      if (call && call.type === "CallExpression" && call.arguments.includes(ancestor)) {
        if (call.callee.type === "Identifier" && call.callee.name === "parMap") return true;
        if (call.callee.type === "MemberExpression" && isStringKey(call.callee.property, "map")) {
          const outer = path[i - 2];
          if (outer && outer.type === "CallExpression" && isPromiseAll(outer)) return true;
          if (outer && outer.type === "ArrayExpression" && path[i - 3] && path[i - 3].type === "CallExpression" && isPromiseAll(path[i - 3])) return true;
        }
      }
      const outer = path[i - 1];
      const outerCall = path[i - 2];
      if (outer && outer.type === "CallExpression" && outer.callee === ancestor && outerCall) {
        const array = outerCall.type === "ArrayExpression" ? outerCall : null;
        if (array && path[i - 3] && path[i - 3].type === "CallExpression" && isPromiseAll(path[i - 3])) return true;
      }
    }
    return false;
  };

  const checkBuiltinMethod = (node, method) => {
    const fnArgument = node.arguments[0];
    const callback = fnArgument && (fnArgument.type === "ArrowFunctionExpression" || fnArgument.type === "FunctionExpression") ? fnArgument : null;
    if (ITERATION_METHODS.has(method) && callback && callback.params.length > 2) flag("ts-callback-arity");
    if (method === "reduce") {
      if (node.arguments.length < 2) flag("ts-reduce-init");
      else if (callback && callback.params.length > 2) flag("ts-callback-arity");
    }
    if (method === "sort" && node.arguments.length > 0) {
      const verdict = acceptedComparator(fnArgument);
      if (verdict === "locale") flag("ts-locale");
      else if (verdict !== null) flag("ts-sort-comparator");
    }
    if (method === "sort" || method === "reverse") features.add("mutating-method");
    if (method === "push") features.add("mutating-method");
    if (method === "toString" && node.arguments.length > 0) flag("ts-unknown-builtin");
    if (["startsWith", "endsWith", "includes", "indexOf"].includes(method) && node.arguments.length !== 1) flag("ts-unknown-builtin");
    if (method === "split" && (node.arguments.length !== 1 || node.arguments[0].type !== "Literal" || node.arguments[0].value === "")) {
      if (!(node.arguments.length === 1 && node.arguments[0].type !== "Literal")) flag("ts-unknown-builtin");
    }
    if (["replace", "replaceAll"].includes(method) && node.arguments.length !== 2) flag("ts-unknown-builtin");
    if (method === "join" && node.arguments.length !== 1) flag("ts-unknown-builtin");
    if ((method === "padStart" || method === "padEnd") && node.arguments.length !== 2) flag("ts-unknown-builtin");
    if (node.arguments.some((a) => a.type === "SpreadElement")) flag("ts-spread");
    if (callback === null && fnArgument && ITERATION_METHODS.has(method) && fnArgument.type !== "Identifier") flag("ts-function-value");
  };

  const checkBounded = (node, name) => {
    if (name === "sleep" || name === "next" || name === "done") return;
    const options = node.arguments[0];
    const literalOptions = options && options.type === "ObjectExpression"
      && options.properties.every((p) => p.type === "Property" && isLiteral(p.value));
    if (!literalOptions) flag("ts-bound-not-literal");
  };

  const analyze = () => {
    for (const [name, refs] of unresolved) {
      if (ALLOWED_GLOBALS.has(name)) {
        if (name === "tools") {
          for (const ref of refs) {
            const parent = ref.path[ref.path.length - 2];
            const grand = ref.path[ref.path.length - 3];
            const okShape = parent && parent.type === "MemberExpression" && parent.object === ref.node && !parent.computed
              && grand && grand.type === "MemberExpression" && grand.object === parent && !grand.computed;
            if (!okShape) flag("ts-tools-escape");
          }
        }
        continue;
      }
      if (name === "text") continue;
      if (DISCOVERY.has(name)) flag("ts-tools-escape");
      else if (CLASS_GLOBALS.has(name)) flag("ts-unsupported-class");
      else if (NONDETERMINISTIC_GLOBALS.has(name)) flag("ts-nondeterministic");
      else if (UNDEFINED_GLOBALS.has(name)) flag("ts-undefined");
      else if (NUMBER_GLOBALS.has(name) || name === "isNaN" || name === "isFinite" || name === "parseFloat") flag("ts-unknown-builtin");
      else if (name === "require" || name === "module" || name === "exports") flag("ts-import-export");
      else if (name === "eval") flag("ts-eval");
      else if (name === "Intl") flag("ts-locale");
      else if (name === "arguments") flag("ts-class");
      else flag("ts-unknown-builtin");
    }
    analyzeFunctions();
    analyzeMutation();
    analyzeCatches();
    analyzeSettled();
    analyzeRecursion();
  };

  const analyzeFunctions = () => {
    for (const { node, path: fnPath } of functionNodes) {
      const parent = fnPath[fnPath.length - 2];
      if (node.type === "FunctionDeclaration") continue;
      if (parent && parent.type === "VariableDeclarator" && parent.init === node) continue;
      if (parent && parent.type === "CallExpression" && parent.callee === node) continue;
      if (isInlineCallback(node, fnPath)) continue;
      if (parent && parent.type === "CallExpression" && parent.arguments.includes(node) && parent.callee.type === "Identifier" && declaredFunctions.has(parent.callee.name)) {
        flag("ts-function-value");
        continue;
      }
      flag("ts-function-value");
    }
  };

  const FRESH_GLOBAL_CALLS = new Set(["JSON.parse", "Object.fromEntries", "Promise.all", "Promise.allSettled"]);

  const isFresh = (expr) => {
    if (!expr) return false;
    switch (expr.type) {
      case "ArrayExpression":
      case "ObjectExpression":
        return true;
      case "AwaitExpression":
        return isFresh(expr.argument);
      case "CallExpression": {
        if (toolCall(expr) !== null) return true;
        const callee = expr.callee;
        if (callee.type !== "MemberExpression" || callee.computed || callee.property.type !== "Identifier") return false;
        if (callee.object.type === "Identifier" && FRESH_GLOBAL_CALLS.has(`${callee.object.name}.${callee.property.name}`)) return true;
        if (FRESH_METHODS.has(callee.property.name)) return !(callee.property.name === "sort" || callee.property.name === "reverse") || isFresh(callee.object);
        return false;
      }
      case "ConditionalExpression":
        return isFresh(expr.consequent) && isFresh(expr.alternate);
      default:
        return false;
    }
  };

  const rootIdentifier = (expr) => {
    let current = expr;
    while (current && current.type === "MemberExpression") current = current.object;
    return current && current.type === "Identifier" ? current : null;
  };

  const analyzeMutation = () => {
    const throwingFunctions = throwingFunctionSet();
    for (const binding of bindings) {
      if (binding.kind === "function" || binding.kind === "catch") continue;
      const owned = binding.declarator && binding.declarator.type === "VariableDeclarator" && isFresh(binding.declarator.init);
      const mutations = [];
      for (const ref of binding.refs) {
        const site = mutationSite(ref, owned);
        if (site) mutations.push({ ...site, ref });
      }
      if (mutations.length === 0) continue;
      for (const mutation of mutations) {
        if (mutation.bad) flag(mutation.bad);
      }
      for (const ref of binding.refs) {
        const closureIssue = closureEscape(binding, ref, mutations);
        if (closureIssue) flag("ts-mutation-escape");
      }
      if (owned) {
        for (const ref of binding.refs) {
          if (unsafeStore(binding, ref, mutations)) flag("ts-mutation-escape");
        }
      }
      for (const mutation of mutations) {
        if (lostInTry(binding, mutation, throwingFunctions)) flag("ts-mutation-in-try");
      }
    }
  };

  const mutationSite = (ref, owned) => {
    const idx = ref.path.length - 1;
    const node = ref.node;
    const parent = ref.path[idx - 1];
    const asStatement = (stmtNode) => {
      const above = ref.path[ref.path.indexOf(stmtNode) - 1];
      return above && (above.type === "ExpressionStatement" || (above.type === "ForStatement" && above.update === stmtNode));
    };
    let target = node;
    let up = parent;
    let depth = 0;
    while (up && up.type === "MemberExpression" && up.object === target) {
      target = up;
      up = ref.path[ref.path.indexOf(target) - 1];
      depth++;
    }
    if (up && up.type === "AssignmentExpression" && up.left === target) {
      if (!PLAIN_ASSIGN.has(up.operator) && !OPERATOR_ASSIGN.has(up.operator)) return { bad: "ts-operator" };
      if (OPERATOR_ASSIGN.has(up.operator)) return { bad: null };
      if (!asStatement(up)) return { bad: "ts-mutation-pattern" };
      if (depth > 0 && !owned) return { bad: "ts-mutation-pattern" };
      return { bad: null, write: true };
    }
    if (up && up.type === "UpdateExpression" && up.argument === target) {
      if (!asStatement(up)) return { bad: "ts-mutation-pattern" };
      if (depth > 0 && !owned) return { bad: "ts-mutation-pattern" };
      return { bad: null, write: true };
    }
    if (parent && parent.type === "MemberExpression" && parent.object === node && !parent.computed && parent.property.type === "Identifier") {
      const call = ref.path[idx - 2];
      if (call && call.type === "CallExpression" && call.callee === parent) {
        const method = parent.property.name;
        if (method === "push") {
          const stmt = ref.path[idx - 3];
          if (!owned || !stmt || stmt.type !== "ExpressionStatement") return { bad: "ts-mutation-pattern", write: true };
          return { bad: null, write: true };
        }
        if (method === "sort" || method === "reverse") {
          if (!owned) return { bad: "ts-mutation-pattern", write: true };
          return { bad: null, write: true };
        }
        if (MUTATING_METHODS.has(method)) return { bad: null, write: true };
      }
    }
    return null;
  };

  const functionOfRef = (ref) => ref.fns[ref.fns.length - 1] ?? null;

  const closureEscape = (binding, ref, mutations) => {
    const chain = ref.fns;
    const declaring = binding.fn;
    const start = declaring ? chain.indexOf(declaring) : -1;
    const inner = chain.slice(start + 1);
    if (inner.length === 0) return false;
    const isWrite = mutations.some((m) => m.ref === ref && m.write);
    if (isWrite) return true;
    for (const fn of inner) {
      const entry = functionNodes.find((f) => f.node === fn);
      if (!entry) continue;
      if (!isInlineCallback(fn, entry.path)) return true;
    }
    return false;
  };

  const unsafeStore = (binding, ref, mutations) => {
    let current = ref.path[ref.path.length - 1];
    let index = ref.path.length - 1;
    while (index > 0) {
      const above = ref.path[index - 1];
      if (above.type === "ConditionalExpression" && above.test !== current || above.type === "LogicalExpression" || above.type === "AwaitExpression" || above.type === "ChainExpression") {
        current = above;
        index--;
        continue;
      }
      break;
    }
    const above = ref.path[index - 1];
    if (!above) return false;
    let stored = false;
    switch (above.type) {
      case "VariableDeclarator":
        stored = above.init === current;
        break;
      case "AssignmentExpression":
        stored = above.right === current;
        break;
      case "Property":
        stored = above.value === current;
        break;
      case "ArrayExpression":
        stored = true;
        break;
      case "CallExpression": {
        if (!above.arguments.includes(current)) break;
        const callee = above.callee;
        if (callee.type === "Identifier") stored = declaredFunctions.has(callee.name);
        else if (callee.type === "MemberExpression" && isStringKey(callee.property, "push")) stored = true;
        break;
      }
      case "ReturnStatement":
        stored = false;
        break;
      default:
    }
    if (!stored) return false;
    const position = current.range[0];
    const laterMutation = mutations.some((m) => m.write && m.ref.node.range[0] > position);
    if (laterMutation) return true;
    const loopsOfRef = ref.loops;
    return loopsOfRef.some((loop) => mutations.some((m) => m.write && m.ref.node.range[0] >= loop.range[0] && m.ref.node.range[1] <= loop.range[1]));
  };

  const throwingFunctionSet = () => {
    const names = new Set();
    const hasThrowing = (body) => containsNode(body, (n) =>
      toolCall(n) !== null || n.type === "ThrowStatement"
      || (n.type === "CallExpression" && n.callee.type === "MemberExpression" && n.callee.object.type === "Identifier" && n.callee.object.name === "JSON" && isStringKey(n.callee.property, "parse"))
      || (n.type === "CallExpression" && n.callee.type === "Identifier" && names.has(n.callee.name)));
    let changed = true;
    while (changed) {
      changed = false;
      for (const { node, path: fnPath } of functionNodes) {
        const parent = fnPath[fnPath.length - 2];
        let name = null;
        if (node.type === "FunctionDeclaration" && node.id) name = node.id.name;
        else if (parent && parent.type === "VariableDeclarator" && parent.init === node && parent.id.type === "Identifier") name = parent.id.name;
        if (name && !names.has(name) && hasThrowing(node.body)) {
          names.add(name);
          changed = true;
        }
      }
    }
    return names;
  };

  const isThrowingOp = (n, names) =>
    toolCall(n) !== null || n.type === "ThrowStatement"
    || (n.type === "CallExpression" && n.callee.type === "MemberExpression" && n.callee.object.type === "Identifier" && n.callee.object.name === "JSON" && isStringKey(n.callee.property, "parse"))
    || (n.type === "CallExpression" && n.callee.type === "Identifier" && names.has(n.callee.name));

  const lostInTry = (binding, mutation, names) => {
    if (!mutation.write) return false;
    const path = mutation.ref.path;
    for (let i = path.length - 1; i > 0; i--) {
      if (path[i].type === "BlockStatement" && path[i - 1].type === "TryStatement" && path[i - 1].block === path[i]) {
        const block = path[i];
        const declaredInside = binding.declarator && binding.declarator.range[0] >= block.range[0] && binding.declarator.range[1] <= block.range[1];
        if (declaredInside) return false;
        const statement = [...path].reverse().find((n) => n.type === "ExpressionStatement");
        const position = (statement ?? mutation.ref.node).range[1];
        let after = false;
        const scan = (n) => {
          if (after) return;
          if (n.range[0] >= position && isThrowingOp(n, names)) after = true;
          for (const child of children(n)) scan(child);
        };
        scan(block);
        if (after) return true;
        for (let k = i + 1; k < path.length; k++) {
          const loop = path[k];
          if (["ForStatement", "ForOfStatement", "WhileStatement", "DoWhileStatement", "ForInStatement"].includes(loop.type)) {
            if (containsNode(loop, (n) => isThrowingOp(n, names))) return true;
          }
        }
        return false;
      }
    }
    return false;
  };

  const isParse = (n) =>
    n.type === "CallExpression" && n.callee.type === "MemberExpression" && n.callee.object.type === "Identifier"
    && n.callee.object.name === "JSON" && isStringKey(n.callee.property, "parse");

  const plainThrowerNames = () => {
    const names = new Set();
    const hasPlain = (body) => containsNode(body, (n) => n.type === "ThrowStatement" || isParse(n)
      || (n.type === "CallExpression" && n.callee.type === "Identifier" && names.has(n.callee.name)));
    let changed = true;
    while (changed) {
      changed = false;
      for (const { node, path: fnPath } of functionNodes) {
        const parent = fnPath[fnPath.length - 2];
        let name = null;
        if (node.type === "FunctionDeclaration" && node.id) name = node.id.name;
        else if (parent && parent.type === "VariableDeclarator" && parent.init === node && parent.id.type === "Identifier") name = parent.id.name;
        if (name && !names.has(name) && hasPlain(node.body)) {
          names.add(name);
          changed = true;
        }
      }
    }
    return names;
  };

  const inConditionPosition = (refPath) => {
    let index = refPath.length - 1;
    let child = refPath[index];
    while (index > 0) {
      const above = refPath[index - 1];
      if ((above.type === "IfStatement" || above.type === "ConditionalExpression") && above.test === child) return true;
      if (above.type === "UnaryExpression" && above.operator === "!") return true;
      if (above.type === "LogicalExpression" && above.left === child && above.operator === "&&") {
        child = above;
        index--;
        continue;
      }
      return false;
    }
    return false;
  };

  const analyzeCatches = () => {
    const plainThrowers = plainThrowerNames();
    const plainInside = (root) => containsNode(root, (n) => n.type === "ThrowStatement" || isParse(n)
      || (n.type === "CallExpression" && n.callee.type === "Identifier" && plainThrowers.has(n.callee.name)));
    for (const { clause, path: clausePath } of catches) {
      const tryStatement = clausePath[clausePath.length - 2];
      const param = clause.param;
      if (!param) continue;
      if (param.type !== "Identifier") {
        flag("ts-error-use");
        continue;
      }
      const binding = bindings.find((b) => b.kind === "catch" && b.declarator === clause && b.name === param.name);
      if (!binding) continue;
      const plain = plainInside(tryStatement.block);
      const parses = containsNode(tryStatement.block, isParse);
      for (const ref of binding.refs) {
        const idx = ref.path.length - 1;
        const parent = ref.path[idx - 1];
        const member = parent && parent.type === "MemberExpression" && parent.object === ref.node && !parent.computed && parent.property.type === "Identifier"
          ? parent.property.name : null;
        if (parent && parent.type === "ThrowStatement") continue;
        if (member !== null) {
          if (member === "message" && !parses) continue;
          if (member === "kind" && !plain) continue;
          flag("ts-error-use");
          continue;
        }
        if (inConditionPosition(ref.path)) continue;
        if (!plain) continue;
        flag("ts-error-use");
      }
    }
  };

  const analyzeSettled = () => {
    const plainThrowers = plainThrowerNames();
    const arrays = [];
    for (const binding of bindings) {
      const init = binding.declarator && binding.declarator.type === "VariableDeclarator" ? binding.declarator.init : null;
      if (!init || init.type !== "AwaitExpression" || init.argument.type !== "CallExpression") continue;
      const callee = init.argument.callee;
      if (!(callee.type === "MemberExpression" && isStringKey(callee.property, "allSettled") && callee.object.type === "Identifier" && callee.object.name === "Promise")) continue;
      const plain = containsNode(init.argument, (n) => n.type === "ThrowStatement" || isParse(n)
        || (n.type === "CallExpression" && n.callee.type === "Identifier" && plainThrowers.has(n.callee.name)));
      arrays.push({ binding, plain, tuple: binding.declarator.id.type === "ArrayPattern" });
    }
    const wholeValue = (plain) => {
      if (plain) flag("ts-settled-escape");
    };
    const elementUse = (ref, plain) => {
      const idx = ref.path.length - 1;
      const parent = ref.path[idx - 1];
      if (parent && parent.type === "MemberExpression" && parent.object === ref.node && !parent.computed && parent.property.type === "Identifier") {
        const name = parent.property.name;
        if (name === "status" || name === "value") return;
        if (name === "reason") {
          const above = ref.path[idx - 2];
          if (above && above.type === "MemberExpression" && above.object === parent && ["message", "kind"].includes(above.property.name)) return;
          if (above && above.type === "ChainExpression") return;
          wholeValue(plain);
          return;
        }
        flag("ts-settled-escape");
        return;
      }
      wholeValue(plain);
    };
    for (const { binding, plain, tuple } of arrays) {
      if (tuple) {
        const elementBindings = bindings.filter((b) => b.declarator === binding.declarator && b !== binding);
        for (const element of elementBindings) for (const ref of element.refs) elementUse(ref, plain);
        continue;
      }
      for (const ref of binding.refs) {
        const idx = ref.path.length - 1;
        const parent = ref.path[idx - 1];
        const grand = ref.path[idx - 2];
        if (parent.type === "MemberExpression" && parent.object === ref.node && parent.computed) {
          elementUse({ node: parent, path: ref.path.slice(0, idx) }, plain);
        } else if (parent.type === "MemberExpression" && parent.object === ref.node && !parent.computed && parent.property.type === "Identifier" && parent.property.name === "length") {
          continue;
        } else if (parent.type === "MemberExpression" && parent.object === ref.node && !parent.computed && grand && grand.type === "CallExpression" && grand.callee === parent
          && ["map", "forEach", "filter", "some", "every"].includes(parent.property.name)) {
          const callback = grand.arguments[0];
          if (callback && (callback.type === "ArrowFunctionExpression" || callback.type === "FunctionExpression") && callback.params[0] && callback.params[0].type === "Identifier") {
            const element = bindings.find((b) => b.declarator === callback && b.name === callback.params[0].name);
            if (element) for (const inner of element.refs) elementUse(inner, plain);
          } else wholeValue(plain);
        } else if (parent.type === "ForOfStatement" && parent.right === ref.node) {
          const head = parent.left;
          const element = head.type === "VariableDeclaration" ? bindings.find((b) => b.declarator === head.declarations[0]) : null;
          if (element) for (const inner of element.refs) elementUse(inner, plain);
          else wholeValue(plain);
        } else {
          wholeValue(plain);
        }
      }
    }
  };

  const analyzeRecursion = () => {
    for (const { node, path: fnPath } of functionNodes) {
      const parent = fnPath[fnPath.length - 2];
      let name = null;
      if (node.type === "FunctionDeclaration" && node.id) name = node.id.name;
      else if (parent && parent.type === "VariableDeclarator" && parent.init === node && parent.id.type === "Identifier") name = parent.id.name;
      if (!name) continue;
      if (containsNode(node.body, (n) => n.type === "CallExpression" && n.callee.type === "Identifier" && n.callee.name === name)) {
        features.add("recursion");
        flag("ts-recursion");
      }
    }
  };

  const wrapper = tree.body[0];
  scopes.push(new Map());
  fnStack.push(wrapper);
  const programScope = new Map();
  scopes.push(programScope);
  hoist(programScope, wrapper.body.body);
  path.push(tree, wrapper, wrapper.body);
  for (const statement of wrapper.body.body) visit(statement);
  path.length = 0;
  current = null;
  analyze();

  const callsTools = calls.length > 0;
  const byCode = Object.fromEntries([...codes.entries()].sort());
  const callSetOk = callsTools && !CODES_FOR_CALL_SET.some((id) => codes.has(id));
  const accepted = callSetOk && codes.size === 0;
  const needsRewriting = REWRITTEN_FEATURES.some((f) => features.has(f));
  return {
    parsed: true,
    features: [...features].sort(),
    calls,
    library: [...library].sort(),
    callsTools,
    codes: byCode,
    sites,
    tiers: {
      callSet: callSetOk,
      asWritten: accepted && !needsRewriting,
      accepted,
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
