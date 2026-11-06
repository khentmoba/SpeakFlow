import { describe, expect, it } from "vitest";
import {
  ASR_CATALOG,
  CATALOG_MODELS,
  catalogLanguages,
  findCatalogModel,
  formatModelSize,
  languageName,
} from "@/lib/asrCatalog";
import {
  accuracyFor,
  bestLanguage,
  benchmarkFor,
  filterCatalogModels,
  formatModelSpeed,
  gradeScore,
} from "@/lib/asrBenchmarks";

describe("ASR catalog", () => {
  it("carries the generated Handy catalog with unique keys", () => {
    expect(ASR_CATALOG.engine_version).toBe("0.4.1");
    expect(CATALOG_MODELS.length).toBeGreaterThan(40);
    // Retired families stay out; see src-tauri/tests/retired_*_contract.rs.
    expect(CATALOG_MODELS.some((model) => /whisper|sensevoice/i.test(model.key))).toBe(false);
    const keys = CATALOG_MODELS.map((model) => model.key);
    expect(new Set(keys).size).toBe(keys.length);
    for (const model of CATALOG_MODELS) {
      expect(model.filename.endsWith(".gguf")).toBe(true);
      expect(model.size).toBeGreaterThan(1_000_000);
      expect(model.languages.length).toBeGreaterThan(0);
      expect(model.license).not.toBe("other");
    }
  });

  it("keeps the models SpeakFlow already shipped", () => {
    for (const key of [
      "qwen3-asr-0.6b",
      "qwen3-asr-1.7b",
      "parakeet-unified-en-0.6b",
      "parakeet-tdt-0.6b-v2",
      "canary-qwen-2.5b",
      "cohere-transcribe-03-2026",
    ]) {
      expect(findCatalogModel(key)).toBeDefined();
    }
    expect(findCatalogModel("confucius4-r2t2")).toBeUndefined();
  });

  it("formats sizes the way the engine list already showed them", () => {
    expect(formatModelSize(731_357_568)).toBe("731 MB");
    expect(formatModelSize(850_423_456)).toBe("850 MB");
    expect(formatModelSize(2_185_030_624)).toBe("2.19 GB");
    expect(formatModelSize(2_797_548_928)).toBe("2.80 GB");
    expect(formatModelSize(25_810_000_000)).toBe("25.81 GB");
  });

  it("formats speed and leaves a missing benchmark empty", () => {
    expect(formatModelSpeed(1.19)).toBe("1.19 s");
    expect(formatModelSpeed(null)).toBeNull();
    expect(benchmarkFor("moonshine-tiny")?.accuracy.en[0]).toMatch(/^[A-F][+-]?$/);
  });

  it("ranks grades from A+ down to F", () => {
    expect(gradeScore("A+")).toBeGreaterThan(gradeScore("A"));
    expect(gradeScore("A-")).toBeGreaterThan(gradeScore("B+"));
    expect(gradeScore("C")).toBeGreaterThan(gradeScore("D"));
    expect(gradeScore("F")).toBeLessThan(gradeScore("D-"));
  });

  it("reports English accuracy when the model has it", () => {
    const parakeet = findCatalogModel("parakeet-unified-en-0.6b")!;
    expect(accuracyFor(parakeet, "all")).toMatchObject({ language: "en", grade: "A-" });
    expect(accuracyFor(parakeet, "fr")).toBeNull();
  });

  it("falls back to the best language for models without English", () => {
    const gigaam = findCatalogModel("gigaam-v3-ctc")!;
    expect(benchmarkFor(gigaam.key)?.accuracy.en).toBeUndefined();
    expect(bestLanguage(gigaam)).toBe("ru");
    expect(accuracyFor(gigaam, "all")).toMatchObject({ language: "ru" });
  });

  it("lists languages by coverage then name", () => {
    const languages = catalogLanguages();
    expect(languages[0].modelCount).toBeGreaterThanOrEqual(languages[languages.length - 1].modelCount);
    const english = languages.find((entry) => entry.code === "en")!;
    expect(english.modelCount).toBeGreaterThan(40);
    expect(languageName("en")).toBe("English");
    expect(languageName("zh")).toBe("Chinese");
  });

  it("ranks a language filter by that language's grade", () => {
    const french = filterCatalogModels({ language: "fr", query: "" }, CATALOG_MODELS);
    expect(french.length).toBeGreaterThan(5);
    expect(french.every((model) => model.languages.includes("fr"))).toBe(true);
    const scores = french.map((model) => gradeScore(benchmarkFor(model.key)!.accuracy.fr[0]));
    expect(scores).toEqual([...scores].sort((a, b) => b - a));
  });

  it("ranks the unfiltered list by English grade, then error, then speed", () => {
    const models = filterCatalogModels({ language: "all", query: "" }, CATALOG_MODELS);
    const scored = models.filter((model) => benchmarkFor(model.key)?.accuracy.en);
    for (let index = 1; index < scored.length; index += 1) {
      const previous = benchmarkFor(scored[index - 1].key)!.accuracy.en;
      const current = benchmarkFor(scored[index].key)!.accuracy.en;
      const left = gradeScore(previous[0]);
      const right = gradeScore(current[0]);
      expect(left > right || (left === right && previous[1] <= current[1])).toBe(true);
    }
  });

  it("matches the search query against name, description and key", () => {
    expect(filterCatalogModels({ language: "all", query: "moonshine" }, CATALOG_MODELS).length)
      .toBeGreaterThan(3);
    expect(filterCatalogModels({ language: "all", query: "parakeet-ultra" }, CATALOG_MODELS)
      .map((model) => model.key)).toEqual(["parakeet-ultra"]);
    expect(filterCatalogModels({ language: "all", query: "zzzz" }, CATALOG_MODELS)).toEqual([]);
  });
});
