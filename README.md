# hn-api-wasm

`hn-api-wasm` is a small Rust WebAssembly plugin that wraps public Hacker News API endpoints in AI-friendly tools. It is built with `extism-pdk` and `wasm-forge-pdk`, and returns compact YAML so tool consumers get readable output without extra response shaping.

## What It Does

The plugin exposes a focused set of Hacker News read-only tools:

| Tool | Purpose | Input |
| --- | --- | --- |
| `get_max_item` | Return the current highest item ID | none |
| `get_top_stories` | Return hydrated top stories | `limit?` |
| `get_new_stories` | Return hydrated newest stories | `limit?` |
| `get_best_stories` | Return hydrated best stories | `limit?` |
| `get_ask_stories` | Return hydrated Ask HN stories | `limit?` |
| `get_show_stories` | Return hydrated Show HN stories | `limit?` |
| `get_job_stories` | Return hydrated job stories | `limit?` |
| `get_item` | Return one Hacker News item | `id` |
| `get_user` | Return one Hacker News user | `id`, `submitted_limit?` |
| `get_updates` | Return recently changed item and profile IDs | none |

## Output Notes

- Story list tools default to `10` items and clamp `limit` to `1..=25`.
- `get_user` defaults `submitted_limit` to `20` and clamps it to `1..=50`.
- Item child lists are summarized with counts plus a short preview.
- Empty or missing fields are omitted from YAML where possible.
- Network access is limited to `https://hacker-news.firebaseio.com/v0`.

## Build And Test

Prerequisites:

- Rust toolchain with the `wasm32-unknown-unknown` target
- Access to the `basilisklabs` cargo registry configured in [.cargo/config.toml](/home/carl/Documents/projects/hn-api-wasm/.cargo/config.toml)

Common commands:

```bash
rustup target add wasm32-unknown-unknown
cargo test
cargo build --release --target wasm32-unknown-unknown
```

The compiled plugin is expected at:

```text
target/wasm32-unknown-unknown/release/hn_api_wasm.wasm
```

## Packaging Files

- [src/lib.rs](/home/carl/Documents/projects/hn-api-wasm/src/lib.rs): plugin implementation and tool entrypoints
- [build.rs](/home/carl/Documents/projects/hn-api-wasm/build.rs): generates `forge.yaml` during build
- [forge.yaml](/home/carl/Documents/projects/hn-api-wasm/forge.yaml): tool metadata and JSON schemas for Forge consumers
- [extism-manifest.json](/home/carl/Documents/projects/hn-api-wasm/extism-manifest.json): local Extism manifest pointing at the built `.wasm`
- [forge-index.json](/home/carl/Documents/projects/hn-api-wasm/forge-index.json): published Forge index entry referencing the container image

## Implementation Summary

The crate is a `cdylib` compiled to WebAssembly. Each tool performs a GET request against the official Hacker News Firebase API, deserializes the response into Rust structs, reshapes it into a smaller output model, and serializes that result to YAML.

The codebase is intentionally small:

- shared helpers build URLs, clamp limits, and standardize HTTP error handling
- story list tools hydrate item IDs into compact summaries
- item and user tools expose the most useful fields while trimming empty data
- tests cover URL construction, limit clamping, YAML omission behavior, and output formatting
