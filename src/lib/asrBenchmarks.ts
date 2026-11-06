import benchmarks from "../../src-tauri/resources/asr_benchmarks.json";
import type { CatalogModel } from "@/lib/asrCatalog";

interface Benchmark {
  /** Time to transcribe 30 seconds of audio on the benchmark machine. */
  speed: number | null;
  /** Language code to `[grade, errorPercent]`. */
  accuracy: Record<string, [string, number]>;
}

interface BenchmarkFile {
  source_commit: string;
  models: Record<string, Benchmark>;
}

/**
 * Published benchmark figures from Handy, kept out of the startup bundle: only
 * the model browser and its tests read this file.
 */
export const ASR_BENCHMARKS = benchmarks as unknown as BenchmarkFile;

export function benchmarkFor(key: string): Benchmark | undefined {
  return ASR_BENCHMARKS.models[key];
}

const GRADE_SCORES: Record<string, number> = {
  "A+": 11,
  A: 10,
  "A-": 9,
  "B+": 8,
  B: 7,
  "B-": 6,
  "C+": 5,
  C: 4,
  "C-": 3,
  "D+": 2,
  D: 1,
  "D-": 0,
  F: -1,
};

export function gradeScore(grade: string): number {
  return GRADE_SCORES[grade] ?? -2;
}

export function formatModelSpeed(secondsPer30s: number | null): string | null {
  return secondsPer30s === null ? null : `${secondsPer30s.toFixed(2)} s`;
}

export interface ModelAccuracy {
  language: string;
  grade: string;
  errorPercent: number;
}

/** Accuracy for one language, or the model's English entry when it has one. */
export function accuracyFor(model: CatalogModel, language: string | "all"): ModelAccuracy | null {
  const benchmark = benchmarkFor(model.key);
  if (!benchmark) return null;
  const code = language === "all"
    ? (benchmark.accuracy.en ? "en" : bestLanguage(model))
    : language;
  if (!code) return null;
  const entry = benchmark.accuracy[code];
  return entry ? { language: code, grade: entry[0], errorPercent: entry[1] } : null;
}

/** The language this model handles best, used when it has no English benchmark. */
export function bestLanguage(model: CatalogModel): string | null {
  const accuracy = benchmarkFor(model.key)?.accuracy ?? {};
  let best: string | null = null;
  for (const [code, [grade]] of Object.entries(accuracy)) {
    const bestGrade = best ? accuracy[best][0] : null;
    if (best === null || gradeScore(grade) > gradeScore(bestGrade!)) best = code;
  }
  return best;
}

function compareModels(language: string | "all") {
  return (a: CatalogModel, b: CatalogModel): number => {
    const left = accuracyFor(a, language);
    const right = accuracyFor(b, language);
    const leftScore = left ? gradeScore(left.grade) : -3;
    const rightScore = right ? gradeScore(right.grade) : -3;
    if (leftScore !== rightScore) return rightScore - leftScore;
    const leftError = left?.errorPercent ?? Number.POSITIVE_INFINITY;
    const rightError = right?.errorPercent ?? Number.POSITIVE_INFINITY;
    if (leftError !== rightError) return leftError - rightError;
    return (benchmarkFor(a.key)?.speed ?? Number.POSITIVE_INFINITY)
      - (benchmarkFor(b.key)?.speed ?? Number.POSITIVE_INFINITY);
  };
}

export interface ModelFilter {
  language: string | "all";
  query: string;
}

/**
 * Models for one language, best grade first, then lowest error, then fastest.
 * A search query matches names, descriptions and engine keys.
 */
export function filterCatalogModels(
  { language, query }: ModelFilter,
  models: readonly CatalogModel[],
): CatalogModel[] {
  const needle = query.trim().toLowerCase();
  return models
    .filter((model) => language === "all" || model.languages.includes(language))
    .filter((model) => !needle
      || model.name.toLowerCase().includes(needle)
      || model.description.toLowerCase().includes(needle)
      || model.key.includes(needle))
    .sort(compareModels(language));
}
