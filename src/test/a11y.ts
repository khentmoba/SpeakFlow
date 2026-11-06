import axe from "axe-core";
import { expect } from "vitest";

/**
 * Runs axe against a rendered subtree. jsdom has no layout or paint, so
 * color-contrast cannot be measured here; every structural rule still runs.
 */
export async function expectNoA11yViolations(container: HTMLElement): Promise<void> {
  const results = await axe.run(container, {
    rules: {
      "color-contrast": { enabled: false },
      // Components render in isolation, outside a page landmark.
      region: { enabled: false },
    },
  });
  const summary = results.violations.map((violation) => ({
    id: violation.id,
    impact: violation.impact,
    nodes: violation.nodes.map((node) => node.html),
  }));
  expect(summary).toEqual([]);
}
