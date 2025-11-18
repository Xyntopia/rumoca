use log::Level;
use md5::{Digest, Md5};
use rumoca::modelica_grammar::ModelicaGrammar;
use rumoca::modelica_parser::parse;
use rumoca::{dae, ir::create_dae::create_dae, ir::flatten::flatten};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    // Better panic messages in the browser console
    console_error_panic_hook::set_once();
    // Default to warn unless user sets something else externally
    let _ = console_log::init_with_level(Level::Warn);
}

fn md5_hex(s: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn web_log(s: &str) {
    log::info!("{}", s);
}

fn to_js_error<E: std::fmt::Display>(e: E) -> JsError {
    JsError::new(&e.to_string())
}

/// A WASM-exported function that mirrors main.rs behavior in-memory:
/// - Parses Modelica
/// - Flattens
/// - Creates DAE
/// - Computes model/template hashes (hex md5)
/// - Renders Jinja template provided as a string
/// Returns the rendered output string or throws a JS error.
#[wasm_bindgen]
pub fn translate_modelica_to_template(
    modelica_source: String,
    template_source: String,
    verbose: Option<bool>,
) -> Result<String, JsError> {
    let verbose = verbose.unwrap_or(false);

    // In the CLI, file_name is used in parse() for diagnostics.
    // Here we fake a filename.
    let file_name = "<memory:modelica.mo>".to_string();
    let model_md5 = md5_hex(&modelica_source);

    let mut modelica_grammar = ModelicaGrammar::new();

    let t0 = js_sys::Date::now();
    match parse(&modelica_source, &file_name, &mut modelica_grammar) {
        Ok(_syntax_tree) => {
            let parse_elapsed = js_sys::Date::now() - t0;
            // Recreate the same logic as main.rs
            let def = modelica_grammar.modelica.clone().expect("failed to parse");

            if verbose {
                web_log(&format!(
                    "Parsing took {} milliseconds.",
                    parse_elapsed as u128
                ));
                web_log(&format!("Success!\n{:#?}", def));
            }

            // flatten
            let mut fclass = flatten(&def).map_err(to_js_error)?;
            if verbose {
                web_log(&format!("Flattened:\n{:#?}", fclass));
            }

            // create DAE
            let mut dae_ast = create_dae(&mut fclass).map_err(to_js_error)?;
            dae_ast.model_hash = model_md5;

            if verbose {
                web_log(&format!("DAE:\n{:#?}", dae_ast));
            }

            // template hash and rendering
            let template_md5 = md5_hex(&template_source);
            dae_ast.template_hash = template_md5;

            let rendered = dae::jinja::render_template_from_str(dae_ast, &template_source)
                .map_err(to_js_error)?;

            Ok(rendered)
        }
        Err(e) => Err(JsError::new(&format!("{e}"))),
    }
}
