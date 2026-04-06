import { LightningElement } from "lwc";
import { loadScript } from "lightning/platformResourceLoader";
import WASM_PKG from "@salesforce/resourceUrl/search_wasm_pkg";
import GZIP_WASM_PKG from "@salesforce/resourceUrl/gzip_wasm_pkg";
import DEMO_DATA_PKG from "@salesforce/resourceUrl/demo_data_pkg";

const ENGINE_OPTIONS = JSON.stringify({
  indexes: ["country", "category", "tags", "skills", "active", "meta.region", "meta.owner", "name", "notes"],
  query_cache_cap: 200,
  columnar: true,
  columnar_fields: ["country", "category", "name", "notes", "price", "salary"]
});

const DEFAULT_PAGE_SIZE = 10;

const COLUMNS = [
  { label: "Name", fieldName: "name" },
  { label: "Country", fieldName: "country" },
  { label: "Category", fieldName: "category" },
  { label: "Price", fieldName: "price", type: "currency" },
  { label: "Salary", fieldName: "salary", type: "number" },
  { label: "Active", fieldName: "active", type: "boolean" }
];

const DEMO_QUERY = 'country = "India" OR category = "software"';

export default class WasmAdvanceDemo extends LightningElement {
  // State
  loading = true;
  wasmLoading = true;
  loadingMessage = "Initializing WASM...";
  loadingTime = "";
  error = "";
  validationError = "";
  showSummaryStats = false;

  wasm = null;
  gzipWasm = null;
  handle = null;

  pagedRows = [];
  totalMatches = 0;
  rowCount = 0;
  datasetSizeMb = "—";
  queryTimeMs = 0;

  page = 1;
  pageSize = DEFAULT_PAGE_SIZE;

  query = "";
  defaultQuery = DEMO_QUERY;

  cacheStats = null;
  lastCacheHits = 0;
  showCacheHit = false;
  _cacheHitTimer = null;

  uploadFileName = "";
  summaryRows = [];
  summaryColumns = [];
  summaryLogged = false;

  // Aggregate quick stats
  countryCount = "—";
  avgPrice = "—";

  columns = COLUMNS;
  pageSizeOptions = [
    { label: "10", value: "10" },
    { label: "25", value: "25" },
    { label: "50", value: "50" }
  ];

  // Computed
  get engineReady() {
    return this.handle !== null && !this.loading;
  }

  get searchDisabled() {
    return this.loading || this.handle === null;
  }

  get totalPages() {
    return Math.max(1, Math.ceil(this.totalMatches / this.pageSize));
  }

  get isFirst() { return this.page <= 1; }
  get isLast() { return this.page >= this.totalPages; }
  get visibleCount() { return this.pagedRows.length; }
  get totalMatchesFormatted() { return this.totalMatches.toLocaleString(); }
  get rowCountFormatted() { return this.rowCount.toLocaleString(); }

  // Lifecycle
  async connectedCallback() {
    this.wasmLoading = true;
    this.loadingMessage = "Loading WASM modules...";
    try {
      await this._loadWasmModules();
      this.wasmLoading = false;
      await this._loadDemoData();
    } catch (err) {
      this.error = err?.message || String(err);
      this.wasmLoading = false;
    } finally {
      this.loading = false;
    }
  }

  disconnectedCallback() {
    this._dropEngine();
    clearTimeout(this._cacheHitTimer);
  }

  // WASM Bootstrap
  async _loadWasmModules() {
    const wasmJsUrl = `${WASM_PKG}/search_wasm_iife.js`;
    const wasmBinUrl = `${WASM_PKG}/search_wasm_bg.wasm`;
    const gzipJsUrl = `${GZIP_WASM_PKG}/gzip_wasm_iife.js`;
    const gzipBinUrl = `${GZIP_WASM_PKG}/gzip_wasm_bg.wasm`;

    await Promise.all([loadScript(this, wasmJsUrl), loadScript(this, gzipJsUrl)]);

    if (!globalThis.searchWasm || !globalThis.gzipWasm) {
      throw new Error("WASM loaders not available");
    }

    const [wasmBytes, gzipBytes] = await Promise.all([
      fetch(wasmBinUrl).then(r => { if (!r.ok) throw new Error("fetch wasm failed"); return r.arrayBuffer(); }),
      fetch(gzipBinUrl).then(r => { if (!r.ok) throw new Error("fetch gzip failed"); return r.arrayBuffer(); })
    ]);

    await Promise.all([globalThis.searchWasm.default(wasmBytes), globalThis.gzipWasm.default(gzipBytes)]);
    this.wasm = globalThis.searchWasm;
    this.gzipWasm = globalThis.gzipWasm;
  }

  // Demo Data
  async _loadDemoData() {
    this.loadingMessage = "Loading demo dataset...";
    const dataGzUrl = `${DEMO_DATA_PKG}/data.json.gz`;
    try {
      const res = await fetch(dataGzUrl, { cache: "no-store" });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const gzBytes = new Uint8Array(await res.arrayBuffer());
      await this._initEngineFromGzBytes(gzBytes, "demo_data.json.gz");
      this.query = "";
      this._runPagedSearch(); // Auto-load data on init
    } catch (err) {
      console.warn("Demo data not available:", err?.message);
      this.loadingMessage = "";
    }
  }

  // Engine Init - with elapsed time display
  async _initEngineFromGzBytes(gzBytes, fileName) {
    const startTime = Date.now();
    let timeInterval;
    
    // Start timer for elapsed time display
    timeInterval = setInterval(() => {
      const elapsed = Math.round((Date.now() - startTime) / 1000);
      this.loadingTime = elapsed + "s";
    }, 1000);
    
    try {
      // Decompress
      this.loadingMessage = `Decompressing ${fileName}...`;
      await new Promise(resolve => setTimeout(resolve, 50));
      
      const jsonBytes = this.gzipWasm.gzip_decompress_bytes(gzBytes);
      const decompressedSize = jsonBytes.length;
      const sizeMb = (decompressedSize / 1024 / 1024).toFixed(1);
      console.log("Decompressed:", decompressedSize, "bytes (", sizeMb, "MB)");
      
      this._dropEngine();
      this.loadingMessage = `Loading ${sizeMb}MB into WASM engine...`;
      
      // Yield to UI before heavy operation
      await new Promise(resolve => setTimeout(resolve, 50));
      
      console.log("Starting init_engine_from_bytes...");
      this.handle = this.wasm.init_engine_from_bytes(jsonBytes, ENGINE_OPTIONS);
      console.log("Engine init complete");
      
      if (!this.handle || this.handle === 0) {
        throw new Error("Engine init failed");
      }
      
      // Get stats
      this._refreshDataSize();
      this._refreshCacheStats();
      
      this.loadingMessage = "Complete!";
      this.loadingTime = "";
      
      setTimeout(() => {
        this.loading = false;
        this.loadingMessage = "";
      }, 500);
      
    } catch (err) {
      console.error("Engine init error:", err);
      this.error = "Failed to load data: " + (err?.message || err);
      this.loadingMessage = "";
      this.loadingTime = "";
    } finally {
      clearInterval(timeInterval);
    }
  }

  _dropEngine() {
    if (this.handle !== null && this.wasm && typeof this.wasm.drop_engine === "function") {
      this.wasm.drop_engine(this.handle);
      this.handle = null;
    }
  }

  // Search
  _runPagedSearch() {
    if (!this.wasm || this.handle === null) return;
    this.error = "";
    const baseQuery = this.query.trim();
    const offset = (this.page - 1) * this.pageSize;
    const pagedQuery = baseQuery ? `${baseQuery} LIMIT ${this.pageSize} OFFSET ${offset}` : `LIMIT ${this.pageSize} OFFSET ${offset}`;
    const queryStart = performance.now();
    try {
      const raw = this.wasm.execute_query_paged(this.handle, pagedQuery);
      const result = JSON.parse(raw);
      this.queryTimeMs = Math.round(performance.now() - queryStart);
      this.pagedRows = Array.isArray(result.rows) ? result.rows : [];
      this.totalMatches = result.total_matches ?? this.pagedRows.length;
      this._refreshCacheStats();
    } catch (err) {
      this.error = err?.message || String(err);
    }
  }

  _validateQuery(query) {
    try {
      if (!this.wasm || typeof this.wasm.validate_query !== "function") return { ok: true };
      return this.wasm.validate_query(query);
    } catch (err) {
      return { ok: false, error: { message: err?.message || String(err), pos: 0 } };
    }
  }

  // Metrics
  _refreshDataSize() {
    if (!this.wasm || this.handle === null) return;
    try {
      const raw = this.wasm.get_engine_data_size(this.handle);
      const info = JSON.parse(raw);
      this.rowCount = info.row_count ?? 0;
      this.datasetSizeMb = info.approx_bytes ? (info.approx_bytes / 1_048_576).toFixed(1) : "—";
    } catch (err) { console.warn("get_engine_data_size failed", err); }
  }

  _refreshCacheStats() {
    if (!this.wasm || this.handle === null) return;
    try {
      const stats = this.wasm.engine_cache_stats(this.handle);
      this.cacheStats = stats;
      if (typeof stats?.hits === "number" && stats.hits > this.lastCacheHits) {
        this.lastCacheHits = stats.hits;
        this.showCacheHit = true;
        clearTimeout(this._cacheHitTimer);
        this._cacheHitTimer = setTimeout(() => { this.showCacheHit = false; }, 1500);
      }
    } catch (err) { console.warn("engine_cache_stats failed", err); }
  }

  // Event Handlers
  handleQueryChange(event) { this.query = event.target.value; }
  handleKeyUp(event) { if (event.key === "Enter") { this.page = 1; this._runPagedSearch(); } }

  runSearch() {
    this.page = 1;
    this._runPagedSearch();
  }

  runDemoQuery() {
    this.query = this.defaultQuery;
    this.page = 1;
    this._runPagedSearch();
  }

  clearSearch() { this.query = ""; this.page = 1; this.totalMatches = this.rowCount; this._runPagedSearch(); this.showSummaryStats = false; }

  prevPage() { if (this.page > 1) { this.page -= 1; this._runPagedSearch(); } }
  nextPage() { if (this.page < this.totalPages) { this.page += 1; this._runPagedSearch(); } }
  goToFirst() { this.page = 1; this._runPagedSearch(); }
  goToLast() { this.page = this.totalPages; this._runPagedSearch(); }
  handlePageSizeChange(event) { this.pageSize = parseInt(event.detail.value, 10); this.page = 1; this._runPagedSearch(); }

  closeSummary() { this.showSummaryStats = false; }

  // Aggregations
  runSummary(spec) {
    this.error = "";
    if (!this.wasm || this.handle === null) { this.error = "WASM not ready"; return; }
    try {
      const filter = this.query.trim();
      if (filter) {
        const v = this._validateQuery(filter);
        if (!v.ok) { this.error = v.error?.message || "Invalid filter"; return; }
        spec = { ...spec, filter };
      }
      let rows = [];
      if (typeof this.wasm.aggregate_handle_json === "function") {
        const out = this.wasm.aggregate_handle_json(this.handle, JSON.stringify(spec));
        rows = JSON.parse(out);
      } else if (typeof this.wasm.aggregate_handle === "function") {
        const out = this.wasm.aggregate_handle(this.handle, JSON.stringify(spec));
        if (Array.isArray(out)) rows = out;
        else if (typeof out === "string") rows = JSON.parse(out);
        else if (out?.rows) rows = out.rows;
        else rows = JSON.parse(JSON.stringify(out));
      }
      if (!this.summaryLogged) { this.summaryLogged = true; console.log("Summary:", rows); }
      this.summaryRows = rows;
      if (rows.length > 0) {
        this.summaryColumns = Object.keys(rows[0]).map(k => ({ label: k, fieldName: k }));
      }
      this.showSummaryStats = true;
    } catch (err) { this.error = err?.message || String(err); }
  }

  runSummaryCountByCountry() { this.runSummary({ group_by: ["country"], aggs: [{ op: "COUNT", field: "*", alias: "count" }] }); }
  runSummaryAvgPrice() { this.runSummary({ group_by: ["category"], aggs: [{ op: "AVG", field: "price", alias: "avg_price" }] }); }

  // File Upload
  async handleFileUpload(event) {
    const file = event.target.files[0];
    if (!file) return;
    this.loading = true;
    this.loadingMessage = `Reading ${file.name}...`;
    this.error = "";
    this.uploadFileName = file.name;
    this.showSummaryStats = false;
    try {
      const buffer = await file.arrayBuffer();
      const gzBytes = new Uint8Array(buffer);
      await this._initEngineFromGzBytes(gzBytes, file.name);
      this.query = ""; this.page = 1; this.totalMatches = this.rowCount;
      this._runPagedSearch();
    } catch (err) { this.error = `Upload failed: ${err?.message || String(err)}`; }
    finally { this.loading = false; }
  }
}