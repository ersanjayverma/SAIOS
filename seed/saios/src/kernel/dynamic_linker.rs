use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::saifs;
use crate::shell::programs::BinaryMetadata;

#[derive(Clone, Debug)]
pub struct LinkReport {
    pub interpreter: String,
    pub libraries: Vec<String>,
    pub resolved_symbols: Vec<String>,
}

#[derive(Clone, Debug)]
struct SharedLibraryInfo {
    soname: String,
    exports: Vec<String>,
}

fn parse_csv_values(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let value = item.trim();
        if !value.is_empty() {
            out.push(value.to_string());
        }
    }
    out
}

fn library_path_candidates(name: &str) -> [String; 2] {
    [
        alloc::format!("/lib/{}", name),
        alloc::format!("/usr/lib/{}", name),
    ]
}

fn resolve_library_path(name: &str) -> Option<String> {
    for candidate in library_path_candidates(name) {
        if saifs::open(candidate.as_str()).is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn parse_library(path: &str) -> Result<SharedLibraryInfo, &'static str> {
    let text = saifs::read_text(path).map_err(|_| "ld: shared library read failed")?;
    if !text.starts_with("SAIOS_SO_V1") {
        return Err("ld: invalid shared library format");
    }

    let mut soname: Option<String> = None;
    let mut exports: Vec<String> = Vec::new();

    for line in text.lines() {
        if let Some(raw) = line.strip_prefix("soname=") {
            let value = raw.trim();
            if !value.is_empty() {
                soname = Some(value.to_string());
            }
            continue;
        }

        if let Some(raw) = line.strip_prefix("exports=") {
            exports = parse_csv_values(raw);
            continue;
        }
    }

    Ok(SharedLibraryInfo {
        soname: soname.ok_or("ld: missing soname")?,
        exports,
    })
}

pub fn link_image(_image_path: &str, meta: &BinaryMetadata) -> Result<LinkReport, &'static str> {
    if !meta.dynamic {
        return Ok(LinkReport {
            interpreter: "-".to_string(),
            libraries: Vec::new(),
            resolved_symbols: Vec::new(),
        });
    }

    let interpreter = meta
        .interpreter
        .clone()
        .unwrap_or_else(|| "/lib/ld-saios.so".to_string());
    if saifs::open(interpreter.as_str()).is_err() {
        return Err("ld: interpreter missing");
    }

    let mut libraries = Vec::new();
    let mut exported_symbols = Vec::new();

    for needed in &meta.needed_libraries {
        let path = resolve_library_path(needed.as_str()).ok_or("ld: needed library missing")?;
        let info = parse_library(path.as_str())?;
        libraries.push(info.soname.clone());

        for symbol in info.exports {
            if !exported_symbols.iter().any(|s| s == &symbol) {
                exported_symbols.push(symbol);
            }
        }
    }

    let mut resolved_symbols = Vec::new();
    for required in &meta.required_symbols {
        if !exported_symbols.iter().any(|s| s == required) {
            return Err("ld: unresolved symbol");
        }
        resolved_symbols.push(required.clone());
    }

    Ok(LinkReport {
        interpreter,
        libraries,
        resolved_symbols,
    })
}
