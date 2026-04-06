# Quick Start — Rust WASM Array Search Engine

This is the fastest way to get the search engine running in JS/React/LWC.

## 1. Build WASM

From `search_wasm`:

```powershell
wasm-pack build --target web
```

Output folder: `search_wasm/pkg`

## 2. Vanilla JS (fast test)

```js
import init, { execute_query_json } from "./pkg/search_wasm.js";

await init();

const data = [
  { name: "NetCore Router", salary: 2500000, tags: ["router", "fiber"] },
  { name: "Support Desk", salary: 450000, tags: ["support"] }
];

const query = 'salary > 1000000 OR tags IN ("fiber")';
const out = execute_query_json(JSON.stringify(data), query);
const results = JSON.parse(out);

console.log(results);
```

## 3. React (Vite)

1. Copy `search_wasm/pkg` into your React app.
2. Use the handle API for performance:

```jsx
import { useEffect, useRef, useState } from "react";
import init, { init_engine, execute_query, drop_engine } from "../pkg/search_wasm.js";

export default function App() {
  const [ready, setReady] = useState(false);
  const [results, setResults] = useState([]);
  const handleRef = useRef(null);

  useEffect(() => {
    (async () => {
      await init();
      const data = [/* your JSON objects */];
      handleRef.current = init_engine(JSON.stringify(data));
      setReady(true);
    })();
    return () => {
      if (handleRef.current !== null) {
        drop_engine(handleRef.current);
      }
    };
  }, []);

  const runQuery = () => {
    const query = 'country IN ("USA","India") ORDER BY price DESC LIMIT 10';
    const out = execute_query(handleRef.current, query);
    setResults(JSON.parse(out));
  };

  return (
    <div>
      <button disabled={!ready} onClick={runQuery}>Run Search</button>
      <pre>{JSON.stringify(results, null, 2)}</pre>
    </div>
  );
}
```

## 4. Salesforce LWC (minimal)

1. `wasm-pack build --target web`
2. Zip contents of `pkg` and upload as Static Resource (e.g. `search_wasm_pkg`)

```js
import { LightningElement } from "lwc";
import WASM_PKG from "@salesforce/resourceUrl/search_wasm_pkg";

export default class SearchDemo extends LightningElement {
  wasmReady = false;
  executeQuery;
  handle;

  async connectedCallback() {
    const wasmUrl = `${WASM_PKG}/search_wasm_bg.wasm`;
    const jsUrl = `${WASM_PKG}/search_wasm.js`;

    const module = await import(jsUrl);
    await module.default(wasmUrl);

    this.executeQuery = module.execute_query;
    this.handle = module.init_engine(JSON.stringify([/* your data */]));
    this.wasmReady = true;
  }

  runSearch() {
    const out = this.executeQuery(this.handle, 'tags IN ("fiber")');
    console.log(JSON.parse(out));
  }
}
```

## 5. Example Queries

```
salary BETWEEN 50000 AND 100000
name NOT LIKE "%router%"
name REGEX "^Net.*"
country IN ("USA","India") ORDER BY price DESC, salary ASC NULLS LAST
SELECT name, price, SCORE WHERE tags IN ("fiber")
```

---

For full docs see `DEVELOPER_GUIDE.md`.

