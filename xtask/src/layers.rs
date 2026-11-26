use crate::{
    Findings,
    syntax::{self, Tree},
    walk,
};
use proc_macro2::Delimiter;
use std::path::Path;

pub(crate) const ORDER: [&str; 6] = ["model", "xc", "pw", "paw", "ks", "io"];

const FORMATS: [&str; 4] = ["toml", "serde", "serde_json", "roxmltree"];

pub(crate) fn check(root: &Path) -> Findings {
    let mut findings = Vec::new();
    for file in walk::files(root, "src")
        .into_iter()
        .filter(|f| f.ends_with(".rs"))
    {
        let path = module_path(&file);
        let layer = path[0].as_str();
        if matches!(layer, "lib" | "main") {
            continue;
        }
        let Some(rank) = ORDER.iter().position(|name| *name == layer) else {
            findings.push(format!(
                "layers: {file} is in unknown layer `{layer}`; add it to ORDER in xtask/src/layers.rs"
            ));
            continue;
        };
        let trees = match syntax::parse(&walk::read(root, &file)) {
            Ok(trees) => trees,
            Err(e) => {
                findings.push(format!("layers: {file} {e}"));
                continue;
            }
        };
        for (alias, line) in aliases(&trees) {
            findings.push(format!(
                "layers: {file}:{line} uses `{alias}`; name each layer that it uses"
            ));
        }
        if layer != "io" {
            let mut named: Vec<&str> = Vec::new();
            for (name, line) in syntax::idents(&trees) {
                if FORMATS.contains(&name) && !named.contains(&name) {
                    named.push(name);
                    findings.push(format!(
                        "layers: {file}:{line} uses a format crate (`{name}`); only src/io/ knows file formats"
                    ));
                }
            }
        }
        for used in targets(&trees, path.len()) {
            match ORDER.iter().position(|name| *name == used) {
                Some(r) if r <= rank => {}
                Some(_) => findings.push(format!(
                    "layers: {file} uses `{used}`; `{layer}` may use only {:?}",
                    &ORDER[..rank]
                )),
                None => findings.push(format!(
                    "layers: {file} uses `crate::{used}`, which is not a layer"
                )),
            }
        }
    }
    findings
}

fn aliases(trees: &[Tree]) -> Vec<(&'static str, usize)> {
    let mut out = Vec::new();
    for (a, b, line) in syntax::pairs(trees) {
        match (a, b) {
            ("crate", "as") => out.push(("crate as", line)),
            ("crate", "self") => out.push(("extern crate self", line)),
            _ => {}
        }
    }
    for (rest, line) in syntax::after_path(trees, "crate") {
        match rest.first() {
            Some(t) if t.is_punct('*') => out.push(("crate::*", line)),
            Some(Tree::Group(Delimiter::Brace, body, _)) => {
                if body.iter().any(|t| t.is_punct('*')) {
                    out.push(("crate::{…, *}", line));
                }
                if body.iter().any(|t| t.is_ident("self")) {
                    out.push(("crate::{self", line));
                }
            }
            _ => {}
        }
    }
    for (rest, line) in syntax::after_path(trees, "super") {
        if rest.first().is_some_and(|t| t.is_punct('*')) {
            out.push(("super::*", line));
        }
    }
    for (rest, line) in syntax::after_path(trees, "self") {
        if rest.first().is_some_and(|t| t.is_ident("super")) {
            out.push(("self::super", line));
        }
    }
    out
}

fn module_path(file: &str) -> Vec<String> {
    file.trim_start_matches("src/")
        .trim_end_matches(".rs")
        .split('/')
        .map(str::to_owned)
        .collect()
}

fn targets(trees: &[Tree], depth: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (rest, _) in syntax::after_path(trees, "crate") {
        out.extend(heads(rest));
    }
    climbs(trees, depth, &mut out);
    out.retain(|name| name != "self" && name != "super");
    out.sort();
    out.dedup();
    out
}

fn climbs(trees: &[Tree], depth: usize, out: &mut Vec<String>) {
    for (k, tree) in trees.iter().enumerate() {
        if let Tree::Group(_, body, _) = tree {
            climbs(body, depth, out);
        }
        let up = trees[k..]
            .chunks(3)
            .take_while(|c| {
                matches!(c, [s, a, b] if s.is_ident("super") && a.is_punct(':') && b.is_punct(':'))
            })
            .count();
        if up >= depth {
            out.extend(heads(&trees[k + 3 * up..]));
        }
    }
}

fn heads(rest: &[Tree]) -> Vec<String> {
    match rest.first() {
        Some(Tree::Ident(name, _)) => vec![name.clone()],
        Some(Tree::Group(Delimiter::Brace, body, _)) => body
            .split(|t| t.is_punct(','))
            .filter_map(|segment| match segment.first() {
                Some(Tree::Ident(name, _)) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}
