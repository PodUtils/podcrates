//! Read each file of `services/`, check it against the schema
//! (`src/schema.rs`), and write the built-in set as Rust constants to
//! `$OUT_DIR/services.rs`: no parse at run time. A fault fails the build
//! with the file and the field.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
#[path = "src/schema.rs"]
mod schema;

use schema::{Day, EndpointSpec, Fault, OriginSpec, SourceSpec, Spec, ValueSpec, WatchSpec};

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    println!("cargo::rerun-if-changed=services");
    println!("cargo::rerun-if-changed=src/schema.rs");
    match generate(&root) {
        Ok(code) => std::fs::write(out.join("services.rs"), code).expect("writing services.rs"),
        Err(fault) => {
            // A fault of the data stops the build, with the file and the field.
            eprintln!("error: pod-services: {fault}");
            std::process::exit(1);
        }
    }
}

fn fault(file: &str, field: &str, why: impl Into<String>) -> Fault {
    Fault { file: file.to_string(), field: field.to_string(), why: why.into() }
}

/// Each `services/*.toml`, checked, as Rust code.
fn generate(root: &Path) -> Result<String, Fault> {
    let dir = root.join("services");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| fault("services", "the directory", e.to_string()))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".toml"))
        .collect();
    names.sort();
    if names.is_empty() {
        return Err(fault("services", "the directory", "no file ID.toml"));
    }
    let mut specs = Vec::new();
    for name in &names {
        let file = format!("services/{name}");
        let text = std::fs::read_to_string(dir.join(name)).map_err(|e| fault(&file, "the file", e.to_string()))?;
        let spec = schema::parse(&file, &text, true)?;
        for f in &spec.facts {
            if let ValueSpec::File(path) = &f.value {
                let field = format!("fact {:?}.file", f.name);
                std::fs::read_to_string(dir.join(path)).map_err(|e| fault(&file, &field, format!("{path}: {e}")))?;
            }
        }
        specs.push((file, spec));
    }
    schema::check_set(specs.iter().map(|(f, s)| (f.as_str(), s)), true)?;
    // The order of the lists of a kind: the rank, then the id.
    specs.sort_by(|(_, a), (_, b)| {
        (a.rank.as_ref().map_or(u32::MAX, |r| r.0), &a.id).cmp(&(b.rank.as_ref().map_or(u32::MAX, |r| r.0), &b.id))
    });
    let mut code = String::from("// Written by build.rs from services/*.toml. Do not edit.\n\n");
    let mut services = Vec::new();
    for (i, (_, spec)) in specs.iter().enumerate() {
        services.push(service(&mut code, &dir, &format!("S{i}"), spec));
    }
    let _ = writeln!(code, "pub(crate) static BUILTIN: [Service; {}] = [\n{}];", services.len(), services.join(""));
    Ok(code)
}

fn text(s: &str) -> String {
    format!("Cow::Borrowed({s:?})")
}

fn optional(s: &Option<String>) -> String {
    s.as_ref().map_or_else(|| "None".to_string(), |s| format!("Some({})", text(s)))
}

/// A static list of `items` of type `ty` named `name`, and its use.
fn list(code: &mut String, name: &str, ty: &str, items: &[String]) -> String {
    if items.is_empty() {
        return "Cow::Borrowed(&[])".to_string();
    }
    let _ = writeln!(code, "static {name}: [{ty}; {}] = [{}];", items.len(), items.join(", "));
    format!("Cow::Borrowed(&{name})")
}

fn day(d: &Day) -> String {
    format!("Date {{ year: {}, month: {}, day: {} }}", d.year, d.month, d.day)
}

fn source(s: &SourceSpec) -> String {
    let origin = match &s.origin {
        OriginSpec::Url(u) => format!("Origin::Url({})", text(u)),
        OriginSpec::Measured(m) => format!("Origin::Measured({})", text(m)),
        OriginSpec::Ruling(r) => format!("Origin::Ruling({})", text(r)),
    };
    format!("Source {{ origin: {origin}, read: {}, note: {} }}", day(&s.read), optional(&s.note))
}

fn value(code: &mut String, dir: &Path, name: &str, v: &ValueSpec) -> String {
    match v {
        ValueSpec::Text(t) => format!("Value::Text({})", text(t)),
        ValueSpec::Int(i) => format!("Value::Int({i})"),
        ValueSpec::Bool(b) => format!("Value::Bool({b})"),
        ValueSpec::Date(d) => format!("Value::Date({})", day(d)),
        ValueSpec::Texts(t) => {
            let items: Vec<String> = t.iter().map(|s| text(s)).collect();
            format!("Value::Texts({})", list(code, name, "Text", &items))
        }
        ValueSpec::Ints(i) => {
            let items: Vec<String> = i.iter().map(i64::to_string).collect();
            format!("Value::Ints({})", list(code, name, "i64", &items))
        }
        ValueSpec::File(path) => {
            // The path from the manifest of the build that compiles the
            // code, not the absolute path of this run: worktrees that share a
            // target directory share this output (cargo hashes a member by
            // its path in the workspace), and an absolute path names the
            // tree of another build.
            let name = dir.file_name().and_then(|n| n.to_str()).expect("the directory of the data has a name");
            let rel = format!("/{name}/{path}");
            debug_assert!(dir.join(path).is_file(), "{}", dir.join(path).display());
            format!("Value::Text(Cow::Borrowed(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {rel:?}))))")
        }
    }
}

fn endpoint(code: &mut String, name: &str, e: &EndpointSpec) -> String {
    let addresses = list(code, &format!("{name}_A"), "Text", &e.addresses.iter().map(|a| text(a)).collect::<Vec<_>>());
    let alpn = list(code, &format!("{name}_L"), "Text", &e.alpn.iter().map(|a| text(a)).collect::<Vec<_>>());
    format!(
        "Endpoint {{ name: {}, role: {}, scheme: {}, host: {}, port: {}, path: {}, addresses: {addresses}, sni: {}, \
         alpn: {alpn}, source: {} }}",
        text(&e.name),
        optional(&e.role),
        text(&e.scheme),
        text(&e.host),
        e.port,
        optional(&e.path),
        optional(&e.sni),
        source(&e.source),
    )
}

fn watch(code: &mut String, dir: &Path, name: &str, w: &WatchSpec) -> String {
    let args: Vec<String> = w
        .args
        .iter()
        .enumerate()
        .map(|(j, (key, v))| {
            format!("Arg {{ name: {}, value: {} }}", text(key), value(code, dir, &format!("{name}_A{j}"), v))
        })
        .collect();
    let args = list(code, &format!("{name}_A"), "Arg", &args);
    format!("Watch {{ kind: {}, args: {args} }}", text(&w.kind))
}

fn service(code: &mut String, dir: &Path, name: &str, spec: &Spec) -> String {
    let endpoints: Vec<String> =
        spec.endpoints.iter().enumerate().map(|(j, e)| endpoint(code, &format!("{name}_E{j}"), e)).collect();
    let endpoints = list(code, &format!("{name}_E"), "Endpoint", &endpoints);
    let facts: Vec<String> = spec
        .facts
        .iter()
        .enumerate()
        .map(|(j, f)| {
            let v = value(code, dir, &format!("{name}_F{j}"), &f.value);
            format!(
                "Fact {{ name: {}, value: {v}, unit: {}, source: {} }}",
                text(&f.name),
                optional(&f.unit),
                source(&f.source)
            )
        })
        .collect();
    let facts = list(code, &format!("{name}_F"), "Fact", &facts);
    let watches: Vec<String> =
        spec.watches.iter().enumerate().map(|(j, w)| watch(code, dir, &format!("{name}_W{j}"), w)).collect();
    let watches = list(code, &format!("{name}_W"), "Watch", &watches);
    let rank = spec
        .rank
        .as_ref()
        .map_or_else(|| "None".to_string(), |(v, s)| format!("Some(Rank {{ value: {v}, source: {} }})", source(s)));
    format!(
        "    Service {{ id: {}, kind: {}, party: {}, about: {}, rank: {rank}, endpoints: {endpoints}, facts: {facts}, \
         watches: {watches} }},\n",
        text(&spec.id),
        text(&spec.kind),
        text(&spec.party),
        text(&spec.about),
    )
}
