use proc_macro2::{Delimiter, Literal, TokenStream, TokenTree};

#[derive(Clone, Debug)]
pub(crate) enum Tree {
    Ident(String, usize),
    Punct(char, usize),
    Literal(String, Option<String>, usize),
    Group(Delimiter, Vec<Tree>, usize),
}

impl Tree {
    pub(crate) fn line(&self) -> usize {
        match self {
            Tree::Ident(_, line)
            | Tree::Punct(_, line)
            | Tree::Literal(_, _, line)
            | Tree::Group(_, _, line) => *line,
        }
    }

    pub(crate) fn is_ident(&self, name: &str) -> bool {
        matches!(self, Tree::Ident(i, _) if i == name)
    }

    pub(crate) fn is_punct(&self, c: char) -> bool {
        matches!(self, Tree::Punct(p, _) if *p == c)
    }

    fn brackets(&self) -> Option<&[Tree]> {
        match self {
            Tree::Group(Delimiter::Bracket, body, _) => Some(body),
            _ => None,
        }
    }
}

pub(crate) fn parse(src: &str) -> Result<Vec<Tree>, String> {
    let stream: TokenStream = src.parse().map_err(|e| format!("does not tokenize: {e}"))?;
    Ok(trees(stream))
}

fn trees(stream: TokenStream) -> Vec<Tree> {
    stream
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Ident(i) => Tree::Ident(i.to_string(), i.span().start().line),
            TokenTree::Punct(p) => Tree::Punct(p.as_char(), p.span().start().line),
            TokenTree::Literal(l) => {
                let line = l.span().start().line;
                Tree::Literal(l.to_string(), text(l), line)
            }
            TokenTree::Group(g) => {
                Tree::Group(g.delimiter(), trees(g.stream()), g.span_open().start().line)
            }
        })
        .collect()
}

fn text(literal: Literal) -> Option<String> {
    match syn::Lit::new(literal) {
        syn::Lit::Str(s) => Some(s.value()),
        syn::Lit::ByteStr(b) => Some(String::from_utf8_lossy(&b.value()).into_owned()),
        syn::Lit::CStr(c) => Some(c.value().to_string_lossy().into_owned()),
        _ => None,
    }
}

pub(crate) fn decimal(text: &str) -> Option<String> {
    match syn::Lit::new(text.parse::<Literal>().ok()?) {
        syn::Lit::Int(i) => Some(format!("{}{}", i.base10_digits(), i.suffix())),
        _ => None,
    }
}

pub(crate) fn attributes(trees: &[Tree]) -> Vec<(&[Tree], usize)> {
    let mut out = Vec::new();
    for (k, tree) in trees.iter().enumerate() {
        if let Some(body) = tree.brackets() {
            let hash = |i: usize| trees.get(i).is_some_and(|t| t.is_punct('#'));
            let inner = k >= 2 && trees[k - 1].is_punct('!') && hash(k - 2);
            if (k >= 1 && hash(k - 1)) || inner {
                out.push((body, tree.line()));
            }
        }
        if let Tree::Group(_, body, _) = tree {
            out.extend(attributes(body));
        }
    }
    out
}

pub(crate) fn idents(trees: &[Tree]) -> Vec<(&str, usize)> {
    let mut out = Vec::new();
    for tree in trees {
        match tree {
            Tree::Ident(name, line) => out.push((name.as_str(), *line)),
            Tree::Group(_, body, _) => out.extend(idents(body)),
            _ => {}
        }
    }
    out
}

pub(crate) fn items(trees: &[Tree]) -> Vec<&[Tree]> {
    let mut out = Vec::new();
    let mut start = 0;
    for k in 0..trees.len() {
        if k < start {
            continue;
        }
        if k == start {
            let width = attribute_width(&trees[k..]);
            if width != 0 {
                start = k + width;
                continue;
            }
        }
        let item = &trees[start..=k];
        let body = matches!(&trees[k], Tree::Group(Delimiter::Brace, _, _))
            && !item.iter().any(|t| t.is_ident("use"));
        if trees[k].is_punct(';') || body {
            out.push(item);
            start = k + 1;
        }
    }
    if start < trees.len() {
        out.push(&trees[start..]);
    }
    out
}

fn attribute_width(trees: &[Tree]) -> usize {
    match trees {
        [Tree::Punct('#', _), Tree::Group(Delimiter::Bracket, ..), ..] => 2,
        [
            Tree::Punct('#', _),
            Tree::Punct('!', _),
            Tree::Group(Delimiter::Bracket, ..),
            ..,
        ] => 3,
        _ => 0,
    }
}

pub(crate) fn after_path<'a>(trees: &'a [Tree], name: &str) -> Vec<(&'a [Tree], usize)> {
    let mut out = Vec::new();
    for (k, tree) in trees.iter().enumerate() {
        if tree.is_ident(name)
            && trees.get(k + 1).is_some_and(|t| t.is_punct(':'))
            && trees.get(k + 2).is_some_and(|t| t.is_punct(':'))
        {
            out.push((&trees[k + 3..], tree.line()));
        }
        if let Tree::Group(_, body, _) = tree {
            out.extend(after_path(body, name));
        }
    }
    out
}

pub(crate) fn pairs(trees: &[Tree]) -> Vec<(&str, &str, usize)> {
    let mut out = Vec::new();
    for (k, tree) in trees.iter().enumerate() {
        if let (Tree::Ident(a, line), Some(Tree::Ident(b, _))) = (tree, trees.get(k + 1)) {
            out.push((a.as_str(), b.as_str(), *line));
        }
        if let Tree::Group(_, body, _) = tree {
            out.extend(pairs(body));
        }
    }
    out
}

pub(crate) fn macros(trees: &[Tree]) -> Vec<(&str, usize, &[Tree])> {
    let mut out = Vec::new();
    for (k, tree) in trees.iter().enumerate() {
        if let (Tree::Ident(name, line), Some(bang), Some(Tree::Group(_, body, _))) =
            (tree, trees.get(k + 1), trees.get(k + 2))
            && bang.is_punct('!')
        {
            out.push((
                name.strip_prefix("r#").unwrap_or(name),
                *line,
                body.as_slice(),
            ));
        }
        if let Tree::Group(_, body, _) = tree {
            out.extend(macros(body));
        }
    }
    out
}

pub(crate) fn assigns(trees: &[Tree], name: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (k, tree) in trees.iter().enumerate() {
        if tree.is_ident(name) && trees.get(k + 1).is_some_and(|t| t.is_punct('=')) {
            out.push(tree.line());
        }
        if let Tree::Group(_, body, _) = tree {
            out.extend(assigns(body, name));
        }
    }
    out
}
