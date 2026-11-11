import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const TESTING_LIBRARY_REACT = "@testing-library/react";
const ALLOWED_REACT_IMPORTS = ["act", "cleanup", "renderHook", "waitFor"];
const BANNED_PACKAGES = [
  "@testing-library/dom",
  "@testing-library/user-event",
  "@testing-library/jest-dom",
  "axe-core",
  "jest-axe",
];
const DEPENDENCY_FIELDS = ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"];
const DOM_MATCHERS = [
  "toBeChecked",
  "toBeDisabled",
  "toBeEnabled",
  "toBeInTheDocument",
  "toBeVisible",
  "toContainElement",
  "toHaveAccessibleDescription",
  "toHaveAccessibleName",
  "toHaveAttribute",
  "toHaveClass",
  "toHaveDisplayValue",
  "toHaveFocus",
  "toHaveNoViolations",
  "toHaveStyle",
  "toHaveTextContent",
  "toHaveValue",
];
const DOM_MATCHER_PATTERN = new RegExp(`\\.(${DOM_MATCHERS.join("|")})\\b`, "g");
const FROM_PATTERN = /\b(import|export)\s+(type\s+)?([^"'`;]*?)\s*from\s*["']([^"']+)["']/g;
const OTHER_REFERENCE_PATTERN =
  /\bimport\s*["']([^"']+)["']|\bimport\(\s*["']([^"']+)["']\s*\)|\brequire\(\s*["']([^"']+)["']\s*\)|\b(?:vi|jest)\.(?:mock|doMock|importActual|requireActual)\(\s*["']([^"']+)["']/g;
const TEST_FILE_PATTERN = /\.(test|spec)\.[cm]?[jt]sx?$/;
const SOURCE_FILE_PATTERN = /\.[cm]?[jt]sx?$/;

function isBannedPackage(specifier) {
  return BANNED_PACKAGES.some((name) => specifier === name || specifier.startsWith(`${name}/`));
}

function isReactTestingLibrary(specifier) {
  return specifier === TESTING_LIBRARY_REACT || specifier.startsWith(`${TESTING_LIBRARY_REACT}/`);
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

function namedBindings(clause) {
  const body = /\{([\s\S]*?)\}/.exec(clause);
  if (!body) return [];
  return body[1]
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item && !/^type\s/.test(item))
    .map((item) => item.split(/\s+as\s+/)[0].trim());
}

function hasDefaultOrNamespaceBinding(clause) {
  return clause.split("{")[0].replace(/,\s*$/, "").trim() !== "";
}

function importViolations(text) {
  const violations = [];
  for (const match of text.matchAll(FROM_PATTERN)) {
    const [, keyword, typeOnly, clause, specifier] = match;
    if (typeOnly) continue;
    const line = lineOf(text, match.index);
    if (isBannedPackage(specifier)) {
      violations.push({ line, message: `${keyword}s ${specifier}; DOM and accessibility test libraries are not allowed` });
      continue;
    }
    if (!isReactTestingLibrary(specifier)) continue;
    if (hasDefaultOrNamespaceBinding(clause)) {
      violations.push({
        line,
        message: `default or namespace ${keyword} from ${TESTING_LIBRARY_REACT} can expose rendering helpers; use named hooks instead`,
      });
      continue;
    }
    const forbidden = namedBindings(clause).filter((name) => !ALLOWED_REACT_IMPORTS.includes(name));
    if (forbidden.length > 0) {
      violations.push({
        line,
        message: `${forbidden.join(", ")} from ${TESTING_LIBRARY_REACT}; logic-only tests may import only ${ALLOWED_REACT_IMPORTS.join(", ")}`,
      });
    }
  }
  for (const match of text.matchAll(OTHER_REFERENCE_PATTERN)) {
    const [, sideEffect, dynamic, required, mocked] = match;
    const specifier = sideEffect ?? dynamic ?? required ?? mocked;
    const line = lineOf(text, match.index);
    if (isBannedPackage(specifier)) {
      violations.push({ line, message: `loads ${specifier}; DOM and accessibility test libraries are not allowed` });
    } else if (isReactTestingLibrary(specifier) && sideEffect === undefined) {
      violations.push({
        line,
        message: `loads ${TESTING_LIBRARY_REACT} dynamically or through a mock; import named hooks statically`,
      });
    }
  }
  return violations;
}

function matcherViolations(text) {
  return [...text.matchAll(DOM_MATCHER_PATTERN)].map((match) => ({
    line: lineOf(text, match.index),
    message: `uses DOM matcher ${match[1]}; assert on hook results or state instead`,
  }));
}

function packageViolations(root) {
  const file = join(root, "package.json");
  if (!existsSync(file)) return [];
  const text = readFileSync(file, "utf8");
  let manifest;
  try {
    manifest = JSON.parse(text);
  } catch (error) {
    throw new Error(`package.json is not valid JSON: ${error.message}`);
  }
  const violations = [];
  for (const field of DEPENDENCY_FIELDS) {
    for (const name of Object.keys(manifest[field] ?? {})) {
      if (!isBannedPackage(name)) continue;
      violations.push({
        file: "package.json",
        line: lineOf(text, text.indexOf(`"${name}"`)),
        message: `declares ${name}; UI-test packages are not allowed`,
      });
    }
  }
  return violations;
}

function sourceFiles(directory) {
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return SOURCE_FILE_PATTERN.test(entry.name) ? [path] : [];
  });
}

export function findTestScopeViolations(root = ROOT) {
  const violations = packageViolations(root);
  for (const file of sourceFiles(join(root, "src"))) {
    const text = readFileSync(file, "utf8");
    const name = relative(root, file).split(sep).join("/");
    const found = importViolations(text);
    if (TEST_FILE_PATTERN.test(file)) found.push(...matcherViolations(text));
    for (const item of found) violations.push({ file: name, ...item });
  }
  return violations;
}

function isEntryPoint() {
  return process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
}

if (isEntryPoint()) {
  const root = process.argv[2] ? resolve(process.argv[2]) : ROOT;
  const violations = findTestScopeViolations(root);
  if (violations.length > 0) {
    for (const violation of violations) {
      console.error(`${violation.file}:${violation.line}: ${violation.message}`);
    }
    console.error("Frontend tests are logic-only (AGENTS.md). Move DOM checks into a hook or src/lib function.");
    process.exit(1);
  }
  console.log("Frontend test scope: no UI-test helpers, DOM matchers, or banned packages.");
}
