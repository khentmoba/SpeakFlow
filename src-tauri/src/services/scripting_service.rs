//! Embedded QuickJS scripting engine (`speakflow.config.ts`) and ordered
//! deterministic + script transformation runner.
//!
//! Supports:
//! - TypeScript/JavaScript config file (`speakflow.config.ts`) stripped via `oxc_parser`
//!   before running inside an isolated QuickJS runtime (no filesystem, network, or timer APIs).
//! - Built-in deterministic transformations (`lowercase`, `uppercase`, `titleCase`, `camelCase`,
//!   `snakeCase`, `kebabCase`, `stripTrailingPunctuation`, `codeWrap`, `markdownList`).
//! - Automatic scaffolding of `speakflow.config.ts`, `speakflow.d.ts`, `tsconfig.json`, and
//!   `AGENTS.md` in the SpeakFlow config folder.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{walk_mut, VisitMut};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use rquickjs::{Context, Function, Object, Runtime, Value};
use serde::{Deserialize, Serialize};

use crate::services::text_transform::TextTransformMode;
use crate::utils::paths;

const SCRIPT_TIMEOUT: Duration = Duration::from_millis(1_000);
const SCRIPT_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const SCRIPT_MAX_STACK_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformContext {
    pub app_name: Option<String>,
    pub app_process: Option<String>,
    pub window_title: Option<String>,
    pub domain: Option<String>,
    pub mode_name: Option<String>,
    pub tone: Option<String>,
    pub raw_text: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredTransform {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub kind: String, // "builtin" | "script"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptingStatus {
    pub config_dir: String,
    pub config_path: String,
    pub config_exists: bool,
    pub valid: bool,
    pub error: Option<String>,
    pub script_transforms: Vec<RegisteredTransform>,
    pub all_transforms: Vec<RegisteredTransform>,
}

struct CachedConfig {
    mtime: Option<SystemTime>,
    len: u64,
    js_source: String,
    transforms: Vec<RegisteredTransform>,
    error: Option<String>,
}

static CONFIG_CACHE: parking_lot::Mutex<Option<CachedConfig>> = parking_lot::Mutex::new(None);

pub fn builtin_transforms() -> Vec<RegisteredTransform> {
    [
        (
            "lowercase",
            "Lowercase first letter",
            "Lowercase the first character of the transcript",
        ),
        (
            "uppercase",
            "UPPERCASE",
            "Convert entire output to uppercase",
        ),
        (
            "titleCase",
            "Title Case",
            "Capitalize the first character of every word",
        ),
        (
            "camelCase",
            "camelCase",
            "Convert words to camelCase identifier",
        ),
        (
            "snakeCase",
            "snake_case",
            "Convert words to snake_case identifier",
        ),
        (
            "kebabCase",
            "kebab-case",
            "Convert words to kebab-case identifier",
        ),
        (
            "stripTrailingPunctuation",
            "Strip trailing punctuation",
            "Remove trailing periods, commas, and sentence punctuation",
        ),
        (
            "codeWrap",
            "Inline code wrap",
            "Wrap output in Markdown backticks (`...`)",
        ),
        (
            "markdownList",
            "Markdown bullet",
            "Prefix output as a Markdown list item (- ...)",
        ),
    ]
    .into_iter()
    .map(|(id, label, desc)| RegisteredTransform {
        id: id.to_string(),
        label: label.to_string(),
        description: Some(desc.to_string()),
        kind: "builtin".to_string(),
    })
    .collect()
}

pub fn config_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("SPEAK_FLOW_CONFIG_DIR") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    paths::get_data_dir().join("scripts")
}

pub fn config_path() -> PathBuf {
    config_dir().join("speakflow.config.ts")
}

pub fn scaffold_config_files() -> Result<PathBuf, String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("Failed to create script config directory: {err}"))?;

    let dts_path = dir.join("speakflow.d.ts");
    let tsconfig_path = dir.join("tsconfig.json");
    let agents_path = dir.join("AGENTS.md");
    let main_path = config_path();

    std::fs::write(&dts_path, SPEAKFLOW_DTS)
        .map_err(|err| format!("Failed to write speakflow.d.ts: {err}"))?;
    if !tsconfig_path.exists() {
        std::fs::write(&tsconfig_path, TSCONFIG_JSON)
            .map_err(|err| format!("Failed to write tsconfig.json: {err}"))?;
    }
    if !agents_path.exists() {
        std::fs::write(&agents_path, SCRIPT_AGENTS_MD)
            .map_err(|err| format!("Failed to write AGENTS.md: {err}"))?;
    }
    if !main_path.exists() {
        std::fs::write(&main_path, DEFAULT_CONFIG_TS)
            .map_err(|err| format!("Failed to write speakflow.config.ts: {err}"))?;
    }
    invalidate_cache();
    Ok(main_path)
}

pub fn invalidate_cache() {
    *CONFIG_CACHE.lock() = None;
}

/// Inspect current scripting configuration, compile & validate, and list available transformations.
pub fn status() -> ScriptingStatus {
    let dir = config_dir();
    let path = config_path();
    let config_exists = path.is_file();

    let (transforms, error) = if config_exists {
        match load_compiled_config(&path) {
            Ok((t, err)) => (t, err),
            Err(err) => (Vec::new(), Some(err)),
        }
    } else {
        (Vec::new(), None)
    };

    let mut all_transforms = builtin_transforms();
    all_transforms.extend(transforms.iter().cloned());

    ScriptingStatus {
        config_dir: dir.to_string_lossy().to_string(),
        config_path: path.to_string_lossy().to_string(),
        config_exists,
        valid: error.is_none(),
        error,
        script_transforms: transforms,
        all_transforms,
    }
}

/// Apply an ordered list of transformation IDs (built-ins and/or QuickJS script transformations).
/// Script errors or timeouts are logged and safely retain the previous value rather than dropping output.
pub fn apply_transformations(
    text: &str,
    transform_ids: &[String],
    ctx: &TransformContext,
) -> String {
    if transform_ids.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let mut current = text.to_string();
    let path = config_path();
    let compiled = if path.is_file() {
        load_compiled_config(&path).ok().and_then(
            |(list, err)| {
                if err.is_none() {
                    Some(list)
                } else {
                    None
                }
            },
        )
    } else {
        None
    };

    for id in transform_ids {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        let builtin = TextTransformMode::parse(trimmed);
        if builtin != TextTransformMode::None {
            current = builtin.apply(&current);
            continue;
        }
        if compiled
            .as_ref()
            .is_some_and(|list| list.iter().any(|item| item.id == trimmed))
        {
            match run_script_transform(&path, trimmed, &current, ctx) {
                Ok(updated) => current = updated,
                Err(err) => {
                    log::warn!("Script transformation '{trimmed}' failed (keeping text): {err}");
                }
            }
        }
    }
    current
}

fn load_compiled_config(path: &Path) -> Result<(Vec<RegisteredTransform>, Option<String>), String> {
    let meta =
        std::fs::metadata(path).map_err(|err| format!("Cannot read {}: {err}", path.display()))?;
    let mtime = meta.modified().ok();
    let len = meta.len();

    {
        let cache = CONFIG_CACHE.lock();
        if let Some(entry) = cache.as_ref() {
            if entry.mtime == mtime && entry.len == len {
                return Ok((entry.transforms.clone(), entry.error.clone()));
            }
        }
    }

    let source = std::fs::read_to_string(path)
        .map_err(|err| format!("Cannot read {}: {err}", path.display()))?;
    let outcome = compile_and_probe_ts(&source);
    let (js_source, transforms, error) = match outcome {
        Ok((js, transforms)) => (js, transforms, None),
        Err(err) => (String::new(), Vec::new(), Some(err)),
    };

    *CONFIG_CACHE.lock() = Some(CachedConfig {
        mtime,
        len,
        js_source,
        transforms: transforms.clone(),
        error: error.clone(),
    });
    Ok((transforms, error))
}

fn compile_and_probe_ts(ts_source: &str) -> Result<(String, Vec<RegisteredTransform>), String> {
    let js_source = strip_typescript(ts_source)?;
    let transforms = probe_script_transforms(&js_source)?;
    Ok((js_source, transforms))
}

/// Strip TypeScript type annotations and exports into valid ES module/script JS for QuickJS.
pub fn strip_typescript(ts_source: &str) -> Result<String, String> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parsed = Parser::new(&allocator, ts_source, source_type).parse();
    if !parsed.errors.is_empty() {
        let first = parsed.errors[0].to_string();
        return Err(format!("TypeScript parse error: {first}"));
    }

    let mut collector = TypeSpanCollector {
        source: ts_source,
        edits: Vec::new(),
    };
    let mut program = parsed.program;
    collector.visit_program(&mut program);
    Ok(apply_span_edits(ts_source, collector.edits))
}

#[derive(Clone, Debug)]
struct SpanEdit {
    start: usize,
    end: usize,
    replacement: &'static str,
}

struct TypeSpanCollector<'a> {
    source: &'a str,
    edits: Vec<SpanEdit>,
}

impl<'a> TypeSpanCollector<'a> {
    fn blank(&mut self, span: Span) {
        self.edits.push(SpanEdit {
            start: span.start as usize,
            end: span.end as usize,
            replacement: "",
        });
    }

    fn space(&mut self, span: Span) {
        self.edits.push(SpanEdit {
            start: span.start as usize,
            end: span.end as usize,
            replacement: " ",
        });
    }

    fn strip_export_prefix(&mut self, span: Span, is_default: bool) {
        let slice = &self.source[span.start as usize..span.end as usize];
        if is_default {
            if let Some(pos) = slice.find("default") {
                let end = span.start as usize + pos + "default".len();
                self.edits.push(SpanEdit {
                    start: span.start as usize,
                    end,
                    replacement: "const __speakflowDefault =",
                });
            }
        } else if slice.starts_with("export") {
            let mut end = span.start as usize + "export".len();
            while end < self.source.len() && self.source.as_bytes()[end].is_ascii_whitespace() {
                end += 1;
            }
            self.edits.push(SpanEdit {
                start: span.start as usize,
                end,
                replacement: "",
            });
        }
    }
}

impl<'a, 'b> VisitMut<'b> for TypeSpanCollector<'a> {
    fn visit_statement(&mut self, stmt: &mut Statement<'b>) {
        match stmt {
            Statement::TSTypeAliasDeclaration(decl) => {
                self.blank(decl.span);
                return;
            }
            Statement::TSInterfaceDeclaration(decl) => {
                self.blank(decl.span);
                return;
            }
            Statement::TSImportEqualsDeclaration(decl) => {
                self.blank(decl.span);
                return;
            }
            Statement::ExportDefaultDeclaration(decl) => {
                let start = decl.span.start as usize;
                let expr_start = match &decl.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => f.span.start as usize,
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => c.span.start as usize,
                    _ => {
                        self.strip_export_prefix(decl.span, true);
                        walk_mut::walk_statement(self, stmt);
                        return;
                    }
                };
                if expr_start > start {
                    self.edits.push(SpanEdit {
                        start,
                        end: expr_start,
                        replacement: "const __speakflowDefault = ",
                    });
                }
            }
            Statement::ExportNamedDeclaration(decl) => {
                if decl.export_kind.is_type() {
                    self.blank(decl.span);
                    return;
                }
                if decl.declaration.is_some() {
                    self.strip_export_prefix(decl.span, false);
                } else {
                    self.blank(decl.span);
                    return;
                }
            }
            Statement::ExportAllDeclaration(decl) => {
                self.blank(decl.span);
                return;
            }
            Statement::ImportDeclaration(decl) => {
                // Imports are disallowed in the sandboxed runtime; type-only imports strip cleanly.
                if decl.import_kind.is_type() {
                    self.blank(decl.span);
                    return;
                }
            }
            _ => {}
        }
        walk_mut::walk_statement(self, stmt);
    }

    fn visit_ts_type_annotation(&mut self, it: &mut TSTypeAnnotation<'b>) {
        self.blank(it.span);
    }

    fn visit_ts_type_parameter_declaration(&mut self, it: &mut TSTypeParameterDeclaration<'b>) {
        self.blank(it.span);
    }

    fn visit_ts_type_parameter_instantiation(&mut self, it: &mut TSTypeParameterInstantiation<'b>) {
        self.blank(it.span);
    }

    fn visit_expression(&mut self, expr: &mut Expression<'b>) {
        match expr {
            Expression::TSAsExpression(inner) => {
                let start = oxc_span::GetSpan::span(&inner.expression).end as usize;
                let end = inner.span.end as usize;
                if end > start {
                    self.edits.push(SpanEdit {
                        start,
                        end,
                        replacement: "",
                    });
                }
            }
            Expression::TSSatisfiesExpression(inner) => {
                let start = oxc_span::GetSpan::span(&inner.expression).end as usize;
                let end = inner.span.end as usize;
                if end > start {
                    self.edits.push(SpanEdit {
                        start,
                        end,
                        replacement: "",
                    });
                }
            }
            Expression::TSNonNullExpression(inner) => {
                let start = oxc_span::GetSpan::span(&inner.expression).end as usize;
                let end = inner.span.end as usize;
                if end > start {
                    self.edits.push(SpanEdit {
                        start,
                        end,
                        replacement: "",
                    });
                }
            }
            Expression::TSTypeAssertion(inner) => {
                let start = inner.span.start as usize;
                let end = oxc_span::GetSpan::span(&inner.expression).start as usize;
                if end > start {
                    self.space(Span::new(start as u32, end as u32));
                }
            }
            _ => {}
        }
        walk_mut::walk_expression(self, expr);
    }
}

fn apply_span_edits(source: &str, mut edits: Vec<SpanEdit>) -> String {
    if edits.is_empty() {
        return source.to_string();
    }
    edits.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut non_overlapping: Vec<SpanEdit> = Vec::with_capacity(edits.len());
    let mut cursor = 0usize;
    for edit in edits {
        if edit.start >= cursor && edit.end <= source.len() && edit.start <= edit.end {
            cursor = edit.end;
            non_overlapping.push(edit);
        }
    }

    let mut out = String::with_capacity(source.len());
    let mut pos = 0usize;
    for edit in non_overlapping {
        if edit.start > pos {
            out.push_str(&source[pos..edit.start]);
        }
        out.push_str(edit.replacement);
        pos = edit.end;
    }
    if pos < source.len() {
        out.push_str(&source[pos..]);
    }
    out
}

fn make_sandboxed_runtime() -> Result<(Runtime, Instant), String> {
    let rt = Runtime::new().map_err(|err| format!("Failed to initialize QuickJS: {err}"))?;
    rt.set_memory_limit(SCRIPT_MEMORY_LIMIT_BYTES);
    rt.set_max_stack_size(SCRIPT_MAX_STACK_BYTES);
    let started = Instant::now();
    rt.set_interrupt_handler(Some(Box::new(move || started.elapsed() >= SCRIPT_TIMEOUT)));
    Ok((rt, started))
}

const PRELUDE_JS: &str = r#"
globalThis.__registeredTransforms = Object.create(null);
function defineConfig(config) {
  return config;
}
function registerTransformation(id, optionsOrFn, maybeFn) {
  if (typeof id !== "string" || !id.trim()) {
    throw new Error("registerTransformation requires a non-empty string id");
  }
  let fn = null;
  let label = id;
  let description = "";
  if (typeof optionsOrFn === "function") {
    fn = optionsOrFn;
  } else if (optionsOrFn && typeof optionsOrFn === "object") {
    label = typeof optionsOrFn.label === "string" && optionsOrFn.label.trim() ? optionsOrFn.label.trim() : id;
    description = typeof optionsOrFn.description === "string" ? optionsOrFn.description : "";
    fn = typeof optionsOrFn.transform === "function" ? optionsOrFn.transform : maybeFn;
  }
  if (typeof fn !== "function") {
    throw new Error("registerTransformation requires a transform function");
  }
  globalThis.__registeredTransforms[id.trim()] = { id: id.trim(), label, description, transform: fn };
}
const speakflow = { defineConfig, registerTransformation };
"#;

const COLLECT_TRANSFORMS_JS: &str = r#"
(() => {
  if (typeof __speakflowDefault === "object" && __speakflowDefault && __speakflowDefault.transformations) {
    for (const [id, entry] of Object.entries(__speakflowDefault.transformations)) {
      if (typeof entry === "function") {
        registerTransformation(id, entry);
      } else if (entry && typeof entry === "object") {
        registerTransformation(id, entry);
      }
    }
  }
  return Object.values(globalThis.__registeredTransforms).map((item) => ({
    id: item.id,
    label: item.label,
    description: item.description || null,
    kind: "script"
  }));
})()
"#;

fn probe_script_transforms(js_source: &str) -> Result<Vec<RegisteredTransform>, String> {
    let (rt, _) = make_sandboxed_runtime()?;
    let ctx = Context::full(&rt).map_err(|err| format!("QuickJS context failed: {err}"))?;
    ctx.with(|ctx| {
        ctx.eval::<(), _>(PRELUDE_JS)
            .map_err(|err| format!("Prelude evaluation failed: {err}"))?;
        ctx.eval::<(), _>(js_source)
            .map_err(|err| format_quickjs_error(&ctx, err))?;
        let val: Value = ctx
            .eval(COLLECT_TRANSFORMS_JS)
            .map_err(|err| format_quickjs_error(&ctx, err))?;
        let json = ctx
            .json_stringify(val)
            .map_err(|err| format!("Failed to serialize script transforms: {err}"))?
            .ok_or_else(|| "Transform registry returned undefined".to_string())?;
        let raw = json
            .to_string()
            .map_err(|err| format!("Invalid UTF-8 from QuickJS: {err}"))?;
        serde_json::from_str::<Vec<RegisteredTransform>>(&raw)
            .map_err(|err| format!("Invalid transform descriptor list: {err}"))
    })
}

fn run_script_transform(
    path: &Path,
    transform_id: &str,
    input_text: &str,
    transform_ctx: &TransformContext,
) -> Result<String, String> {
    load_compiled_config(path)?;
    let js_source = {
        let cache = CONFIG_CACHE.lock();
        let entry = cache
            .as_ref()
            .ok_or_else(|| "Config cache unavailable".to_string())?;
        if let Some(err) = entry.error.as_ref() {
            return Err(err.clone());
        }
        entry.js_source.clone()
    };

    let (rt, _) = make_sandboxed_runtime()?;
    let ctx = Context::full(&rt).map_err(|err| format!("QuickJS context failed: {err}"))?;
    let ctx_json = serde_json::to_string(transform_ctx)
        .map_err(|err| format!("Failed to serialize TransformContext: {err}"))?;

    ctx.with(|ctx| {
        ctx.eval::<(), _>(PRELUDE_JS)
            .map_err(|err| format!("Prelude evaluation failed: {err}"))?;
        ctx.eval::<(), _>(js_source.as_str())
            .map_err(|err| format_quickjs_error(&ctx, err))?;
        ctx.eval::<(), _>(COLLECT_TRANSFORMS_JS)
            .map_err(|err| format_quickjs_error(&ctx, err))?;

        let globals = ctx.globals();
        let registry: Object = globals
            .get("__registeredTransforms")
            .map_err(|err| format!("Missing transform registry: {err}"))?;
        let entry: Object = registry
            .get(transform_id)
            .map_err(|err| format!("Transform '{transform_id}' not found: {err}"))?;
        let func: Function = entry
            .get("transform")
            .map_err(|err| format!("Transform '{transform_id}' missing function: {err}"))?;
        let parsed_ctx = ctx
            .json_parse(ctx_json)
            .map_err(|err| format!("Failed to build QuickJS context object: {err}"))?;
        let result: Value = func
            .call((input_text, parsed_ctx))
            .map_err(|err| format_quickjs_error(&ctx, err))?;

        if result.is_undefined() || result.is_null() {
            return Ok(input_text.to_string());
        }
        result
            .as_string()
            .and_then(|s| s.to_string().ok())
            .ok_or_else(|| format!("Transform '{transform_id}' must return a string"))
    })
}

fn format_quickjs_error<'js>(ctx: &rquickjs::Ctx<'js>, err: rquickjs::Error) -> String {
    if matches!(err, rquickjs::Error::Exception) {
        let exc = ctx.catch();
        if let Some(obj) = exc.as_object() {
            if let Ok(msg) = obj.get::<_, String>("message") {
                return format!("Script error: {msg}");
            }
        }
        if let Some(s) = exc.as_string().and_then(|v| v.to_string().ok()) {
            return format!("Script error: {s}");
        }
    }
    format!("Script execution error: {err}")
}

pub fn read_config_source() -> Result<String, String> {
    let path = config_path();
    if !path.is_file() {
        scaffold_config_files()?;
    }
    std::fs::read_to_string(&path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))
}

pub fn save_config_source(source: &str) -> Result<ScriptingStatus, String> {
    let path = config_path();
    if !path.exists() {
        scaffold_config_files()?;
    }
    compile_and_probe_ts(source)?;
    paths::atomic_write(&path, source.as_bytes())
        .map_err(|err| format!("Failed to save {}: {err}", path.display()))?;
    invalidate_cache();
    Ok(status())
}

pub fn test_config_source(
    source: &str,
    transform_ids: &[String],
    input_text: &str,
    ctx: &TransformContext,
) -> Result<String, String> {
    let js_source = strip_typescript(source)?;
    let script_transforms = probe_script_transforms(&js_source)?;
    let mut current = input_text.to_string();
    for id in transform_ids {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        let builtin = TextTransformMode::parse(trimmed);
        if builtin != TextTransformMode::None {
            current = builtin.apply(&current);
            continue;
        }
        if script_transforms.iter().any(|t| t.id == trimmed) {
            let (rt, _) = make_sandboxed_runtime()?;
            let qctx =
                Context::full(&rt).map_err(|err| format!("QuickJS context failed: {err}"))?;
            let ctx_json = serde_json::to_string(ctx)
                .map_err(|err| format!("Failed to serialize TransformContext: {err}"))?;
            current = qctx.with(|qctx| {
                qctx.eval::<(), _>(PRELUDE_JS)
                    .map_err(|err| format!("Prelude evaluation failed: {err}"))?;
                qctx.eval::<(), _>(js_source.as_str())
                    .map_err(|err| format_quickjs_error(&qctx, err))?;
                qctx.eval::<(), _>(COLLECT_TRANSFORMS_JS)
                    .map_err(|err| format_quickjs_error(&qctx, err))?;
                let globals = qctx.globals();
                let registry: Object = globals
                    .get("__registeredTransforms")
                    .map_err(|err| format!("Missing transform registry: {err}"))?;
                let entry: Object = registry
                    .get(trimmed)
                    .map_err(|err| format!("Transform '{trimmed}' not found: {err}"))?;
                let func: Function = entry
                    .get("transform")
                    .map_err(|err| format!("Transform '{trimmed}' missing function: {err}"))?;
                let parsed_ctx = qctx
                    .json_parse(ctx_json)
                    .map_err(|err| format!("Failed to build QuickJS context object: {err}"))?;
                let result: Value = func
                    .call((current.as_str(), parsed_ctx))
                    .map_err(|err| format_quickjs_error(&qctx, err))?;
                if result.is_undefined() || result.is_null() {
                    return Ok(current.clone());
                }
                result
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .ok_or_else(|| format!("Transform '{trimmed}' must return a string"))
            })?;
        }
    }
    Ok(current)
}

const SPEAKFLOW_DTS: &str = r#"export interface TransformContext {
  appName: string | null;
  appProcess: string | null;
  windowTitle: string | null;
  domain: string | null;
  modeName: string | null;
  tone: string | null;
  rawText: string;
  language: string | null;
}

export type TransformFn = (text: string, ctx: TransformContext) => string;

export interface TransformDefinition {
  label?: string;
  description?: string;
  transform: TransformFn;
}

export interface SpeakFlowConfig {
  transformations?: Record<string, TransformFn | TransformDefinition>;
}

declare global {
  function defineConfig(config: SpeakFlowConfig): SpeakFlowConfig;
  function registerTransformation(
    id: string,
    definition: TransformFn | TransformDefinition,
  ): void;
}
"#;

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "strict": true,
    "noEmit": true
  },
  "include": ["speakflow.config.ts", "speakflow.d.ts"]
}
"#;

const SCRIPT_AGENTS_MD: &str = r#"# SpeakFlow Scripting Guide

`speakflow.config.ts` runs inside an isolated, synchronous QuickJS sandbox after
transcription and AI polish.

- Register custom transforms via `export default defineConfig({ transformations: { ... } })`
  or `registerTransformation("id", { label, transform })`.
- No filesystem, network, or timer APIs exist inside the sandbox (`1000ms` timeout, `64 MiB` memory cap).
- Throwing or exceeding limits keeps the previous text cleanly.
"#;

const DEFAULT_CONFIG_TS: &str = r#"/// <reference path="./speakflow.d.ts" />

export default defineConfig({
  transformations: {
    "arrow-bullet": {
      label: "Arrow bullet",
      description: "Prefix each non-empty line with ->",
      transform(text: string): string {
        return text
          .split("\n")
          .map((line: string) => (line.trim() ? `-> ${line.trim()}` : line))
          .join("\n");
      },
    },
  },
});
"#;

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let unique = format!(
                "speakflow_{prefix}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_ctx() -> TransformContext {
        TransformContext {
            app_name: Some("VS Code".into()),
            app_process: Some("Code.exe".into()),
            window_title: Some("main.rs".into()),
            domain: None,
            mode_name: Some("Coding".into()),
            tone: None,
            raw_text: "hello world".into(),
            language: Some("en".into()),
        }
    }

    #[test]
    fn strips_ts_and_executes_sandboxed_transform() {
        let dir = TestDir::new("script_ok");
        let path = dir.0.join("speakflow.config.ts");
        let source = r#"
interface CustomMeta { prefix: string; }
type Suffix = string;
const meta: CustomMeta = { prefix: "[SF] " };
export default defineConfig({
  transformations: {
    tagLine: {
      label: "Tag Line",
      transform(text: string, ctx: TransformContext): string {
        const app = (ctx.appName ?? "App") as Suffix;
        return `${meta.prefix}${app}: ${text.trim()}`;
      }
    }
  }
});
"#;
        std::fs::write(&path, source).unwrap();
        invalidate_cache();

        let (transforms, err) = load_compiled_config(&path).unwrap();
        assert_eq!(err, None);
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].id, "tagLine");
        assert_eq!(transforms[0].label, "Tag Line");

        let out = run_script_transform(&path, "tagLine", "deploy now", &sample_ctx()).unwrap();
        assert_eq!(out, "[SF] VS Code: deploy now");
    }

    #[test]
    fn infinite_loop_hits_interrupt_timeout() {
        let dir = TestDir::new("script_hang");
        let path = dir.0.join("speakflow.config.ts");
        std::fs::write(
            &path,
            r#"registerTransformation("hang", () => { while (true) {} });"#,
        )
        .unwrap();
        invalidate_cache();

        let result = run_script_transform(&path, "hang", "safe text", &sample_ctx());
        assert!(result.is_err());
    }
}
