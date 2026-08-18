# Rust source plugin example

Install Rust's `wasm32-wasip2` target, then run:

```sh
cargo build --release --target wasm32-wasip2
```

The component is written to `target/wasm32-wasip2/release/rill_example_source_plugin.wasm`. Install that file through Rill's plugin administration API.

