# Compile rumoca to web assembly

## Building

```sh
# install prerequisites
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# go into wasm sub directory
cd wasm

# build the wasm
# For bundlers:
    wasm-pack build . --release --target bundler
# For web (native ESM in browsers):
    wasm-pack build . --release --target web
# For Node.js:
    wasm-pack build . --release --target nodejs
```
