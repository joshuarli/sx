import { parse, type Node } from "acorn";

type Ast = Node & Record<string, any>;

const ARITHMETIC = new Set(["+", "-", "*", "/", "%"]);
const COMPOUND = new Set(["+=", "-=", "*=", "/=", "%="]);
const PREFIX = "async function __program() {\n";

function childNodes(node: Ast): Ast[] {
  const found: Ast[] = [];
  for (const key of Object.keys(node)) {
    if (key === "type" || key === "start" || key === "end") continue;
    const value = node[key];
    if (Array.isArray(value)) {
      for (const item of value) if (item && typeof item.type === "string") found.push(item);
    } else if (value && typeof value.type === "string") found.push(value);
  }
  return found.sort((a, b) => a.start - b.start);
}

export function instrument(javascript: string): string {
  const source = `${PREFIX}${javascript}\n}`;
  const tree = parse(source, { ecmaVersion: "latest", sourceType: "script", allowAwaitOutsideFunction: false }) as Ast;
  const program = tree.body[0] as Ast;
  let fresh = 0;
  let chains = 0;

  const splice = (node: Ast): string => {
    let out = "";
    let cursor = node.start;
    for (const child of childNodes(node)) {
      if (child.start < cursor) continue;
      out += source.slice(cursor, child.start) + emit(child);
      cursor = child.end;
    }
    return out + source.slice(cursor, node.end);
  };

  const target = (node: Ast): string => {
    if (node.type !== "MemberExpression") return emit(node);
    const object = emit(node.object);
    return node.computed ? `${object}[${emit(node.property)}]` : `${object}.${node.property.name}`;
  };

  const withDestructuring = (node: Ast, params: Ast[]): { params: string; prologue: string } => {
    let prologue = "";
    let cursor = node.start;
    let text = "";
    const first = params[0];
    if (first === undefined) return { params: "", prologue };
    cursor = first.start;
    params.forEach((param, index) => {
      text += source.slice(cursor, param.start);
      if (param.type === "ArrayPattern") {
        const name = `$p${fresh++}`;
        text += name;
        prologue += `const ${source.slice(param.start, param.end)} = $.d(${name}, ${param.elements.length});`;
      } else text += emit(param);
      cursor = param.end;
      void index;
    });
    return { params: text, prologue };
  };

  const emitFunction = (node: Ast): string => {
    const params = node.params as Ast[];
    const patterned = params.some((p) => p.type === "ArrayPattern");
    if (!patterned) return splice(node);
    const { params: text, prologue } = withDestructuring(node, params);
    const head = source.slice(node.start, params[0]!.start);
    const tail = source.slice(params[params.length - 1]!.end, node.body.start);
    const body =
      node.body.type === "BlockStatement"
        ? `{${prologue}${emit(node.body)}}`
        : `{${prologue}return (${emit(node.body)});}`;
    return `${head}${text}${tail}${body}`;
  };

  const emit = (node: Ast): string => {
    switch (node.type) {
      case "ChainExpression": {
        chains++;
        const text = splice(node);
        chains--;
        return text;
      }
      case "BinaryExpression":
        if (ARITHMETIC.has(node.operator)) return `$.a(${JSON.stringify(node.operator)}, ${emit(node.left)}, ${emit(node.right)})`;
        return splice(node);
      case "AssignmentExpression": {
        if (COMPOUND.has(node.operator)) {
          const op = node.operator.slice(0, -1);
          return `(${target(node.left)} = $.a(${JSON.stringify(op)}, ${emit(node.left)}, ${emit(node.right)}))`;
        }
        return `${target(node.left)} ${node.operator} ${emit(node.right)}`;
      }
      case "UpdateExpression":
        return node.prefix ? `${node.operator}${target(node.argument)}` : `${target(node.argument)}${node.operator}`;
      case "MemberExpression":
        if (node.computed && chains === 0 && !node.optional && !(node.property.type === "Literal" && typeof node.property.value === "string")) {
          return `$.i(${emit(node.object)}, ${emit(node.property)})`;
        }
        return splice(node);
      case "CallExpression": {
        const callee = node.callee as Ast;
        if (
          chains === 0 && !node.optional && callee.type === "MemberExpression" && !callee.computed && !callee.optional &&
          callee.property.name === "slice"
        ) {
          const args = (node.arguments as Ast[]).map(emit).join(", ");
          return `$.sl(${emit(callee.object)}${args ? ", " : ""}${args})`;
        }
        return splice(node);
      }
      case "VariableDeclarator":
        if (node.id.type === "ArrayPattern" && node.init) {
          return `${splice(node.id)} = $.d(${emit(node.init)}, ${node.id.elements.length})`;
        }
        return splice(node);
      case "ForOfStatement": {
        const left = node.left as Ast;
        if (left.type === "VariableDeclaration" && left.declarations[0].id.type === "ArrayPattern") {
          const pattern = left.declarations[0].id as Ast;
          const name = `$f${fresh++}`;
          const awaited = node.await ? "await " : "";
          return `for ${awaited}(${left.kind} ${name} of ${emit(node.right)}) { ${left.kind} ${source.slice(pattern.start, pattern.end)} = $.d(${name}, ${pattern.elements.length}); ${emit(node.body)} }`;
        }
        return splice(node);
      }
      case "CatchClause": {
        const name = `$e${fresh++}`;
        const bound = node.param ? `const ${source.slice(node.param.start, node.param.end)} = ${name};` : "";
        return `catch (${name}) { $.c(${name}); ${bound} ${emit(node.body)} }`;
      }
      case "FunctionDeclaration":
      case "FunctionExpression":
      case "ArrowFunctionExpression":
        return emitFunction(node);
      default:
        return splice(node);
    }
  };

  const body = program.body as Ast;
  return body.body.map((statement: Ast, index: number, all: Ast[]) => {
    const gap = index === 0 ? source.slice(body.start + 1, statement.start) : source.slice(all[index - 1]!.end, statement.start);
    return gap + emit(statement);
  }).join("") + source.slice((body.body.at(-1) as Ast | undefined)?.end ?? body.start + 1, body.end - 1);
}
