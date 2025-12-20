# Compile rumoca to web assembly

Rumoca can be configured to compile to a WASM target
which enables it to be used in the browser without any other dependencies.

## Building

### npm based workflow

This workflow builds the rumoca wasm target and
pack it into an npm package which can be used
together with bundlers and is published to npm.

```sh
# building a package
cd wasm && npm run build

# publish package to npm
cd wasm && npm run publish
```

### rust based workflow for pure WASM

```sh
# build the wasm
# For web (native ESM in browsers):
    wasm-pack build . --release --target web --no-default-features --features wasm
# For bundlers:
    wasm-pack build . --release --target bundler --no-default-features --features wasm
# For Node.js:
    wasm-pack build . --release --target nodejs --no-default-features --features wasm
```

### Notes:

- Support for browsers with vite seems to work best with the "web" target.
- In order to use this rumoca-wasm with the "vite" bundler, as of 2025/12, you will have to use
  the [vite-plugin-wasm](https://www.npmjs.com/package/vite-plugin-wasm) module!

## Debug

We can debug rumoca on the browser by building it the following way :

```sh
wasm-pack build . --dev --target web --no-default-features --features wasm
```
