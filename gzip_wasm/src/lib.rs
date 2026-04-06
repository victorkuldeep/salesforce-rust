/**
 * Copyright (c) 2024 Kuldeep Singh
 * Licensed under the MIT License
 *
 * Gzip compression/decompression WASM module for Salesforce LWC
 *
 * Features:
 * - Decompress .gz files in browser without JS overhead
 * - Base64 encoded input/output support
 * - Designed for Salesforce LWC deployment
 *
 * Usage:
 *   const decompressed = decompress_gzip(base64CompressedData);
 *   const compressed = compress_gzip(jsonString);
 */
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main_js() -> Result<(), JsValue> {
    // This provides better error messages in debug mode.
    // It's optional but very useful.
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    Ok(())
}

/// Compresses a string into a GZIP base64 string
#[wasm_bindgen]
pub fn gzip_compress(input: &str) -> String {
    let bytes = compress_bytes(input.as_bytes());
    STANDARD.encode(bytes)
}

/// Decompresses a GZIP base64 string back into a string
#[wasm_bindgen]
pub fn gzip_decompress(base64_gz: &str) -> Result<String, JsValue> {
    let compressed_bytes = STANDARD
        .decode(base64_gz)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let decompressed_bytes =
        decompress_bytes(&compressed_bytes).map_err(|e| JsValue::from_str(&e))?;
    String::from_utf8(decompressed_bytes).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Compresses a raw Uint8Array into a raw Uint8Array (faster for large datasets)
#[wasm_bindgen]
pub fn gzip_compress_bytes(input: &[u8]) -> Vec<u8> {
    compress_bytes(input)
}

/// Decompresses a raw Uint8Array into a raw Uint8Array (faster for large datasets)
#[wasm_bindgen]
pub fn gzip_decompress_bytes(input: &[u8]) -> Result<Vec<u8>, JsValue> {
    decompress_bytes(input).map_err(|e| JsValue::from_str(&e))
}

// --- Internal helpers --- //

fn compress_bytes(input: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input).expect("Compression failed");
    encoder.finish().expect("Failed to finish compression")
}

fn decompress_bytes(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = GzDecoder::new(input);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| e.to_string())?;
    Ok(decompressed)
}
