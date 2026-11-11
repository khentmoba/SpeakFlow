import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, describe, it } from "node:test";
import { findTestScopeViolations, ROOT } from "./check_frontend_test_scope.mjs";

const scratch = mkdtempSync(join(tmpdir(), "frontend-test-scope-"));
after(() => rmSync(scratch, { recursive: true, force: true }));

let fixtureCount = 0;
function fixture(files) {
  const root = join(scratch, `case-${fixtureCount++}`);
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  }
  return root;
}

function messagesFor(files) {
  return findTestScopeViolations(fixture(files)).map((violation) => violation.message);
}

describe("frontend test scope", () => {
  it("passes the repository as checked in", () => {
    assert.deepEqual(findTestScopeViolations(ROOT), []);
  });

  it("allows logic-only hook tests", () => {
    assert.deepEqual(messagesFor({
      "src/hooks/useThing.test.tsx": 'import { act, renderHook, waitFor } from "@testing-library/react";\n',
    }), []);
  });

  it("allows type-only imports", () => {
    assert.deepEqual(messagesFor({
      "src/hooks/useThing.test.tsx": 'import type { RenderHookResult } from "@testing-library/react";\n',
    }), []);
  });

  it("rejects component rendering helpers", () => {
    const messages = messagesFor({
      "src/pages/Page.test.tsx": 'import { render, screen } from "@testing-library/react";\n',
    });
    assert.equal(messages.length, 1);
    assert.match(messages[0], /render, screen/);
  });

  it("rejects namespace imports from testing-library/react", () => {
    assert.equal(messagesFor({ "src/a.test.tsx": 'import * as rtl from "@testing-library/react";\n' }).length, 1);
  });

  it("rejects default and combined bindings from testing-library/react", () => {
    for (const statement of [
      'import rtl from "@testing-library/react";',
      'import rtl, { renderHook } from "@testing-library/react";',
    ]) {
      assert.equal(messagesFor({ "src/a.test.tsx": `${statement}\n` }).length, 1, statement);
    }
  });

  it("rejects re-exports of rendering helpers", () => {
    for (const statement of [
      'export { render } from "@testing-library/react";',
      'export * from "@testing-library/react";',
      'export * as rtl from "@testing-library/react";',
    ]) {
      assert.equal(messagesFor({ "src/a.ts": `${statement}\n` }).length, 1, statement);
    }
  });

  it("allows named re-exports of the allowed hooks", () => {
    assert.deepEqual(messagesFor({ "src/a.ts": 'export { renderHook } from "@testing-library/react";\n' }), []);
  });

  it("rejects dynamic loads, requires, and mocks of rendering or DOM libraries", () => {
    for (const statement of [
      'const rtl = await import("@testing-library/react");',
      'const { render } = require("@testing-library/react");',
      'const ue = await import("@testing-library/user-event");',
      'vi.mock("@testing-library/dom", () => ({}));',
      'jest.mock("axe-core");',
      'const { render } = await vi.importActual("@testing-library/react");',
      'const { render } = jest.requireActual("@testing-library/react");',
    ]) {
      assert.equal(messagesFor({ "src/a.test.ts": `${statement}\n` }).length, 1, statement);
    }
  });

  it("applies the same bindings rules to subpaths of testing-library/react", () => {
    for (const statement of [
      'import { render } from "@testing-library/react/pure";',
      'import * as pure from "@testing-library/react/pure";',
      'const { render } = await import("@testing-library/react/pure");',
    ]) {
      assert.equal(messagesFor({ "src/a.test.tsx": `${statement}\n` }).length, 1, statement);
    }
    assert.deepEqual(messagesFor({
      "src/a.test.tsx": 'import { renderHook } from "@testing-library/react/pure";\n',
    }), []);
  });

  it("rejects DOM and accessibility libraries", () => {
    const specifiers = [
      "@testing-library/dom",
      "@testing-library/user-event",
      "@testing-library/jest-dom/vitest",
      "axe-core",
      "jest-axe",
    ];
    for (const specifier of specifiers) {
      assert.equal(messagesFor({ "src/a.test.ts": `import "${specifier}";\n` }).length, 1, specifier);
    }
  });

  it("rejects DOM matchers that a library grep misses", () => {
    const messages = messagesFor({
      "src/components/Nav.test.tsx": [
        'expect(button.classList.contains("on")).toBe(true);',
        'expect(item).toHaveClass("active");',
        "",
      ].join("\n"),
    });
    assert.equal(messages.length, 1);
    assert.match(messages[0], /toHaveClass/);
  });

  it("rejects UI-test packages declared in package.json", () => {
    const messages = messagesFor({
      "package.json": JSON.stringify({ devDependencies: { "@testing-library/user-event": "^14.0.0" } }),
    });
    assert.equal(messages.length, 1);
    assert.match(messages[0], /@testing-library\/user-event/);
  });
});
