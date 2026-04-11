# Kelora Vision & Design

Kelora is a **programmable, scriptable log processor** for real-world logs, fast pipelines, and complete control. It is a **CLI-first** tool for turning messy, diverse logs into structured, analyzable events — with scripting, formatting, and logic *you* own.

This document defines what Kelora is, what it is not, and the design constraints that keep it focused.

---

## ✅ WHAT KELORA IS

* A **CLI tool** for transforming logs into structured events
* A **streaming pipeline**: Input → Parsing → Processing (Rhai) → Output
* A **scriptable processor** for filtering, mutation, enrichment, and event shaping
* A tool that supports batch and live-stream workflows (stdin/files)
* A system with explicit resiliency modes: resilient default, strict fail-fast
* A tool for composable automation in CI and shell pipelines

---

## ❌ WHAT KELORA IS NOT

| Not a… | Why not | Use instead |
|---|---|---|
| Log shipper | Kelora processes logs; it does not transport/route them | fluent-bit, vector, filebeat |
| Dashboard / visualization stack | Kelora does not store/query/visualize at scale | Grafana, Kibana, Loki |
| Metrics/time-series database | Kelora computes during processing; it is not persistent metrics storage | Prometheus, ClickHouse, InfluxDB |
| Full-text search/indexing engine | Kelora is event processing, not indexing/search infra | ripgrep, Elasticsearch |
| General regex replacement toolkit | Regex exists here as parsing support, not core product identity | grep, ripgrep |
| Interactive log browser UI | Kelora has a REPL for command ergonomics, not an ncurses/GUI log viewer | lnav |

---

## 🔒 SCOPE DISCIPLINE

### Hard boundaries

* **No TUI/GUI product surface**: keep focus on scriptable CLI behavior
* **No dynamic plugin runtime** (`.so`/WASM marketplace model)
* **No hidden magic**: behaviors must be explicit and predictable
* **No silent error swallowing by default**: diagnostics stay visible unless explicitly suppressed
* **No implicit type coercion policy**: conversions should remain explicit and auditable

### Allowed with guardrails

* **Interactive REPL mode** (`kelora` with no args): for command entry/helpfulness, not for visual exploration workflows
* **Format auto-detection** only via explicit auto modes (`auto`, `auto-per-file`) with deterministic fallback behavior
* **Multiline handling** only via explicit multiline configuration and deterministic chunking strategies
* **Parallel processing** with explicit ordering and batching controls
* **Streaming decompression** for gzip/zstd inputs via explicit input usage (magic-byte detection)

---

## 🧠 DESIGN PRINCIPLES

| Area | Principle |
|---|---|
| Core identity | Scriptable CLI log processor, not a platform |
| Event model | Each input unit becomes a structured event with typed values |
| Core fields | `ts`, `level`, `msg` are first-class and consistently handled |
| Error model | Resilient by default; strict mode for fail-fast correctness |
| Type model | Explicit conversion over implicit coercion |
| Performance model | Configurable parallelism, ordering, and batching |
| Output model | Multiple output formats; keep stdout data-clean and stderr diagnostic |
| UX model | Predictable flags, composable operations, shell/CI-friendly behavior |

---

## 🧱 EVENT & PIPELINE MODEL

Each event contains structured fields plus internal metadata needed for processing/tracking.

The execution path is intentionally simple:

1. **Input**: stdin/files (including compressed streams)
2. **Parsing**: selected format parser (or explicit auto mode)
3. **Processing**: Rhai filters/transforms + built-in functions
4. **Output**: formatter and diagnostics according to selected flags

---

## 🧪 RESILIENCY MODEL

* **Resilient mode (default)**: continue processing and report issues clearly
* **Strict mode**: abort on first relevant parse/runtime failure
* Diagnostics go to stderr; valid event output goes to stdout
* Suppression flags exist, but are explicit and intentional

---

## 🔧 CLI PRINCIPLES

* One flag, one responsibility
* Prefer composability over broad, implicit behavior
* Avoid surprise defaults that hide correctness issues
* Keep behavior script-friendly and machine-consumable
* Maintain stable semantics and minimize breaking changes

---

## ✨ KELORA IS GREAT FOR

* CI log processing and regression signal extraction
* Converting semi-structured logs into structured records
* Stateful/logic-rich filtering and enrichment with Rhai
* Pipeline-friendly one-liners and repeatable automation
* Fast forensic triage on local files or live streams

---

## 📌 SUMMARY TAGLINE

**Kelora is a scriptable, CLI-first log processor for real-world logs.**

Structured in, structured out. Predictable behavior, explicit control, production-friendly ergonomics.
