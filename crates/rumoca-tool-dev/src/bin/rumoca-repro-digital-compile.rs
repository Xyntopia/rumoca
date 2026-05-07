use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use rumoca_compile::Session;
use rumoca_compile::compile::{CompilationMode, SourceRootKind};
use rumoca_compile::parsing::parse_source_to_ast;
use zip::ZipArchive;

#[derive(Parser, Debug)]
#[command(name = "rumoca-repro-digital-compile")]
struct Args {
    #[arg(long)]
    msl_zip: PathBuf,
    #[arg(long)]
    model: String,
    #[arg(long, default_value = "phases")]
    mode: String,
}

fn sanitize_library_path(path: &str) -> String {
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() > 1
        && (parts[0].contains("Library")
            || parts[0].eq_ignore_ascii_case("MSL")
            || parts[0].eq_ignore_ascii_case("ModelicaStandardLibrary"))
    {
        return parts[1..].join("/");
    }
    if let Some(first) = parts.first_mut() {
        *first = first.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
    }
    parts.join("/")
}

fn load_msl_sources(msl_zip: &Path) -> Result<BTreeMap<String, String>> {
    let file = File::open(msl_zip)
        .with_context(|| format!("failed opening MSL zip: {}", msl_zip.display()))?;
    let mut archive = ZipArchive::new(file).context("failed reading zip archive")?;
    let mut out = BTreeMap::new();
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).context("failed reading zip entry")?;
        let raw_path = entry.name().to_string();
        let lower = raw_path.to_lowercase();
        if !lower.ends_with(".mo") {
            continue;
        }
        if raw_path.contains("Test") || raw_path.contains("Obsolete") {
            continue;
        }
        let mut buf = String::new();
        entry
            .read_to_string(&mut buf)
            .with_context(|| format!("failed reading utf8 Modelica source: {raw_path}"))?;
        out.insert(sanitize_library_path(&raw_path), buf);
    }
    Ok(out)
}

fn main() -> Result<()> {
    let args = Args::parse();
    eprintln!(
        "[repro] loading msl zip={} model={}",
        args.msl_zip.display(),
        args.model
    );
    let sources = load_msl_sources(&args.msl_zip)?;
    eprintln!("[repro] msl sources loaded count={}", sources.len());

    let mut session = Session::default();
    let parsed = sources
        .into_iter()
        .map(|(uri, source)| {
            let parsed = parse_source_to_ast(&source, &uri)
                .unwrap_or_else(|err| panic!("parse failed for {uri}: {err}"));
            (uri, parsed)
        })
        .collect::<Vec<_>>();
    let inserted = session.replace_parsed_source_set("msl", SourceRootKind::External, parsed, None);
    eprintln!("[repro] parsed source set inserted={inserted}");

    match args.mode.as_str() {
        "phases" => {
            eprintln!("[repro] compile_model_phases start");
            let result = session.compile_model_phases(&args.model);
            eprintln!("[repro] compile_model_phases result={result:?}");
            if result.is_err() {
                bail!("compile failed (phases)");
            }
        }
        "strict" => {
            eprintln!("[repro] compile_model_with_mode(strict) start");
            let report = session.compile_model_with_mode(&args.model, CompilationMode::StrictReachable);
            eprintln!(
                "[repro] strict report: failures={} has_requested={}",
                report.failures.len(),
                report.requested_result.is_some()
            );
            if let Some(result) = report.requested_result {
                let summary = match result {
                    rumoca_compile::compile::PhaseResult::Success(_) => "Success",
                    rumoca_compile::compile::PhaseResult::NeedsInner { .. } => "NeedsInner",
                    rumoca_compile::compile::PhaseResult::Failed { .. } => "Failed",
                };
                eprintln!("[repro] strict requested_result={summary}");
            }
            if !report.failures.is_empty() {
                bail!("compile failed (strict)");
            }
        }
        other => bail!("unsupported --mode '{other}', expected 'phases' or 'strict'"),
    }
    Ok(())
}
