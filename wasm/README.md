# Compile rumoca to web assembly

## Directory Layout

proj root (rumoca)

- wasm
  - pkg (WASM ouput directory, inculding wasm, js and d.ts files)
  - src (WASM library Api which refers to functions found in rumoca project)
  - target (binary files built by rust)

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

## Packaging

To build the WASM package and create an npm tarball in one step, run:

```sh
cd wasm && wasm-pack build . --release --target bundler &&  npm pack pkg
```

You can then manually upload this `.tgz` file to your private server (e.g. via `scp` or `rsync`).

### Note

In order to use this rumoca-wasm with the "vite" bundler, as of 2025/12, you will have to use
the vite-plugin-wasm module!

## Debug

We can debug rumoca on the browser by building it the following way:

```sh
wasm-pack build . --dev --target web
```
