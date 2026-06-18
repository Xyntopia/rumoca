import { inflateRawSync } from "node:zlib";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { ensureNodeSelfForWasmBindgenRayon } from "./node_rayon_shim.mjs";

const END_OF_CENTRAL_DIRECTORY_SIGNATURE = 0x06054b50;
const CENTRAL_DIRECTORY_FILE_HEADER_SIGNATURE = 0x02014b50;
const LOCAL_FILE_HEADER_SIGNATURE = 0x04034b50;

function argValue(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : fallback;
}

function argValues(name) {
  const values = [];
  for (let index = 0; index < process.argv.length; index++) {
    if (process.argv[index] === name && process.argv[index + 1]) {
      values.push(process.argv[index + 1]);
    }
  }
  return values;
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function collectMoFiles(root, prefix = "") {
  const entries = await readdir(root, { withFileTypes: true });
  entries.sort((lhs, rhs) => lhs.name.localeCompare(rhs.name));
  const files = [];
  for (const entry of entries) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (relative.includes("Test") || relative.includes("Obsolete")) {
      continue;
    }
    const absolute = path.join(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...await collectMoFiles(absolute, relative));
    } else if (entry.isFile() && entry.name.endsWith(".mo")) {
      files.push([absolute, relative]);
    }
  }
  return files;
}

async function readMslSources(mslArchiveRoot) {
  const roots = [
    ["Modelica", path.join(mslArchiveRoot, "Modelica 4.1.0")],
    ["ModelicaServices", path.join(mslArchiveRoot, "ModelicaServices 4.1.0")],
  ];
  const sources = {};
  for (const [logicalRoot, root] of roots) {
    const rootStat = await stat(root);
    assert(rootStat.isDirectory(), `missing MSL source-root directory: ${root}`);
    for (const [absolute, relative] of await collectMoFiles(root)) {
      sources[`${logicalRoot}/${relative}`] = await readFile(absolute, "utf8");
    }
  }

  const complexPath = path.join(mslArchiveRoot, "Complex.mo");
  sources["Complex.mo"] = await readFile(complexPath, "utf8");
  return sources;
}

function findEndOfCentralDirectory(zip) {
  const minOffset = Math.max(0, zip.length - 0xffff - 22);
  for (let offset = zip.length - 22; offset >= minOffset; offset--) {
    if (zip.readUInt32LE(offset) === END_OF_CENTRAL_DIRECTORY_SIGNATURE) {
      return offset;
    }
  }
  throw new Error("ZIP end-of-central-directory record not found");
}

function zipEntries(zip) {
  const eocdOffset = findEndOfCentralDirectory(zip);
  const entryCount = zip.readUInt16LE(eocdOffset + 10);
  let offset = zip.readUInt32LE(eocdOffset + 16);
  const entries = [];

  for (let index = 0; index < entryCount; index++) {
    assert(
      zip.readUInt32LE(offset) === CENTRAL_DIRECTORY_FILE_HEADER_SIGNATURE,
      `invalid ZIP central directory header at ${offset}`,
    );
    const compressionMethod = zip.readUInt16LE(offset + 10);
    const compressedSize = zip.readUInt32LE(offset + 20);
    const uncompressedSize = zip.readUInt32LE(offset + 24);
    const fileNameLength = zip.readUInt16LE(offset + 28);
    const extraLength = zip.readUInt16LE(offset + 30);
    const commentLength = zip.readUInt16LE(offset + 32);
    const localHeaderOffset = zip.readUInt32LE(offset + 42);
    const nameStart = offset + 46;
    const name = zip.toString("utf8", nameStart, nameStart + fileNameLength);
    entries.push({
      name,
      compressionMethod,
      compressedSize,
      uncompressedSize,
      localHeaderOffset,
    });
    offset = nameStart + fileNameLength + extraLength + commentLength;
  }

  return entries;
}

function normalizeArchivePath(relativePath) {
  const parts = relativePath.split("/");
  if (parts.length > 1 && /(?:Standard)?Library|^MSL/i.test(parts[0])) {
    return parts.slice(1).join("/");
  }
  if (parts.length > 0) {
    parts[0] = parts[0].replace(/[\s-][\d.]+$/, "");
    return parts.join("/");
  }
  return relativePath;
}

function inflateZipEntryToString(zip, entry) {
  assert(
    zip.readUInt32LE(entry.localHeaderOffset) === LOCAL_FILE_HEADER_SIGNATURE,
    `invalid ZIP local file header at ${entry.localHeaderOffset}`,
  );
  const fileNameLength = zip.readUInt16LE(entry.localHeaderOffset + 26);
  const extraLength = zip.readUInt16LE(entry.localHeaderOffset + 28);
  const dataStart = entry.localHeaderOffset + 30 + fileNameLength + extraLength;
  const compressed = zip.subarray(dataStart, dataStart + entry.compressedSize);
  if (entry.compressionMethod === 0) {
    return compressed.toString("utf8");
  }
  if (entry.compressionMethod === 8) {
    const inflated = inflateRawSync(compressed);
    assert(
      inflated.length === entry.uncompressedSize,
      `inflated size mismatch for ${entry.name}`,
    );
    return inflated.toString("utf8");
  }
  throw new Error(
    `unsupported ZIP compression method ${entry.compressionMethod} for ${entry.name}`,
  );
}

async function readMslZipSources(mslZipPath) {
  const readStarted = performance.now();
  const zip = await readFile(mslZipPath);
  const zipReadMs = ms(readStarted);

  const parseStarted = performance.now();
  const moEntries = zipEntries(zip).filter((entry) => {
    return (
      entry.name.endsWith(".mo") &&
      !entry.name.includes("Test") &&
      !entry.name.includes("Obsolete")
    );
  });
  const zipParseMs = ms(parseStarted);

  const extractStarted = performance.now();
  const sources = {};
  for (const entry of moEntries) {
    sources[normalizeArchivePath(entry.name)] = inflateZipEntryToString(zip, entry);
  }

  return {
    sources,
    zipReadMs,
    zipParseMs,
    zipExtractMs: ms(extractStarted),
  };
}

function ms(msStarted) {
  return Math.round((performance.now() - msStarted) * 10) / 10;
}

function sourceWithinPrefix(source) {
  const match = source.match(/^\s*within\s+([^;]*);/m);
  if (!match) {
    return [];
  }
  const raw = match[1].trim();
  return raw ? raw.split(".").filter(Boolean) : [];
}

function declarationName(line) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith("//")) {
    return null;
  }
  const match = trimmed.match(
    /^(?:(?:encapsulated|partial|final|redeclare)\s+)*(?:(?:operator)\s+)?(?:package|model|block|class|record|connector|function|type)\s+([A-Za-z_][A-Za-z0-9_]*)\b/,
  );
  return match?.[1] || null;
}

function endName(line) {
  const match = line.trim().match(/^end\s+([A-Za-z_][A-Za-z0-9_]*)\s*;/);
  return match?.[1] || null;
}

function buildLazyClassIndex(sources) {
  const classToUri = new Map();
  const uriToClasses = new Map();
  const uriToWithin = new Map();
  for (const [uri, source] of Object.entries(sources)) {
    const stack = sourceWithinPrefix(source);
    uriToWithin.set(uri, stack.join("."));
    const classes = [];
    for (const line of source.split(/\r?\n/)) {
      const declared = declarationName(line);
      if (declared) {
        const fullName = [...stack, declared].join(".");
        classes.push(fullName);
        classToUri.set(fullName, uri);
        stack.push(declared);
      }
      const ended = endName(line);
      if (ended && stack[stack.length - 1] === ended) {
        stack.pop();
      }
    }
    uriToClasses.set(uri, classes);
  }
  return { classToUri, uriToClasses, uriToWithin };
}

function addClassAndParents(index, selectedUris, className) {
  const parts = className.split(".");
  for (let end = 1; end <= parts.length; end++) {
    const candidate = parts.slice(0, end).join(".");
    const uri = index.classToUri.get(candidate);
    if (uri) {
      selectedUris.add(uri);
    }
  }
}

function addSuffixMatches(index, selectedUris, reference) {
  const suffix = `.${reference}`;
  let count = 0;
  for (const className of index.classToUri.keys()) {
    if (className === reference || className.endsWith(suffix)) {
      addClassAndParents(index, selectedUris, className);
      count++;
    }
  }
  return count;
}

function candidateReferences(source) {
  const references = new Set();
  const regex = /\b[A-Z][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*/g;
  for (const match of sourceWithoutCommentsStringsAndAnnotations(source).matchAll(regex)) {
    references.add(match[0]);
  }
  return references;
}

function sourceWithoutCommentsStringsAndAnnotations(source) {
  return source
    .split(/\r?\n/)
    .filter((line) => !line.trimStart().startsWith("annotation"))
    .map((line) => {
      let out = "";
      let inString = false;
      for (let index = 0; index < line.length; index++) {
        const ch = line[index];
        const next = line[index + 1];
        if (!inString && ch === "/" && next === "/") {
          break;
        }
        if (ch === '"' && line[index - 1] !== "\\") {
          inString = !inString;
          out += " ";
          continue;
        }
        out += inString ? " " : ch;
      }
      return out;
    })
    .join("\n");
}

function resolveReferenceCandidates(index, uri, reference) {
  if (reference.startsWith("Modelica.")) {
    return [reference];
  }
  const within = index.uriToWithin.get(uri) || "";
  const parts = within ? within.split(".") : [];
  const candidates = [reference];
  for (let end = parts.length; end >= 0; end--) {
    const prefix = parts.slice(0, end).join(".");
    candidates.push(prefix ? `${prefix}.${reference}` : reference);
  }
  return candidates;
}

function selectLazySourceSubset(sources, modelName) {
  const index = buildLazyClassIndex(sources);
  const selectedUris = new Set();
  addClassAndParents(index, selectedUris, modelName);
  expandLazySelectionFromSources(sources, index, selectedUris);

  const subset = {};
  for (const uri of Array.from(selectedUris).sort((lhs, rhs) => lhs.localeCompare(rhs))) {
    subset[uri] = sources[uri];
  }
  return {
    sources: subset,
    index,
    selectedUris,
    indexedClassCount: index.classToUri.size,
    selectedFileCount: selectedUris.size,
  };
}

function expandLazySelectionFromSources(sources, index, selectedUris) {
  for (let pass = 0; pass < 8; pass++) {
    const before = selectedUris.size;
    for (const uri of Array.from(selectedUris)) {
      const source = sources[uri];
      if (!source) {
        continue;
      }
      for (const reference of candidateReferences(source)) {
        for (const candidate of resolveReferenceCandidates(index, uri, reference)) {
          if (index.classToUri.has(candidate)) {
            addClassAndParents(index, selectedUris, candidate);
          }
        }
      }
    }
    if (selectedUris.size === before) {
      break;
    }
  }
}

function compileTiming(raw) {
  const parsed = JSON.parse(raw);
  return {
    jsonBytes: Buffer.byteLength(raw),
    balanced: parsed.balance?.is_balanced === true,
    equations: parsed.balance?.num_equations ?? null,
    unknowns: parsed.balance?.num_unknowns ?? null,
    phaseTiming: parsed.__compile_phase_timing ?? null,
  };
}

function compileModels(wasmModule, models) {
  const source = "model WasmBenchmarkInput\nend WasmBenchmarkInput;\n";
  return models.map((model) => {
    const started = performance.now();
    try {
      const raw = wasmModule.compile(source, model);
      return {
        model,
        ok: true,
        compileMs: ms(started),
        ...compileTiming(raw),
      };
    } catch (error) {
      return {
        model,
        ok: false,
        compileMs: ms(started),
        error: error?.message || String(error),
      };
    }
  });
}

function unresolvedReferences(errorText) {
  const refs = new Set();
  const regex = /unresolved (?:type )?reference: '([^']+)'/g;
  for (const match of String(errorText).matchAll(regex)) {
    refs.add(match[1]);
  }
  return Array.from(refs);
}

function lazyCompileModel(wasmModule, sources, lazySubset, model) {
  const attempts = [];
  for (let attempt = 0; attempt < 5; attempt++) {
    const selected = {};
    for (const uri of Array.from(lazySubset.selectedUris).sort((lhs, rhs) =>
      lhs.localeCompare(rhs),
    )) {
      selected[uri] = sources[uri];
    }
    wasmModule.clear_source_root_cache();
    const stringifyStarted = performance.now();
    const sourceRootsJson = JSON.stringify(selected);
    const stringifyMs = ms(stringifyStarted);
    const loadStarted = performance.now();
    const loadResult = JSON.parse(wasmModule.load_source_roots(sourceRootsJson));
    const loadMs = ms(loadStarted);
    const compile = compileModels(wasmModule, [model])[0];
    const unresolved = compile.ok ? [] : unresolvedReferences(compile.error);
    let addedFromUnresolved = 0;
    for (const reference of unresolved) {
      addedFromUnresolved += addSuffixMatches(
        lazySubset.index,
        lazySubset.selectedUris,
        reference,
      );
    }
    if (addedFromUnresolved > 0) {
      expandLazySelectionFromSources(sources, lazySubset.index, lazySubset.selectedUris);
    }
    attempts.push({
      attempt: attempt + 1,
      selectedFileCount: Object.keys(selected).length,
      sourceJsonBytes: Buffer.byteLength(sourceRootsJson),
      stringifyMs,
      wasmLoadSourceRootsMs: loadMs,
      parsedCount: loadResult.parsed_count,
      errorCount: loadResult.error_count,
      compile,
      unresolved,
      addedFromUnresolved,
    });
    if (compile.ok || addedFromUnresolved === 0) {
      break;
    }
  }
  return attempts;
}

async function main() {
  ensureNodeSelfForWasmBindgenRayon();
  const pkgSubdir = argValue("--pkg-subdir", "release-full-web");
  const mslRoot = argValue(
    "--msl-root",
    "target/msl/ModelicaStandardLibrary-4.1.0",
  );
  const mslZip = argValue("--msl-zip", "");
  const lazyModel = argValue("--lazy-model", "");
  const models = argValues("--compile-model");
  if (models.length === 0) {
    models.push(
      "Modelica.Blocks.Examples.BooleanNetwork1",
      "Modelica.Electrical.Analog.Examples.Resistor",
      "Modelica.Blocks.Examples.PID_Controller",
    );
  }
  const wasmModule = await import(`../../../pkg/${pkgSubdir}/rumoca_bind_wasm.js`);
  const wasmBytes = await readFile(
    new URL(`../../../pkg/${pkgSubdir}/rumoca_bind_wasm_bg.wasm`, import.meta.url),
  );
  await wasmModule.default({ module_or_path: wasmBytes });

  wasmModule.clear_source_root_cache();

  let started = performance.now();
  const zipTimings = mslZip
    ? await readMslZipSources(path.resolve(mslZip))
    : null;
  const sources = zipTimings
    ? zipTimings.sources
    : await readMslSources(path.resolve(mslRoot));
  const readMs = zipTimings ? undefined : ms(started);
  const uris = Object.keys(sources).sort((lhs, rhs) => lhs.localeCompare(rhs));

  let lazyReport = null;
  if (lazyModel) {
    wasmModule.clear_source_root_cache();
    const lazyStarted = performance.now();
    const lazySubset = selectLazySourceSubset(sources, lazyModel);
    const lazyIndexAndSelectMs = ms(lazyStarted);
    const attempts = lazyCompileModel(wasmModule, sources, lazySubset, lazyModel);
    const lastAttempt = attempts[attempts.length - 1];
    lazyReport = {
      model: lazyModel,
      indexedClassCount: lazySubset.indexedClassCount,
      indexAndSelectMs: lazyIndexAndSelectMs,
      selectedFileCount: lastAttempt?.selectedFileCount ?? lazySubset.selectedFileCount,
      sourceJsonBytes: lastAttempt?.sourceJsonBytes ?? 0,
      stringifyMs: lastAttempt?.stringifyMs ?? 0,
      wasmLoadSourceRootsMs: lastAttempt?.wasmLoadSourceRootsMs ?? 0,
      parsedCount: lastAttempt?.parsedCount ?? 0,
      errorCount: lastAttempt?.errorCount ?? 0,
      compile: lastAttempt?.compile ?? null,
      attempts,
    };
  }

  started = performance.now();
  const sourceRootsJson = JSON.stringify(sources);
  const stringifyMs = ms(started);

  started = performance.now();
  const loadResult = JSON.parse(wasmModule.load_source_roots(sourceRootsJson));
  const loadMs = ms(started);
  const documentCountAfterLoad = wasmModule.get_source_root_document_count();

  const compileAfterTextLoad = compileModels(wasmModule, models);

  started = performance.now();
  const binaryCache = wasmModule.export_parsed_source_roots_binary(JSON.stringify(uris));
  const exportBinaryMs = ms(started);

  wasmModule.clear_source_root_cache();
  started = performance.now();
  const restoredCount = wasmModule.merge_parsed_source_roots_binary(binaryCache);
  const restoreBinaryMs = ms(started);
  const documentCountAfterRestore = wasmModule.get_source_root_document_count();

  const compileAfterBinaryRestore = compileModels(wasmModule, models);

  const report = {
    pkgSubdir,
    mslRoot: path.resolve(mslRoot),
    fileCount: uris.length,
    lazyReport,
    sourceJsonBytes: Buffer.byteLength(sourceRootsJson),
    binaryCacheBytes: binaryCache.length,
    readMs,
    zipReadMs: zipTimings?.zipReadMs,
    zipParseMs: zipTimings?.zipParseMs,
    zipExtractMs: zipTimings?.zipExtractMs,
    stringifyMs,
    wasmLoadSourceRootsMs: loadMs,
    exportParsedBinaryMs: exportBinaryMs,
    restoreParsedBinaryMs: restoreBinaryMs,
    parsedCount: loadResult.parsed_count,
    errorCount: loadResult.error_count,
    documentCountAfterLoad,
    compileAfterTextLoad,
    restoredCount,
    documentCountAfterRestore,
    compileAfterBinaryRestore,
  };

  console.log(JSON.stringify(report, null, 2));
}

main().catch((error) => {
  console.error("[wasm-benchmark] failed:");
  console.error(error);
  process.exit(1);
});
