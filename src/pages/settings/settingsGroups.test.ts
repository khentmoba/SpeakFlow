import { describe, expect, it } from "vitest";
import en from "@/i18n/en";
import {
  SETTINGS_GROUPS,
  SETTINGS_SECTIONS,
  groupForSection,
  sectionsInGroup,
} from "./settingsGroups";

function label(key: string): string {
  return key.split(".").reduce<unknown>(
    (node, part) => (node as Record<string, unknown> | undefined)?.[part],
    en,
  ) as string;
}

describe("settings groups", () => {
  it("assigns every section to exactly one group", () => {
    const assigned = SETTINGS_GROUPS.flatMap((group) => group.sectionIds);
    expect(new Set(assigned).size).toBe(assigned.length);
    expect([...assigned].sort()).toEqual(SETTINGS_SECTIONS.map(({ id }) => id).sort());
  });

  it("only names sections that exist", () => {
    const known = new Set(SETTINGS_SECTIONS.map(({ id }) => id));
    for (const group of SETTINGS_GROUPS) {
      for (const id of group.sectionIds) expect(known.has(id)).toBe(true);
    }
  });

  it("returns a group's sections in the order the group lists them", () => {
    expect(sectionsInGroup("audio").map(({ id }) => id)).toEqual(["microphone", "audio-behavior"]);
  });

  it("finds the owning group for every section", () => {
    for (const { id } of SETTINGS_SECTIONS) expect(groupForSection(id)).toBeDefined();
    expect(groupForSection("nope")).toBeUndefined();
  });

  it("gives every group and section a real English label", () => {
    for (const group of SETTINGS_GROUPS) {
      expect(label(group.labelKey), group.labelKey).toBeTypeOf("string");
      expect(label(group.labelKey).length, group.labelKey).toBeGreaterThan(0);
    }
    for (const section of SETTINGS_SECTIONS) {
      expect(label(section.labelKey), section.labelKey).toBeTypeOf("string");
    }
  });
});
