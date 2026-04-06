# Rust WASM Array Search Engine — Developer Guide

This comprehensive guide covers the WASM search engine architecture, APIs, and usage patterns for JavaScript, React, and Salesforce LWC.

---

## 1. Overview

The engine is a high-performance, in-memory search layer over arrays of JSON objects. It supports:
- Full-text search with operators (AND, OR, NOT, LIKE, FUZZY, REGEX)
- Ordering, pagination, and projections
- Aggregations (COUNT, SUM, AVG, MIN, MAX, DISTINCT)
- Adaptive caching for repeat queries
- Zero-copy mode for large datasets

---

## 2. Architecture

### 2.1 Core Concepts

```
┌─────────────────────────────────────────────────────────────────┐
│                        Query Flow                                │
├─────────────────────────────────────────────────────────────────┤
│  Query String ──► Parse ──► Plan ──► Evaluate ──► Results     │
│                       │         │         │                     │
│                       ▼         ▼         ▼                     │
│                   AST Cache   Index    Columnar                 │
│                               Selection  View                   │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 Memory Models

| Model | JS Heap | WASM Memory | Best For |
|-------|---------|-------------|----------|
| Full Copy | ✅ | ✅ | Small datasets (<50k) |
| Zero-Copy | ❌ | ✅ | Large datasets (500k-1M+) |

**Zero-Copy**: Pass decompressed bytes directly to WASM - no JS string copy.

---

## 3. API Reference

### 3.1 Initialization Functions

| Function | Input | Returns | Use Case |
|----------|-------|---------|----------|
| `init_engine(items_json)` | JSON String | handle | Simple init |
| `init_engine_with_options(items_json, options)` | JSON + Config | handle | Full features |
| `init_engine_from_bytes(bytes, options)` | Vec\<u8\> + Config | handle | **Zero-copy** |
| `execute_query_json(items_json, query)` | JSON + Query | results | One-shot |

```javascript
// Full Copy (legacy)
const handle = wasm.init_engine(JSON.stringify(data));

// Zero-Copy (recommended for large data)
const jsonBytes = gzipWasm.gzip_decompress_bytes(gzBytes);
const handle = wasm.init_engine_from_bytes(jsonBytes, options);
```

### 3.2 Query Functions

| Function | Returns | Pagination | Best For |
|----------|---------|-------------|----------|
| `execute_query(handle, query)` | JSON array | No | Small results |
| `execute_query_paged(handle, query)` | `{total_matches, rows}` | **Yes** | Large datasets |
| `execute_query_indices(handle, query)` | Array of indices | No | Custom pagination |

```javascript
// Paged query - returns total_matches + page of rows
const result = wasm.execute_query_paged(handle, "country = 'India' LIMIT 10 OFFSET 20");
const { total_matches, rows } = JSON.parse(result);
// total_matches = 2457, rows = [10 items]
```

### 3.3 Data Management

```javascript
// Get row count and size (cached, O(1))
const info = JSON.parse(wasm.get_engine_data_size(handle));
// { row_count: 1000000, approx_bytes: 145678900 }

// Drop engine to free memory
wasm.drop_engine(handle);

// Get cache statistics
const stats = wasm.engine_cache_stats(handle);
// { hits: 15, misses: 5, entries: 3, cap: 128 }
```

### 3.4 Aggregations

```javascript
// COUNT by country
const spec = { group_by: ["country"], aggs: [{ op: "COUNT", field: "*", alias: "count" }] };
const rows = wasm.aggregate_handle(handle, JSON.stringify(spec));

// AVG price by category
const spec = { group_by: ["category"], aggs: [{ op: "AVG", field: "price", alias: "avg_price" }] };

// DISTINCT countries
const spec = { distinct_fields: ["country"] };
```

---

## 4. Engine Options

```javascript
const options = {
  indexes: ["country", "category", "tags", "active"],
  query_cache_cap: 200,
  columnar: true,
  columnar_fields: ["country", "category", "name", "price"]
};
const handle = wasm.init_engine_with_options(JSON.stringify(data), JSON.stringify(options));
```

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `indexes` | Vec\<String\> | auto | Fields to index for fast filtering |
| `query_cache_cap` | usize | 128 | Max cached queries |
| `columnar` | bool | false | Enable columnar view |
| `columnar_fields` | Vec\<String\> | [] | Fields for columnar storage |

---

## 5. Query Language Reference

### 5.1 Logical Operators
```
country = "India" AND category = "software"
country = "USA" OR country = "UK"
NOT active = false
```

### 5.2 Comparisons
```
salary > 100000
price >= 500 AND price <= 1000
name != "Test"
```

### 5.3 IN / NOT IN
```
tags IN ("fiber", "router", "network")
country NOT IN ("USA", "UK")
```

### 5.4 LIKE / FUZZY / REGEX
```
name LIKE "%router%"
name FUZZY "NetCore"        // Tolerates typos
email REGEX ".*@domain.com"
```

### 5.5 BETWEEN
```
salary BETWEEN 50000 AND 100000
price BETWEEN (100, 500)
```

### 5.6 Nested Paths
```
meta.region = "APAC"
tags.0 = "fiber"
```

### 5.7 Ordering & Pagination
```
ORDER BY price DESC, salary ASC
LIMIT 25 OFFSET 50
ORDER BY SCORE DESC           // BM25 scoring
```

### 5.8 Projection (SELECT)
```
SELECT name, price, country WHERE category = "network"
```

### 5.9 Mode Flags
```
CASE SENSITIVE
STRICT                         // Exact type matching
```

---

## 6. Caching Strategy

### 6.1 How It Works

The engine implements **adaptive result caching**:

1. **First query**: Execute scan, cache indices after ~2 hits
2. **Subsequent queries**: Return cached indices instantly
3. **Adaptive**: If latency >50ms, cache after 1 hit

### 6.2 Cache Key

```javascript
// Query without LIMIT/OFFSET becomes cache key
"country = 'India' LIMIT 10 OFFSET 0"  →  key: "country = 'India'"
"country = 'India' LIMIT 10 OFFSET 10"  →  key: "country = 'India'" (CACHE HIT!)
```

This enables **instant pagination** - same filter, different page = cache hit.

### 6.3 Cache Stats

```javascript
const stats = wasm.engine_cache_stats(handle);
// { hits: 10, misses: 2, entries: 3, cap: 128 }
```

---

## 7. Performance

### 7.1 Expected Performance (1M Rows)

| Operation | Time | Notes |
|-----------|------|-------|
| Init (zero-copy) | ~2s | Decompress + parse |
| First query | ~500ms | Full scan |
| Cached query | <1ms | Instant |
| Paged query (cached) | <1ms | Instant |
| Metrics refresh | O(1) | Cached size |

### 7.2 Optimization Tips

1. **Use zero-copy** for datasets >100k rows
2. **Build indexes** on frequently filtered fields
3. **Enable columnar** for large scans
4. **Keep queries consistent** to enable cache hits
5. **Use LIMIT** to reduce payload

---

## 8. Build & Deploy

### 8.1 Build WASM

```powershell
wasm-pack build --target web --out-dir pkg
```

### 8.2 Bundle for LWC (IIFE)

```powershell
node node_modules/rollup/dist/bin/rollup pkg/search_wasm.js --format iife --name searchWasm --file rollup_lwc/search_wasm_iife.js
```

### 8.3 Deploy to Salesforce

Copy to Static Resource (`search_wasm_pkg`):
- `search_wasm_iife.js`
- `search_wasm_bg.wasm`

---

## 9. Usage Examples

### 9.1 Vanilla JS (ESM)

```javascript
import init, { init_engine, execute_query, drop_engine } from "./pkg/search_wasm.js";

await init();
const handle = init_engine(JSON.stringify(data));

const result = execute_query(handle, 'country = "India" ORDER BY salary DESC');
const rows = JSON.parse(result);

drop_engine(handle);
```

### 9.2 Zero-Copy (with gzip)

```javascript
// Decompress then init - no JS string copy
const jsonBytes = gzipWasm.gzip_decompress_bytes(gzBytes);
const handle = wasm.init_engine_from_bytes(jsonBytes, options);
```

### 9.3 Salesforce LWC

```javascript
import { LightningElement } from "lwc";
import { loadScript } from "lightning/platformResourceLoader";
import WASM_PKG from "@salesforce/resourceUrl/search_wasm_pkg";

export default class WasmSearch extends LightningElement {
  async connectedCallback() {
    await loadScript(this, `${WASM_PKG}/search_wasm_iife.js`);
    const bytes = await (await fetch(`${WASM_PKG}/search_wasm_bg.wasm`)).arrayBuffer();
    await globalThis.searchWasm.default(bytes);
    this.wasm = globalThis.searchWasm;
  }

  runSearch() {
    const result = this.wasm.execute_query_paged(this.handle, "country = 'India' LIMIT 10");
    const { total_matches, rows } = JSON.parse(result);
  }
}
```

### 9.4 React

```jsx
import { useEffect, useState } from "react";
import init, { init_engine, execute_query } from "./pkg/search_wasm.js";

function App() {
  const [ready, setReady] = useState(false);
  
  useEffect(() => { init().then(() => setReady(true)); }, []);
  
  const search = () => {
    const handle = init_engine(JSON.stringify(data));
    const result = execute_query(handle, 'country = "USA"');
    // ...
  };
  
  return <button onClick={search} disabled={!ready}>Search</button>;
}
```

---

## 10. Safety Limits

| Limit | Value | Purpose |
|-------|-------|---------|
| MAX_QUERY_LEN | 8192 | Prevent huge queries |
| MAX_NESTING_DEPTH | 100 | Prevent stack overflow |
| MAX_REGEX_LEN | 256 | ReDoS protection |
| MAX_RESULT_ROWS | 100,000 | Limit result size |

---

## 11. Troubleshooting

| Issue | Solution |
|-------|----------|
| No results | Check string quotes: `country = "India"` |
| WASM not loading | Call `init()` before queries |
| Slow queries | Build indexes, enable columnar |
| Memory issues | Use zero-copy mode (`init_engine_from_bytes`) |
| Cache not working | Query must have same filter (LIMIT/OFFSET stripped) |

---

## 12. API Summary

```
Initialization:
  init_engine(items_json) → handle
  init_engine_with_options(items_json, options) → handle
  init_engine_from_bytes(bytes, options) → handle
  execute_query_json(items_json, query) → results

Querying:
  execute_query(handle, query) → JSON array
  execute_query_paged(handle, query) → {total_matches, rows}
  execute_query_indices(handle, query) → indices array

Management:
  drop_engine(handle) → void
  get_engine_data_size(handle) → {row_count, approx_bytes}
  engine_cache_stats(handle) → {hits, misses, entries, cap}
  validate_query(query) → {ok, error}

Aggregations:
  aggregate_handle(handle, spec_json) → results
  aggregate_handle_json(handle, spec_json) → results

Metrics:
  get_metrics(handle) → {query_count, avg_latency, p95, etc.}
```
