# Rust WASM Search Engine (Array of Objects)

This library parses boolean search queries and filters an array of JSON objects.

Supported operators:
- `AND`, `OR`, `NOT`
- `IN`, `NOT IN`
- `=`, `!=`, `>`, `>=`, `<`, `<=`
- `LIKE` with `%` and `_`
- `BETWEEN (a, b)`
- `CONTAINS`
- `STARTS WITH`, `ENDS WITH`
- `EXISTS`, `IS NULL`, `IS NOT NULL`
- Parentheses with deep nesting

Field predicates use dotted paths (e.g. `meta.region`, `tags`, `tags.0`).

## Example Queries

```
hello
name = "Kuldeep Singh" AND country LIKE "%India%"
salary > 1000000 OR skills IN ("SOM", "REACT")
price BETWEEN (200, 900) AND category = "network"
name CONTAINS "desk" OR notes STARTS WITH "crm"
meta.region EXISTS AND meta.owner IS NOT NULL
network AND (tags IN ("fiber", "router"))
category IN ("software", "finance") AND NOT active IN (false)
name LIKE "%desk%" OR notes LIKE "%workflow%"
meta.region IN ("APAC") AND (tags IN ("billing") OR tags IN ("payments"))
```

## Build WASM

```
wasm-pack build --target web
```

## JS Usage (Vite/Next)

```
import init, { sample_data, search_json, search_sample } from "./pkg/search_wasm.js";

await init();

const data = sample_data();
const results1 = search_json(data, "network AND tags IN (\"fiber\")");
const results2 = search_sample("name LIKE \"%desk%\" OR notes LIKE \"%workflow%\"");
```
