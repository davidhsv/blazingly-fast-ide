// The webview UI's TypeScript program and the static resolution the port's generators share:
// which object literals an expression can evaluate to, and the literal values of their properties.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";

export const root = process.cwd().replaceAll("\\", "/");

export const config = ts.readConfigFile(`${root}/tsconfig.json`, ts.sys.readFile);
export const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
export const program = ts.createProgram(parsed.fileNames, parsed.options);
export const checker = program.getTypeChecker();
export const rel = (file) => file.replaceAll("\\", "/").slice(root.length + 1);
export const isTest = (file) => /\.test\.tsx?$|\/__tests__\/|\/__fixtures__\/|^src\/test\//.test(file);
export const appSources = program
  .getSourceFiles()
  .filter((s) => rel(s.fileName).startsWith("src/") && !isTest(rel(s.fileName)) && !s.isDeclarationFile);
export const at = (node) => {
  const source = node.getSourceFile();
  return `${rel(source.fileName)}:${source.getLineAndCharacterOfPosition(node.getStart()).line + 1}`;
};

export function walk(dir, accept, files = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (name === "node_modules") continue;
    if (statSync(p).isDirectory()) walk(p, accept, files);
    else if (accept(p)) files.push(p);
  }
  return files;
}
export const lineCount = (file) => readFileSync(file, "utf8").split("\n").length;
export const isSource = (p) => /\.(ts|tsx)$/.test(p) && !isTest(rel(p));

// ---------------------------------------------------------------------------------------------
// Static resolution of contribution objects and their ids.

export const unwrap = (node) => {
  while (
    node &&
    (ts.isParenthesizedExpression(node) ||
      ts.isAsExpression(node) ||
      ts.isSatisfiesExpression(node) ||
      ts.isNonNullExpression(node) ||
      ts.isTypeAssertionExpression?.(node))
  )
    node = node.expression;
  return node;
};

export function declarationsOf(identifier) {
  let symbol = checker.getSymbolAtLocation(identifier);
  if (symbol && symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol);
  return symbol?.declarations ?? [];
}

export function returnedExpressions(fn) {
  if (!fn.body) return [];
  if (!ts.isBlock(fn.body)) return [fn.body];
  const found = [];
  const visit = (node) => {
    if (ts.isFunctionLike(node) && node !== fn) return;
    if (ts.isReturnStatement(node) && node.expression) found.push(node.expression);
    ts.forEachChild(node, visit);
  };
  visit(fn.body);
  return found;
}

/** The object literals `expr` can evaluate to (through identifiers, arrays, spreads and helper calls). */
export function objectLiterals(expr, depth = 0, seen = new Set()) {
  expr = unwrap(expr);
  if (!expr || depth > 8 || seen.has(expr)) return [];
  seen.add(expr);
  const next = (e) => objectLiterals(e, depth + 1, seen);
  if (ts.isObjectLiteralExpression(expr)) return [expr];
  if (ts.isArrayLiteralExpression(expr))
    return expr.elements.flatMap((e) => next(ts.isSpreadElement(e) ? e.expression : e));
  if (ts.isConditionalExpression(expr)) return [...next(expr.whenTrue), ...next(expr.whenFalse)];
  if (ts.isBinaryExpression(expr)) return [...next(expr.left), ...next(expr.right)];
  if (ts.isIdentifier(expr)) {
    return declarationsOf(expr).flatMap((d) => {
      if (ts.isVariableDeclaration(d) && d.initializer) return next(d.initializer);
      if (ts.isParameter(d) && d.initializer) return next(d.initializer);
      return [];
    });
  }
  if (ts.isArrowFunction(expr) || ts.isFunctionExpression(expr)) return returnedExpressions(expr).flatMap(next);
  if (ts.isCallExpression(expr)) {
    const callee = unwrap(expr.expression);
    // `list.map((x) => ({ ... }))`, `list.filter(...)`, `Object.values(table)`, ...
    if (ts.isPropertyAccessExpression(callee)) {
      const method = callee.name.text;
      if (["map", "flatMap"].includes(method) && expr.arguments[0]) {
        const mapped = next(expr.arguments[0]);
        return mapped.length ? mapped : next(callee.expression);
      }
      if (["filter", "concat", "slice", "sort", "reverse"].includes(method))
        return [...next(callee.expression), ...expr.arguments.flatMap(next)];
      if (["values", "freeze"].includes(method) && expr.arguments[0]) return next(expr.arguments[0]);
    }
    if (ts.isIdentifier(callee)) {
      return declarationsOf(callee).flatMap((d) => {
        if (ts.isFunctionDeclaration(d)) return returnedExpressions(d).flatMap(next);
        if (ts.isVariableDeclaration(d) && d.initializer) {
          const init = unwrap(d.initializer);
          if (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) return returnedExpressions(init).flatMap(next);
        }
        return [];
      });
    }
  }
  return [];
}

/** String values a property initializer can take; empty when it is computed at run time. */
export function literals(node, depth = 0) {
  node = unwrap(node);
  if (!node || depth > 6) return [];
  if (ts.isStringLiteralLike(node)) return [node.text];
  if (ts.isConditionalExpression(node))
    return [...literals(node.whenTrue, depth + 1), ...literals(node.whenFalse, depth + 1)];
  if (ts.isIdentifier(node)) {
    const fromDeclaration = declarationsOf(node).flatMap((d) =>
      ts.isVariableDeclaration(d) && d.initializer ? literals(d.initializer, depth + 1) : [],
    );
    if (fromDeclaration.length) return fromDeclaration;
  }
  const type = checker.getTypeAtLocation(node);
  return (type.isUnion() ? type.types : [type]).flatMap((part) => (part.isStringLiteral() ? [part.value] : []));
}

export function property(object, name) {
  for (const p of object.properties) {
    if (ts.isPropertyAssignment(p) && p.name.getText().replace(/["']/g, "") === name) return literals(p.initializer);
    if (ts.isShorthandPropertyAssignment(p) && p.name.text === name) return literals(p.name);
  }
  return [];
}

// ---------------------------------------------------------------------------------------------
// Actions: every object literal typed as an Action ({ id, title, run }).

export function isAction(type) {
  return ["id", "title", "run"].every((name) => checker.getPropertyOfType(type, name));
}

/** Function node → its call expressions, for ids passed into a factory (`text("editor.x", …)`). */
export let callSitesByFunction = null;
export function callsOf(fn) {
  if (!callSitesByFunction) {
    callSitesByFunction = new Map();
    for (const source of appSources) {
      const visit = (node) => {
        if (ts.isCallExpression(node) && ts.isIdentifier(unwrap(node.expression))) {
          for (const d of declarationsOf(unwrap(node.expression))) {
            const target = ts.isVariableDeclaration(d) && d.initializer ? unwrap(d.initializer) : d;
            if (ts.isFunctionLike(target))
              callSitesByFunction.set(target, [...(callSitesByFunction.get(target) ?? []), node]);
          }
        }
        ts.forEachChild(node, visit);
      };
      visit(source);
    }
  }
  return callSitesByFunction.get(fn) ?? [];
}

/** Ids a factory's object literal takes: the literal arguments its callers pass for the `id` parameter. */
export function idsFromCallers(object) {
  const p = object.properties.find(
    (q) => (ts.isPropertyAssignment(q) || ts.isShorthandPropertyAssignment(q)) && q.name.getText() === "id",
  );
  const value = p && unwrap(ts.isPropertyAssignment(p) ? p.initializer : p.name);
  if (!value || !ts.isIdentifier(value)) return [];
  const symbol = ts.isShorthandPropertyAssignment(p)
    ? checker.getShorthandAssignmentValueSymbol(p)
    : checker.getSymbolAtLocation(value);
  const parameter = symbol?.declarations?.find(ts.isParameter);
  if (!parameter || !ts.isFunctionLike(parameter.parent)) return [];
  const index = parameter.parent.parameters.indexOf(parameter);
  return callsOf(parameter.parent).flatMap((call) => (call.arguments[index] ? literals(call.arguments[index]) : []));
}

export function actions() {
  const found = new Map();
  for (const source of appSources) {
    const visit = (node) => {
      if (ts.isObjectLiteralExpression(node)) {
        const type = checker.getContextualType(node) ?? checker.getTypeAtLocation(node);
        const names = new Set(node.properties.map((p) => p.name?.getText().replace(/["']/g, "")));
        if (names.has("id") && names.has("run") && isAction(type)) {
          const own = property(node, "id");
          const ids = own.length ? own : idsFromCallers(node);
          if (ids.length) {
            for (const id of ids) if (!found.has(id)) found.set(id, at(node));
          } else {
            // An id built at run time (one action per plugin command, per run configuration, ...).
            found.set(`(dynamic) ${at(node)}`, at(node));
          }
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return found;
}

// ---------------------------------------------------------------------------------------------
// Extension points: every call of an exported `register*` function of the workbench, the platform
// and the editor API (direct, or passed to `.map` / `.forEach`).

export const POINT_FILES = /^src\/(workbench\/|platform\/|editor\/api\.ts|editor\/navigationProviders\.ts)/;
export const COVERED_ELSEWHERE = new Set(["registerAction", "registerActions", "registerMessages"]);
export const ID_PROPERTIES = ["id", "kind", "name", "slot", "language", "scheme"];

export function pointName(identifier) {
  if (!ts.isIdentifier(identifier) || !/^register[A-Z]/.test(identifier.text)) return null;
  const declared = declarationsOf(identifier).some((d) => POINT_FILES.test(rel(d.getSourceFile().fileName)));
  return declared && !COVERED_ELSEWHERE.has(identifier.text) ? identifier.text : null;
}

export function extensionPoints() {
  /** point → Map(row key → source) */
  const points = new Map();
  const add = (point, objects, callSite) => {
    const rows = points.get(point) ?? new Map();
    points.set(point, rows);
    const ids = objects.flatMap((o) => ID_PROPERTIES.map((p) => property(o, p)).find((v) => v.length) ?? []);
    if (ids.length) {
      for (const id of ids) if (!rows.has(id)) rows.set(id, at(callSite));
    } else {
      // A handler or provider without an id of its own: the registration site is the row.
      rows.set(`(at) ${at(callSite)}`, at(callSite));
    }
  };
  for (const source of appSources) {
    const visit = (node) => {
      if (ts.isCallExpression(node)) {
        const callee = unwrap(node.expression);
        const direct = pointName(callee);
        if (direct && node.arguments.length) {
          // The declaration itself (`export const registerX = register(...)`) is not a use.
          add(direct, objectLiterals(node.arguments[0]), node);
        } else if (ts.isPropertyAccessExpression(callee) && ["map", "forEach", "flatMap"].includes(callee.name.text)) {
          const passed = node.arguments[0] && pointName(unwrap(node.arguments[0]));
          if (passed) add(passed, objectLiterals(callee.expression), node);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return points;
}
