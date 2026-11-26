//! the browser's types, generated from the wire: hand-written brands for
//! the primitives, then every wire type as ts-rs declares it, laid out one
//! member per line

use std::collections::BTreeSet;

use ts_rs::{Config, TS};

use crate::web::wire::*;

/// a primitive the browser keeps apart from its kind: `Ms` is a number, the
/// names are branded strings
macro_rules! brand {
    ($type:ty, $name:literal) => {
        impl TS for $type {
            type WithoutGenerics = Self;
            type OptionInnerType = Self;

            fn name(_: &Config) -> String {
                $name.into()
            }

            fn inline(config: &Config) -> String {
                Self::name(config)
            }
        }
    };
}

brand!(Ms, "Ms");
brand!(Hostname, "Hostname");
brand!(ServiceName, "ServiceName");
brand!(AlertId, "AlertId");

/// what `web/src/lib/wire/types.ts` holds
pub fn typescript() -> String {
    let config = Config::new().with_large_int("number");
    let declarations = [
        (Hello::docs(), Hello::decl(&config)),
        (SignIn::docs(), SignIn::decl(&config)),
        (Jwk::docs(), Jwk::decl(&config)),
        (SignedIn::docs(), SignedIn::decl(&config)),
        (Viewer::docs(), Viewer::decl(&config)),
        (PushView::docs(), PushView::decl(&config)),
        (StreamEvent::docs(), StreamEvent::decl(&config)),
        (Snapshot::docs(), Snapshot::decl(&config)),
        (Node::docs(), Node::decl(&config)),
        (Machine::docs(), Machine::decl(&config)),
        (Os::docs(), Os::decl(&config)),
        (Hardware::docs(), Hardware::decl(&config)),
        (MachineState::docs(), MachineState::decl(&config)),
        (Vitals::docs(), Vitals::decl(&config)),
        (Units::docs(), Units::decl(&config)),
        (FailedUnit::docs(), FailedUnit::decl(&config)),
        (Service::docs(), Service::decl(&config)),
        (UnitStatus::docs(), UnitStatus::decl(&config)),
        (UnitState::docs(), UnitState::decl(&config)),
        (Heartbeat::docs(), Heartbeat::decl(&config)),
        (HeartbeatState::docs(), HeartbeatState::decl(&config)),
        (Probe::docs(), Probe::decl(&config)),
        (ProbeKind::docs(), ProbeKind::decl(&config)),
        (ProbeView::docs(), ProbeView::decl(&config)),
        (Alert::docs(), Alert::decl(&config)),
        (Severity::docs(), Severity::decl(&config)),
        (AlertState::docs(), AlertState::decl(&config)),
        (AlertDetail::docs(), AlertDetail::decl(&config)),
        (TimelineEntry::docs(), TimelineEntry::decl(&config)),
        (Series::docs(), Series::decl(&config)),
        (Subject::docs(), Subject::decl(&config)),
        (Metric::docs(), Metric::decl(&config)),
        (SeriesData::docs(), SeriesData::decl(&config)),
        (Line::docs(), Line::decl(&config)),
        (LogPage::docs(), LogPage::decl(&config)),
        (LogLine::docs(), LogLine::decl(&config)),
        (Level::docs(), Level::decl(&config)),
        (LogEvent::docs(), LogEvent::decl(&config)),
        (FixtureClock::docs(), FixtureClock::decl(&config)),
    ];

    let mut source = String::from(
        "// generated from src/web/wire.rs by `PHOEBE_BLESS=1 cargo test web::`; change the rust types, not this file\n\
         \n\
         /** epoch milliseconds */\n\
         export type Ms = number;\n\
         /** a machine's name in the registry, nodes included */\n\
         export type Hostname = string & { readonly __host: unique symbol };\n\
         /** a service's name in the registry */\n\
         export type ServiceName = string & { readonly __service: unique symbol };\n\
         /** an alert's alertmanager fingerprint, 16 hex digits */\n\
         export type AlertId = string & { readonly __alert: unique symbol };\n",
    );
    for (docs, declaration) in declarations {
        source.push('\n');
        source.push_str(&layout(docs.as_deref(), &declaration));
    }
    source
}

/// `type X = …;` as ts-rs writes it, exported and laid out for reading
fn layout(docs: Option<&str>, declaration: &str) -> String {
    let declaration = one_line_docs(declaration);
    let (name, body) = declaration
        .strip_prefix("type ")
        .and_then(|declaration| declaration.split_once(" = "))
        .unwrap();
    let body = body.strip_suffix(';').unwrap();

    let mut out = docs
        .map(one_line_docs)
        .map(|docs| docs + "\n")
        .unwrap_or_default();
    let members = split(body, '|');
    match members.as_slice() {
        [object] if object.starts_with('{') => {
            out += &format!("export type {name} = {};\n", member(object, ""));
        }
        members if !body.contains("/*") && body.len() <= 72 => {
            let members: Vec<String> = members.iter().map(|m| member(m, "  ")).collect();
            out += &format!("export type {name} = {};\n", members.join(" | "));
        }
        members => {
            out += &format!("export type {name} =");
            for member in members {
                let (docs, member) = leading_docs(member);
                if let Some(docs) = docs {
                    out += &format!("\n  {docs}");
                }
                out += &format!("\n  | {}", self::member(member, "    "));
            }
            out += ";\n";
        }
    }
    out
}

/// a union member: objects, named types, or their intersection
fn member(text: &str, indent: &str) -> String {
    split(text, '&')
        .iter()
        .map(|part| match part.starts_with('{') {
            true => object(part, indent),
            false => part.clone(),
        })
        .collect::<Vec<_>>()
        .join(" & ")
}

/// `{ a: A, b: B, }`, on one line unless a field carries docs
fn object(text: &str, indent: &str) -> String {
    let inner = &text[1..text.len() - 1];
    let parts = split(inner, ',');
    let fields: Vec<(Option<&str>, String)> = parts
        .iter()
        .map(|field| {
            let (docs, field) = leading_docs(field);
            let field = match field.split_once("\": ") {
                Some((key, value)) if field.starts_with('"') => format!("{}: {value}", &key[1..]),
                _ => field.to_owned(),
            };
            (docs, field)
        })
        .collect();

    if indent.is_empty() || fields.iter().any(|(docs, _)| docs.is_some()) {
        let mut out = String::from("{");
        for (docs, field) in &fields {
            if let Some(docs) = docs {
                out += &format!("\n{indent}  {docs}");
            }
            out += &format!("\n{indent}  {field};");
        }
        out + &format!("\n{indent}}}")
    } else {
        let fields: Vec<&str> = fields.iter().map(|(_, field)| field.as_str()).collect();
        format!("{{ {} }}", fields.join("; "))
    }
}

/// a `/** … */` that starts `text`, and the rest
fn leading_docs(text: &str) -> (Option<&str>, &str) {
    match text.strip_prefix("/**") {
        Some(rest) => {
            let end = rest.find("*/").unwrap() + 2;
            (Some(&text[..end + 3]), rest[end..].trim_start())
        }
        None => (None, text),
    }
}

/// every doc comment as `/** text */` on the line of what it documents
fn one_line_docs(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("/**") {
        out.push_str(rest[..start].trim_end());
        if !out.is_empty() {
            out.push(' ');
        }
        let end = start + rest[start..].find("*/").unwrap();
        let words: Vec<&str> = rest[start + 3..end]
            .lines()
            .map(|line| line.trim().trim_start_matches('*').trim())
            .filter(|line| !line.is_empty())
            .collect();
        out += &format!("/** {} */ ", words.join(" "));
        rest = rest[end + 2..].trim_start();
    }
    out.push_str(rest);
    out.trim_end().to_owned()
}

/// splits at `separator` outside brackets, strings and comments
fn split(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    let mut chars = text.char_indices().peekable();
    while let Some((at, char)) = chars.next() {
        match char {
            '"' => {
                for (_, char) in chars.by_ref() {
                    if char == '"' {
                        break;
                    }
                }
            }
            '/' if text[at..].starts_with("/*") => {
                let end = at + text[at..].find("*/").unwrap() + 2;
                while chars.next_if(|(at, _)| *at < end).is_some() {}
            }
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' => depth -= 1,
            char if char == separator && depth == 0 => {
                parts.push(text[start..at].trim().to_owned());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim().to_owned());
    parts.retain(|part| !part.is_empty());
    parts
}

/// the string literals in typescript source, outside comments
pub fn literals(source: &str) -> BTreeSet<String> {
    code(source)
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// the capitalized names in typescript source, outside comments and strings
pub fn names(source: &str) -> BTreeSet<String> {
    code(source)
        .split('"')
        .step_by(2)
        .flat_map(|code| code.split(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
        .filter(|word| word.starts_with(|c: char| c.is_ascii_uppercase()))
        .map(str::to_owned)
        .collect()
}

/// typescript source without its comments
fn code(source: &str) -> String {
    let mut code = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        code.push_str(&rest[..start]);
        rest = &rest[start + rest[start..].find("*/").unwrap() + 2..];
    }
    code.push_str(rest);
    code.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}
