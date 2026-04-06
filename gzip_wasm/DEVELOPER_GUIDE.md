# Rust GZIP WASM Utility — Developer Guide

This guide explains how to use the `gzip_wasm` utility in JavaScript, React, and Salesforce LWC. It allows you to efficiently compress and decompress data, significantly reducing payload sizes when passing data to browser-based search engines or other tools.

## 1. What This Library Does

The utility provides access to:
- String to base-64 GZIP compression/decompression
- Raw `Uint8Array` to `Uint8Array` compression/decompression

This is particularly built to power scenarios like:
1. Fetching a large `.json.gz` file via HTTP.
2. Decompressing it quickly using `gzip_wasm`.
3. Loading the resulting JSON string into `search_wasm`.
4. Doing filtering and re-compressing the filtered data to `.gz` for download.

## 2. Build WASM

From `gzip_wasm` root:

```powershell
wasm-pack build --target web
```

Output is generated in `gzip_wasm/pkg`.

### 2.1 Build Targets (React vs LWC)

- **React/Vite (ES Modules)**
  ```powershell
  wasm-pack build --target web --out-dir pkg
  ```
  Use `import init from "./pkg/gzip_wasm.js"` in React.

- **Salesforce LWC (IIFE via Rollup)**
  LWC doesn’t allow dynamic imports, so we ship an IIFE bundle exactly like `search_wasm`:
  ```powershell
  wasm-pack build --target web --out-dir pkg
  npx rollup "./pkg/gzip_wasm.js" --format iife --name gzipWasm --file "./rollup_lwc/gzip_wasm_iife.js"
  ```
  Copy `gzip_wasm_iife.js` and `gzip_wasm_bg.wasm` to your Salesforce **Static Resource**.

## 3. Usage in JavaScript (Vanilla / Module)

### 3.1 Base64 String Usage

This is the easiest approach if your data transport explicitly gives you base64:

```js
import init, { gzip_compress, gzip_decompress } from "./pkg/gzip_wasm.js";

await init();

// Compress a string
const inputParams = JSON.stringify({ huge: "data", array: [1,2,3,4,5] });
const compressedBase64 = gzip_compress(inputParams);
console.log("Compressed:", compressedBase64);

// Decompress back to string
const decompressedStr = gzip_decompress(compressedBase64);
console.log("Decompressed:", decompressedStr);
```

### 3.2 Raw Uint8Array Usage (Highest Performance)

If you fetch `.gz` data from the server, you should handle it as raw bytes (ArrayBuffer -> Uint8Array) to avoid base64 encoding/decoding overheads.

```js
import init, { gzip_compress_bytes, gzip_decompress_bytes } from "./pkg/gzip_wasm.js";

await init();

// Fetch a raw gzip compressed file
const response = await fetch('/data/huge_payload.json.gz');
const arrayBuffer = await response.arrayBuffer();
const gzippedData = new Uint8Array(arrayBuffer);

// Decompress bytes
const jsonBytes = gzip_decompress_bytes(gzippedData);

// Decode to string for `search_wasm` or JS parsing
const decoder = new TextDecoder();
const jsonStr = decoder.decode(jsonBytes);
const dataArray = JSON.parse(jsonStr);
```

## 4. Usage in Salesforce LWC (IIFE Bundle)

When working inside LWC, load the IIFE bundle you created:

```js
import { LightningElement } from "lwc";
import { loadScript } from "lightning/platformResourceLoader";
import GZIP_WASM_PKG from "@salesforce/resourceUrl/gzip_wasm_pkg";

export default class DataPipelineDemo extends LightningElement {
  wasmReady = false;
  wasm; // Will hold the gzip module

  async connectedCallback() {
    const wasmUrl = `${GZIP_WASM_PKG}/gzip_wasm_bg.wasm`;
    const jsUrl = `${GZIP_WASM_PKG}/gzip_wasm_iife.js`;

    await loadScript(this, jsUrl);
    
    // Check if the global object exists
    if (!globalThis.gzipWasm || !globalThis.gzipWasm.default) {
      throw new Error("WASM loader not available (gzipWasm missing)");
    }

    // Initialize the module
    await globalThis.gzipWasm.default(wasmUrl);
    this.wasm = globalThis.gzipWasm;
    this.wasmReady = true;
    
    // Now you can start fetching and decompressing!
  }
}
```

## 5. Integrating with `search_wasm`

If you are using this alongside `search_wasm`, the pipeline looks like this:

1. `gzip_wasm` decompresses `.json.gz` payload into raw bytes $\\rightarrow$ Decoded JSON String.
2. `search_wasm` via `init_engine(jsonString)` sets up the arrays.
3. User runs a query in your LWC UI. 
4. `search_wasm` computes filtered results.
5. If user wants to download these filtered results, loop back to `gzip_wasm_compress` or `gzip_wasm_compress_bytes` and prompt a download of the `filtered.json.gz` file.
