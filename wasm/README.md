# Compile rumoca to web assembly

Rumoca can be configured to compile to a WASM target
which enables it to be used in the browser without any other dependencies.

## Building

```sh
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

## Packaging

To build the WASM package and create an npm tarball in one step, run:

```sh
wasm-pack build . --release --target web --no-default-features --features wasm && npm pack pkg
```

You can then manually upload this `.tgz` file to your private server (e.g. via `scp` or `rsync`).

### Note

In order to use this rumoca-wasm with the "vite" bundler, as of 2025/12, you will have to use
the [vite-plugin-wasm](https://www.npmjs.com/package/vite-plugin-wasm) module!

## Debug

We can debug rumoca on the browser by building it the following way :

```sh
wasm-pack build . --dev --target web --no-default-features --features wasm
```
