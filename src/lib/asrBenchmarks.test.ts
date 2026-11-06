import { describe, expect, it } from "vitest";
import { CATALOG_MODELS } from "@/lib/asrCatalog";
import {
  ENGLISH_RECOMMENDED_MODEL,
  recommendedModelFor,
} from "@/lib/asrBenchmarks";

describe("recommendedModelFor", () => {
  it("picks Parakeet Unified EN for English", () => {
    expect(recommendedModelFor("en")?.key).toBe(ENGLISH_RECOMMENDED_MODEL);
    expect(ENGLISH_RECOMMENDED_MODEL).toBe("parakeet-unified-en-0.6b");
  });

  it("follows the benchmark for other languages", () => {
    // German: Parakeet Ultra and Whisper Large v3 both score A-, but Parakeet
    // Ultra is roughly ten times faster on the published benchmark.
    expect(recommendedModelFor("de")?.key).toBe("parakeet-ultra");
    // Spanish: Parakeet Ultra is the only A+ model inside the download budget.
    expect(recommendedModelFor("es")?.key).toBe("parakeet-ultra");
    // Japanese: Fun-ASR Nano Multilingual is the only A+ model.
    expect(recommendedModelFor("ja")?.key).toBe("fun-asr-mlt-nano-2512");
  });

  it("never recommends a model above the download budget when a smaller one covers the language", () => {
    for (const language of ["de", "fr", "es", "ja", "ko", "pt", "it"]) {
      const model = recommendedModelFor(language);
      expect(model, language).not.toBeNull();
      expect(model!.size, `${language}: ${model!.key}`).toBeLessThanOrEqual(4_000_000_000);
    }
  });

  it("only recommends models that list the language", () => {
    for (const language of ["de", "ja", "zh", "ru", "ar", "hi"]) {
      const model = recommendedModelFor(language);
      expect(model, language).not.toBeNull();
      expect(model!.languages, `${language}: ${model!.key}`).toContain(language);
    }
  });

  it("falls back to the best available model when nothing is affordable", () => {
    const onlyHuge = CATALOG_MODELS.filter((model) => model.key === "voxtral-small-24b-2507");
    expect(recommendedModelFor("de", onlyHuge)?.key).toBe("voxtral-small-24b-2507");
  });

  it("returns nothing for a language no model covers", () => {
    expect(recommendedModelFor("xx")).toBeNull();
    expect(recommendedModelFor("all")).toBeNull();
  });
});
