# Compile rumoca to web assembly

## Building

```sh
# install prerequisites
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# go into wasm sub directory
cd wasm

# build the wasm
# For web (native ESM in browsers):
    wasm-pack build . --release --target web
# For bundlers:
    wasm-pack build . --release --target bundler
# For Node.js:
    wasm-pack build . --release --target nodejs
```

### Note:

Support for browsers with vite seems to work best with the "web" target.

## Debug

We can debug rumoca on the browser by building it the following way:

```sh
wasm-pack build . --dev --target web
```
