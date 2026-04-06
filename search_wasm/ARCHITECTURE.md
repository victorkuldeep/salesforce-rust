# Architecture Overview — Rust WASM Array Search Engine

This document captures the architecture, optimization techniques, and consumer patterns for the zero-copy WASM search engine designed for Salesforce LWC.

## 1. System Overview

The engine is a lightweight, read‑only search layer over an **array of JSON objects**. It is not a relational database; there is no FROM or JOIN. The dataset is supplied in‑memory (JS → WASM), and queries run locally in WASM.

High‑level flow:

1. **Tokenize** query string
2. **Parse** into AST
3. **Plan** (index selection + options)
4. **Evaluate** against dataset
5. **Return** JSON array of matches

---

## 2. Consumer Patterns & Engine Initialization

The library supports **multiple consumption patterns** depending on data source and use case:

### Pattern A: Full Copy (Legacy - wasmSearchDemo)
```js
// JS has full copy of data - works but uses more memory
data = JSON.parse(jsonString);           // JS heap copy
handle = wasm.init_engine_with_options(JSON.stringify(data), options);
```
- **Use case**: Small datasets (<50k rows), simple demos
- **Memory**: Data in both JS heap + WASM linear memory (2x)
- **Pros**: Simple, familiar API
- **Cons**: Highest memory usage

### Pattern B: Zero-Copy via Bytes (Current - wasmAdvanceDemo)
```js
// Zero-copy: JS passes bytes, WASM parses internally
jsonBytes = gzipWasm.gzip_decompress_bytes(gzBytes);  // WASM memory
handle = wasm.init_engine_from_bytes(jsonBytes, options);
```
- **Use case**: Large datasets (500k-1M+ rows), production LWC
- **Memory**: Only WASM linear memory, JS heap has ZERO data copy
- **Pros**: ~50% less JS heap usage, best for large data
- **Cons**: Requires gzip preprocessing

### Pattern C: One-Shot (No Handle)
```js
// No engine handle - one-shot query, no persistence
result = wasm.execute_query_json(itemsJsonString, query);
```
- **Use case**: Single queries, stateless operations
- **Memory**: Transient, no engine state
- **Pros**: No engine lifecycle management
- **Cons**: Can't reuse indexes, slower for repeat queries

### Pattern D: Handle-Based with Options (Most Flexible)
```js
handle = wasm.init_engine_with_options(itemsJsonString, optionsJson);
```
- **Use case**: Full control over indexes, columnar mode, cache
- **Pros**: All features available
- **Cons**: Requires JSON string in JS

---

## 3. Engine API Reference

### Initialization Functions

| Function | Input Type | Memory Model | Best For |
|----------|------------|--------------|----------|
| `init_engine(items_json)` | JSON String | Full copy | Legacy/simple |
| `init_engine_with_options(items_json, options)` | JSON String + Config | Full copy | Full features |
| `init_engine_from_bytes(bytes, options)` | Vec\<u8\> | **Zero-copy** | Large datasets |
| `execute_query_json(items_json, query)` | JSON String | Transient | One-shot |

### Query Functions

| Function | Returns | Pagination | Best For |
|----------|---------|-------------|----------|
| `execute_query(handle, query)` | All matches | No (returns all) | Small results |
| `execute_query_paged(handle, query)` | `{total_matches, rows}` | Yes (LIMIT/OFFSET) | Large datasets |
| `execute_query_indices(handle, query)` | Array of indices | No | Custom pagination |

### Data Management

| Function | Purpose |
|----------|---------|
| `update_engine(handle, items_json)` | Replace dataset in existing handle |
| `drop_engine(handle)` | Free WASM memory |
| `get_engine_data_size(handle)` | Get row count + size (cached) |

---

## 4. Performance Optimizations (v1.1.0+)

### 4.1 Cached Byte Size
**Problem**: `get_engine_data_size()` serialized entire dataset on every call (O(n)).

**Solution**: Store `approx_bytes` at engine init time. Metrics now O(1).

```rust
// Engine struct now includes:
approx_bytes: usize,  // Calculated once at init

// Instead of O(n) serialization:
let approx_bytes = engine.data.iter()
    .map(|v| serde_json::to_string(v).map(|s| s.len()).unwrap_or(0))
    .sum();

// Now just:
let approx_bytes = engine.approx_bytes;  // O(1)
```

**Impact**: ~100x faster for metrics refresh on 1M rows.

### 4.2 Smart Count for Small LIMITs
**Problem**: `execute_query_paged` did double scan for non-ORDER BY queries.

**Solution**: For small LIMITs (≤100), skip full count and use scanned as estimate.

```rust
let limit = parsed.limit.unwrap_or(PAGE_SIZE_DEFAULT);
let should_skip_full_count = parsed.order_by.is_empty() && limit <= 100;

if should_skip_full_count {
    // Use scanned result - sufficient for pagination
    total = scanned.saturating_add(parsed.offset.unwrap_or(0));
} else {
    // Full count for accuracy
    total = count_all_matches(...);
}
```

**Impact**: ~50% faster query execution for paged UI queries.

### 4.3 Memory Architecture

```
Pattern A (Full Copy):
┌─────────────────────┐     ┌─────────────────────────────┐
│   JS Heap           │     │   WASM Linear Memory         │
│  ┌───────────────┐  │     │  ┌────────────────────────┐  │
│  │ data (JSON)   │──┼─────►│  │ data (Vec<Value>)      │  │
│  └───────────────┘  │     │  │ indexes                 │  │
│                     │     │  │ columnar_store          │  │
└─────────────────────┘     │  │ text_index              │  │
                           │  └────────────────────────┘  │
                           └─────────────────────────────┘

Pattern B (Zero-Copy):
┌─────────────────────┐     ┌─────────────────────────────┐
│   JS Heap           │     │   WASM Linear Memory         │
│  (NO DATA COPY!)    │     │  ┌────────────────────────┐  │
│                     │     │  │ data (Vec<Value>)      │  │
└─────────────────────┘     │  │ indexes                 │  │
                           │  │ columnar_store          │  │
                           │  └────────────────────────┘  │
                           └─────────────────────────────┘
```

---

## 5. Query Language

### 5.1 Predicates
- Boolean: `AND`, `OR`, `NOT`
- Comparisons: `=`, `!=`, `>`, `>=`, `<`, `<=`
- IN / NOT IN (arrays or scalars)
- LIKE / NOT LIKE (`%`, `_`)
- CONTAINS / STARTS WITH / ENDS WITH
- BETWEEN (two syntaxes)
- REGEX / NOT REGEX
- FUZZY / NOT FUZZY (Damerau‑Levenshtein)

### 5.2 Clauses
- Nested paths: `meta.region`, `tags.0`
- Ordering: `ORDER BY price DESC, salary ASC`
- Pagination: `LIMIT 25 OFFSET 50`
- Projection: `SELECT name, price`
- Modes: `CASE SENSITIVE`, `STRICT`

---

## 6. Indexing & Caching

### 6.1 User-Defined Indexes
- Pass array of index fields: `["country", "category", "tags"]`
- Index is HashMap\<value → row_ids\>
- Used for candidate filtering before evaluation

### 6.2 Query Cache (LRU)
- Parsed AST cached by normalized query string
- Configurable capacity: `set_query_cache_size(200)`
- Adaptive result cache for repeat queries

---

## 7. Safety Limits

| Limit | Value | Purpose |
|-------|-------|---------|
| MAX_QUERY_LEN | 8192 | Prevent huge queries |
| MAX_NESTING_DEPTH | 100 | Prevent stack overflow |
| MAX_REGEX_LEN | 256 | ReDoS protection |
| MAX_RESULT_ROWS | 100,000 | Prevent huge result sets |
| PAGE_SIZE_DEFAULT | 25 | Default pagination size |

---

## 8. LWC Integration Guide

### 8.1 Static Resource Setup
Two files required in static resource:
```
search_wasm_pkg/
├── search_wasm_iife.js   (IIFE bundle)
└── search_wasm_bg.wasm   (WASM binary)
```

### 8.2 Loading in LWC
```js
import { loadScript } from "lightning/platformResourceLoader";
import WASM_PKG from "@salesforce/resourceUrl/search_wasm_pkg";

await loadScript(this, `${WASM_PKG}/search_wasm_iife.js`);
const wasmBytes = await (await fetch(`${WASM_PKG}/search_wasm_bg.wasm`)).arrayBuffer();
await globalThis.searchWasm.default(wasmBytes);
```

### 8.3 LWC Components

| Component | Pattern | Data Source |
|-----------|---------|-------------|
| `wasmSearchDemo` | Pattern A | Full copy, small data |
| `wasmAdvanceDemo` | Pattern B | Zero-copy, large data |

---

## 9. Build & Deploy

### Build Commands
```bash
# Step 1: Build WASM
wasm-pack build --target web --out-dir pkg

# Step 2: Bundle IIFE for LWC
node node_modules/rollup/dist/bin/rollup pkg/search_wasm.js --format iife --name searchWasm --file rollup_lwc/search_wasm_iife.js

# Step 3: Deploy to Salesforce
# Copy rollup_lwc/search_wasm_iife.js → static resource
# Copy pkg/search_wasm_bg.wasm → static resource
```

---

## 10. Performance Characteristics

### Expected Performance (1 Million Rows)

| Operation | Before | After | Improvement |
|-----------|--------|-------|-------------|
| Metrics refresh | O(n) serialize | O(1) cached | ~100x |
| Paged query (LIMIT 25) | 2 scans | 1 scan | ~50% |
| Memory (JS heap) | Full copy | Zero-copy | ~50% less |
| Init time (1M rows) | ~2s | ~2s | Same |

---

## 11. Roadmap

### Immediate
- [x] Zero-copy init via bytes
- [x] Cached byte size
- [x] Smart count optimization

### Next
- [ ] Projection pushdown (build only requested fields)
- [ ] Streaming JSON parse (true zero-copy from gzip)
- [ ] Rank-aware top-K (skip full sort for large ORDER BY)

### Future
- [ ] Cost-based index selection
- [ ] Parallel scan (WebWorkers)
- [ ] Indexed field statistics
