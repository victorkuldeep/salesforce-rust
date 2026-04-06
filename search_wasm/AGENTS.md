# Agent Coding Guidelines - search_wasm

Rust WASM library for high-performance in-memory search over arrays of JSON objects. Used by Salesforce LWC components.

---

## 1. Build / Test / Lint Commands

### Build WASM
```bash
# Build to pkg/ (ESM for bundlers)
wasm-pack build --target web --out-dir pkg

# Full release build
cargo build --release --target wasm32-unknown-unknown
```

### Bundle for LWC (Salesforce)
```bash
# Install rollup
npm install

# Bundle ESM to IIFE for loadScript
node node_modules/rollup/dist/bin/rollup pkg/search_wasm.js --format iife --name searchWasm --file rollup_lwc/search_wasm_iife.js
```

### Rust Development
```bash
# Format
cargo fmt

# Clippy lint
cargo clippy -- -D warnings

# Tests
cargo test

# Single test
cargo test <test_name>

# Check compilation
cargo check
```

---

## 2. Code Style Guidelines

### Formatting
- 4-space indent
- Run `cargo fmt` before commits
- 120 char line max

### Naming
- `snake_case` for functions, variables, modules
- `CamelCase` for types, structs, enums, traits
- `SCREAMING_SNAKE_CASE` for constants
- Prefixes: `is_`, `has_`, `get_` for predicates/accessors

### Imports (in order)
```rust
use std::...;                          // std library
use wasm_bindgen::prelude::*;          // wasm-bindgen
use serde::{Serialize, Deserialize};  // external crates
use crate::module::function;           // local modules
```

### Error Handling
- **Internal**: `Result<T, String>` for pure Rust functions
- **WASM export**: `Result<T, JsValue>` with `map_err(|e| JsValue::from_str(&e))`
- Never panic in WASM boundary functions

### WASM Bindgen Patterns
```rust
#[wasm_bindgen]
pub fn my_function(handle: u32, query: String) -> Result<String, JsValue> {
    // Always return Result for functions that can fail
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        let engine = engines.get_mut(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        // ... logic
        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}
```

### Thread-Local Storage
```rust
thread_local! {
    static ENGINES: RefCell<HashMap<u32, Engine>> = RefCell::new(HashMap::new());
    static NEXT_ENGINE_ID: RefCell<u32> = RefCell::new(1);
}
```

### Performance Guidelines
- Avoid allocations in hot paths
- Use `&[T]` and `&str` slices instead of owned types where possible
- Pre-allocate vectors with `Vec::with_capacity()`
- Use `ColumnarView` for columnar scans on large datasets

---

## 3. Architecture

### Engine Lifecycle
1. `init_engine()` / `init_engine_with_options()` → returns `handle: u32`
2. `execute_query(handle, query)` → JSON string results
3. `drop_engine(handle)` → frees WASM linear memory

### Query Language
- Predicates: `field = value`, `field != value`, `field > N`
- Logical: `AND`, `OR`, `NOT`
- Special: `LIKE`, `FUZZY`, `REGEX`, `CONTAINS`, `STARTS`, `ENDS`
- Clauses: `ORDER BY`, `LIMIT N`, `OFFSET N`

### Memory Model
- All data stored in WASM linear memory
- Engine handle is an index into `ENGINES` HashMap
- JSON serialization happens at boundaries only

---

## 4. Adding New WASM Functions

### Step 1: Add to lib.rs
```rust
#[wasm_bindgen]
pub fn new_function(handle: u32, param: String) -> Result<String, JsValue> {
    // Implementation
}
```

### Step 2: Rebuild
```bash
wasm-pack build --target web --out-dir pkg
```

### Step 3: Bundle
```bash
node node_modules/rollup/dist/bin/rollup pkg/search_wasm.js --format iife --name searchWasm --file rollup_lwc/search_wasm_iife.js
```

### Step 4: Deploy
Copy IIFE and WASM binary to Salesforce static resource.

---

## 5. Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test execute_query_paged

# With output
cargo test -- --nocapture
```

---

## 6. Common Patterns

### Zero-Copy Data Loading (new)
```rust
// Accept raw bytes instead of JSON string
#[wasm_bindgen]
pub fn init_engine_from_bytes(bytes: Vec<u8>, options_json: String) -> Result<u32, JsValue> {
    let json_str = std::str::from_utf8(&bytes)
        .map_err(|e| JsValue::from_str(&format!("Invalid UTF-8: {}", e)))?;
    // ... parse and init
}
```

### Paginated Results
```rust
#[derive(serde::Serialize)]
pub struct PagedResult {
    pub total_matches: usize,
    pub rows: Vec<Value>,
}
```

---

## 7. Git Commit Style

```
feat: add init_engine_from_bytes for zero-copy loading
fix: execute_query_paged returns accurate total_matches
docs: update DEVELOPER_GUIDE with new API
```

Never modify existing exported function signatures — add new functions instead for backward compatibility.
