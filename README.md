# Rust WASM Search Engine for Salesforce LWC

Copyright (c) 2024 Kuldeep Singh - Licensed under the MIT License

A production-ready, zero-copy WebAssembly search engine designed for Salesforce Lightning Web Components (LWC). Enables high-performance in-memory search over JSON arrays directly in the browser.

## Features

- **In-Memory Search** - Direct search over JSON array with zero-copy during query execution
- **Gzip Decompression** - Companion gzip_wasm module for decompressing .gz files in browser
- **Full-Text Search** - Boolean operators (AND, OR, NOT), LIKE, FUZZY, REGEX
- **Comparison Operators** - =, !=, >, >=, <, <=, BETWEEN, IN, NOT IN
- **Text Matching** - CONTAINS, STARTS WITH, ENDS WITH
- **Pagination** - Built-in pagination with accurate total match count
- **Adaptive Caching** - Result caching for fast repeat queries
- **Salesforce LWC Ready** - IIFE bundle works in Lightning Web Components

## Project Structure

```
salesforce-rust/
├── search_wasm/          # Main search engine (Rust + WASM)
│   ├── src/lib.rs        # Core search engine source
│   ├── Cargo.toml        # Rust dependencies
│   ├── README.md         # Search engine documentation
│   ├── DEVELOPER_GUIDE.md
│   └── ARCHITECTURE.md
│
├── gzip_wasm/            # Gzip decompression WASM
│   ├── src/lib.rs        # Gzip decompressor
│   └── Cargo.toml
│
├── lwc/                  # Salesforce LWC components
│   └── wasmAdvanceDemo/ # Demo component with search UI
│
└── staticresources/     # Static resources for Salesforce
    ├── search_wasm_pkg/ # Bundled WASM + IIFE
    ├── gzip_wasm_pkg/   # Gzip WASM bundle
    └── demo_data_pkg/   # Sample data (.json.gz)
```

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) with `wasm32-unknown-unknown` target
- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer.html)

### Build

```bash
# Build search engine
cd search_wasm
wasm-pack build --target web --out-dir pkg

# Build gzip decompressor
cd ../gzip_wasm
wasm-pack build --target web --out-dir pkg

# Bundle for LWC (IIFE)
cd ../search_wasm
npm install
npm run build:iife
```

### Usage in JavaScript

```javascript
// Initialize
import init, { init_engine, search } from './search_wasm.js';
await init();

// Load data
const engineId = init_engine(jsonDataString);

// Search
const result = search(engineId, 'country = "India" AND category = "software"');
console.log(result.items);
console.log(result.total);
```

### Usage in Salesforce LWC

```javascript
// In your LWC component
import { LightningElement } from 'lwc';
import SEARCH_WASM from '@salesforce/resourceUrl/search_wasm_pkg';

export default class MySearch extends LightningElement {
    async connectedCallback() {
        const wasm = await SEARCH_WASM();
        // Initialize and use
    }
}
```

## Example Queries

```sql
-- Basic equality
name = "Kuldeep"

-- Multiple conditions
country = "India" AND category = "software"

-- IN clause
category IN ("finance", "software", "network")

-- LIKE with wildcards
name LIKE "%desk%"

-- BETWEEN
price BETWEEN (100, 1000)

-- Complex boolean
(name = "John" OR name = "Jane") AND country = "USA" AND status = "active"
```

## Performance

- Handles 500K+ records in browser memory
- Sub-millisecond query execution for cached queries
- Zero-copy gzip loading avoids JS decompression overhead

## License

MIT License - see LICENSE file