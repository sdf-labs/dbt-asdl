use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::asdl::{self, Constructor, Field, Module, Type, TypeKind};

const BUILTIN_RUST: &[(&str, &str)] = &[
    ("identifier", "dbt_asdl::Identifier"),
    ("string", "&'a str"),
    ("byte", "u8"),
    ("int", "i32"),
    ("long", "i64"),
    ("bool", "bool"),
];

const OWNED_STRING: &str = "Box<str>";

const SCALAR_BUILTINS: &[&str] = &["byte", "int", "long", "bool"];

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "gen", "try", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodegenError(pub(crate) String);

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "asdl_rs: error: {}", self.0)
    }
}

impl std::error::Error for CodegenError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedRust {
    pub(crate) shared: String,
    pub(crate) arena: String,
    pub(crate) owned: String,
    pub(crate) views: String,
    pub(crate) sexpr: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Arena,
    Owned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoKind {
    Product,
    SimpleSum,
    CompoundSum,
}

#[derive(Debug, Clone)]
struct TypeInfo {
    name: String,
    rust: String,
    attributes: Vec<Field>,
    kind: InfoKind,
    fields: Vec<Field>,
    constructors: Vec<Constructor>,
    plain: bool,
    copyable: bool,
    arena: bool,
    shared: bool,
}

impl TypeInfo {
    fn from_dfn(dfn: &Type) -> Self {
        let name = dfn.name.clone();
        let rust = camel(&dfn.name);
        let attributes = dfn.value.attributes().to_vec();
        match &dfn.value {
            TypeKind::Product(prod) => Self {
                name,
                rust,
                attributes,
                kind: InfoKind::Product,
                fields: prod.fields.clone(),
                constructors: Vec::new(),
                plain: false,
                copyable: false,
                arena: false,
                shared: false,
            },
            TypeKind::Sum(sum) => {
                let fieldless = sum.types.iter().all(|c| c.fields.is_empty());
                let kind = if fieldless {
                    InfoKind::SimpleSum
                } else {
                    InfoKind::CompoundSum
                };
                Self {
                    name,
                    rust,
                    attributes,
                    kind,
                    fields: Vec::new(),
                    constructors: sum.types.clone(),
                    plain: false,
                    copyable: false,
                    arena: false,
                    shared: false,
                }
            }
        }
    }

    fn compound(&self) -> bool {
        self.kind != InfoKind::SimpleSum && !self.plain
    }

    fn tp(&self) -> &'static str {
        if self.arena { "<'a>" } else { "" }
    }
}

fn builtin_rust(ty: &str) -> Option<&'static str> {
    BUILTIN_RUST
        .iter()
        .find_map(|&(k, v)| (k == ty).then_some(v))
}

fn is_scalar_builtin(ty: &str) -> bool {
    SCALAR_BUILTINS.contains(&ty)
}

pub(crate) fn camel(name: &str) -> String {
    let mut out = String::new();
    for part in name.split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            for c in first.to_uppercase() {
                out.push(c);
            }
            out.push_str(chars.as_str());
        }
    }
    out
}

pub(crate) fn uncamel(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        for c in ch.to_lowercase() {
            out.push(c);
        }
    }
    out
}

pub(crate) fn rust_field(name: &str) -> String {
    if RUST_KEYWORDS.contains(&name) {
        format!("r#{name}")
    } else {
        name.to_string()
    }
}

fn field_name(f: &Field) -> &str {
    f.name.as_deref().unwrap_or("")
}

fn all_fields(info: &TypeInfo) -> Vec<Field> {
    let mut fields = Vec::new();
    fields.extend(info.attributes.iter().cloned());
    fields.extend(info.fields.iter().cloned());
    for c in &info.constructors {
        fields.extend(c.fields.iter().cloned());
    }
    fields
}

fn compute_value_semantics(order: &[String], infos: &mut HashMap<String, TypeInfo>) {
    fn is_plain(
        name: &str,
        infos: &HashMap<String, TypeInfo>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let Some(info) = infos.get(name) else {
            return true;
        };
        if info.kind != InfoKind::Product || visiting.contains(name) {
            return false;
        }
        visiting.insert(name.to_string());
        let res = all_fields(info).iter().all(|f| {
            let t = f.ty.as_str();
            if builtin_rust(t).is_some()
                || infos.get(t).is_none_or(|i| i.kind == InfoKind::SimpleSum)
            {
                return true;
            }
            is_plain(t, infos, visiting)
        });
        visiting.remove(name);
        res
    }

    fn is_copyable(
        name: &str,
        infos: &HashMap<String, TypeInfo>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let Some(info) = infos.get(name) else {
            return true;
        };
        if info.kind == InfoKind::SimpleSum {
            return true;
        }
        if info.kind != InfoKind::Product || visiting.contains(name) {
            return false;
        }
        visiting.insert(name.to_string());
        let res = all_fields(info).iter().all(|f| {
            if f.seq {
                return false;
            }
            if builtin_rust(&f.ty).is_some() {
                return true;
            }
            is_copyable(&f.ty, infos, visiting)
        });
        visiting.remove(name);
        res
    }

    fn is_shared(
        name: &str,
        infos: &HashMap<String, TypeInfo>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let info = &infos[name];
        if info.kind != InfoKind::Product || visiting.contains(name) {
            return false;
        }
        visiting.insert(name.to_string());
        let res = all_fields(info).iter().all(|f| {
            if f.seq || f.ty == "string" {
                return false;
            }
            let Some(t) = infos.get(&f.ty) else {
                return true;
            };
            if t.kind == InfoKind::SimpleSum {
                return true;
            }
            is_shared(&f.ty, infos, visiting)
        });
        visiting.remove(name);
        res
    }

    fn needs_arena(
        name: &str,
        infos: &HashMap<String, TypeInfo>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let info = &infos[name];
        if info.kind == InfoKind::SimpleSum {
            return false;
        }
        for f in all_fields(info) {
            if f.seq || f.ty == "string" {
                return true;
            }
            if let Some(t) = infos.get(&f.ty)
                && t.kind != InfoKind::SimpleSum
                && !visiting.contains(&f.ty)
            {
                visiting.insert(name.to_string());
                let sub = t.compound() || needs_arena(&f.ty, infos, visiting);
                visiting.remove(name);
                if sub {
                    return true;
                }
            }
        }
        false
    }

    for name in order {
        if infos[name].kind == InfoKind::Product {
            let plain = is_plain(name, infos, &mut HashSet::new());
            let copyable = is_copyable(name, infos, &mut HashSet::new());
            let shared = plain && is_shared(name, infos, &mut HashSet::new());
            let info = infos.get_mut(name).unwrap();
            info.plain = plain;
            info.copyable = copyable;
            info.shared = shared;
        }
    }
    for name in order {
        let mut visiting = HashSet::new();
        visiting.insert(name.clone());
        let arena = needs_arena(name, infos, &mut visiting);
        infos.get_mut(name).unwrap().arena = arena;
    }
}

fn rust_type(
    field: &Field,
    infos: &HashMap<String, TypeInfo>,
    qual: &str,
    backend: Backend,
) -> Result<String, CodegenError> {
    let (base, linked, tp) = match infos.get(&field.ty) {
        None => {
            let Some(mut base) = builtin_rust(&field.ty).map(str::to_string) else {
                return Err(CodegenError(format!(
                    "unsupported field type '{}' (field '{}')",
                    field.ty,
                    field_name(field)
                )));
            };
            if backend == Backend::Owned && field.ty == "string" {
                base = OWNED_STRING.to_string();
            }
            (base, false, "")
        }
        Some(info) if info.kind == InfoKind::SimpleSum || info.shared => {
            (format!("{qual}{}", info.rust), false, "")
        }
        Some(info) => {
            let tp = if backend == Backend::Arena {
                info.tp()
            } else {
                ""
            };
            (format!("{qual}{}", info.rust), info.compound(), tp)
        }
    };
    let item = match backend {
        Backend::Arena => {
            if linked {
                format!("{base}Ref<'a>")
            } else {
                format!("{base}{tp}")
            }
        }
        Backend::Owned => {
            if linked {
                format!("Box<{base}>")
            } else {
                base.clone()
            }
        }
    };
    if field.seq {
        return Ok(match backend {
            Backend::Arena => format!("Vec<'a, {item}>"),
            Backend::Owned => format!("Vec<{}>", if linked { base } else { item }),
        });
    }
    if field.opt {
        return Ok(format!("Option<{item}>"));
    }
    Ok(item)
}

fn validate_fields(where_ctx: &str, fields: &[Field]) -> Result<(), CodegenError> {
    for f in fields {
        let Some(name) = &f.name else {
            return Err(CodegenError(format!(
                "{where_ctx}: fields must be named (got positional '{}')",
                f.ty
            )));
        };
        if f.quantifiers.len() > 1 {
            return Err(CodegenError(format!(
                "{where_ctx}.{name}: at most one quantifier (? or *) is supported"
            )));
        }
    }
    Ok(())
}

fn validate(mod_node: &Module) -> Result<(), CodegenError> {
    for dfn in &mod_node.dfns {
        match &dfn.value {
            TypeKind::Product(prod) => {
                validate_fields(&dfn.name, &prod.fields)?;
            }
            TypeKind::Sum(sum) => {
                if !sum.attributes.is_empty() && sum.types.iter().all(|c| c.fields.is_empty()) {
                    return Err(CodegenError(format!(
                        "{}: attributes on a field-less sum make no sense",
                        dfn.name
                    )));
                }
                for c in &sum.types {
                    validate_fields(&format!("{}.{}", dfn.name, c.name), &c.fields)?;
                }
            }
        }
        validate_fields(&format!("{} attributes", dfn.name), dfn.value.attributes())?;
    }
    Ok(())
}

fn seq_list_types(mod_node: &Module, infos: &HashMap<String, TypeInfo>) -> HashSet<String> {
    let mut out = HashSet::new();
    for dfn in &mod_node.dfns {
        let info = &infos[&dfn.name];
        for f in all_fields(info) {
            if !f.seq {
                continue;
            }
            if let Some(t) = infos.get(&f.ty)
                && t.kind != InfoKind::SimpleSum
                && !t.shared
            {
                out.insert(f.ty.clone());
            }
        }
    }
    out
}

fn validate_view_names(
    mod_node: &Module,
    order: &[String],
    infos: &HashMap<String, TypeInfo>,
) -> Result<(), CodegenError> {
    let taken: HashSet<String> = infos.values().map(|info| info.rust.clone()).collect();
    let mut generated: HashSet<String> = HashSet::new();

    let mut claim = |name: &str| -> Result<(), CodegenError> {
        if taken.contains(name) || generated.contains(name) {
            return Err(CodegenError(format!(
                "generated read-API name '{name}' collides with an existing name"
            )));
        }
        generated.insert(name.to_string());
        Ok(())
    };

    claim("IdentList")?;
    claim("StringList")?;
    claim("StringListIter")?;
    for t in seq_list_types(mod_node, infos) {
        let r = &infos[&t].rust;
        claim(&format!("{r}List"))?;
        claim(&format!("{r}ListIter"))?;
    }
    for name in order {
        let info = &infos[name];
        let per_backend = info.kind != InfoKind::SimpleSum && !info.shared;
        if per_backend {
            claim(&format!("{}Ref", info.rust))?;
        }
        for a in &info.attributes {
            let aname = field_name(a);
            if aname == "kind" || aname == "view" {
                return Err(CodegenError(format!(
                    "{}: attribute '{aname}' collides with a generated method",
                    info.name
                )));
            }
        }
        if info.kind == InfoKind::Product {
            continue;
        }
        claim(&format!("{}Kind", info.rust))?;
        claim(&format!("{}View", info.rust))?;
        for c in &info.constructors {
            if !info.attributes.is_empty() || !c.fields.is_empty() {
                claim(&format!("{}{}View", info.rust, camel(&c.name)))?;
            }
            for f in &c.fields {
                if field_name(f) == "node" {
                    return Err(CodegenError(format!(
                        "{}.{}.node: field name reserved for view structs",
                        info.name, c.name
                    )));
                }
            }
        }
    }
    Ok(())
}

fn data_attrs(info: &TypeInfo) -> Vec<Field> {
    info.attributes
        .iter()
        .filter(|a| !field_name(a).starts_with("span"))
        .cloned()
        .collect()
}

fn default_test_rs(f: &Field, value: &str) -> Option<String> {
    if f.seq {
        return Some(format!("{value}.is_empty()"));
    }
    if f.opt {
        return Some(format!("{value}.is_none()"));
    }
    if f.ty == "bool" {
        return Some(format!("!{value}"));
    }
    None
}

fn emit_field_sexpr(w: &mut Vec<String>, f: &Field, value: &str, indent: &str) {
    let fname = field_name(f);
    let guard = if f.seq {
        Some(format!("!{value}.is_empty()"))
    } else if f.opt {
        Some(format!("{value}.is_some()"))
    } else if f.ty == "bool" {
        Some(value.to_string())
    } else {
        None
    };
    let Some(guard) = guard else {
        w.push(format!("{indent}out.push_str(\" ({fname} \");"));
        w.push(format!("{indent}{value}.write_sexpr(out);"));
        w.push(format!("{indent}out.push(')');"));
        return;
    };
    w.push(format!("{indent}if {guard} {{"));
    w.push(format!("{indent}    out.push_str(\" ({fname} \");"));
    w.push(format!("{indent}    {value}.write_sexpr(out);"));
    w.push(format!("{indent}    out.push(')');"));
    w.push(format!("{indent}}}"));
}

fn emit_simple_sum(w: &mut Vec<String>, info: &TypeInfo) {
    w.push("#[derive(Clone, Copy, Debug, PartialEq, Eq)]".to_string());
    w.push(format!("pub enum {} {{", info.rust));
    for c in &info.constructors {
        w.push(format!("    {},", camel(&c.name)));
    }
    w.push("}".to_string());
}

fn emit_shared_product(
    w: &mut Vec<String>,
    info: &TypeInfo,
    infos: &HashMap<String, TypeInfo>,
) -> Result<(), CodegenError> {
    let attrs = &info.attributes;
    let data = data_attrs(info);
    let mut derives = vec!["Copy", "Clone", "Debug"];
    if data == *attrs {
        derives.extend(["PartialEq", "Eq"]);
    } else {
        derives.push("Eq");
    }
    w.push(format!("#[derive({})]", derives.join(", ")));
    w.push(format!("pub struct {} {{", info.rust));
    for f in attrs.iter().chain(info.fields.iter()) {
        let rname = rust_field(field_name(f));
        let rty = rust_type(f, infos, "", Backend::Arena)?;
        w.push(format!("    pub {rname}: {rty},"));
    }
    w.push("}".to_string());
    if data != *attrs {
        let own: Vec<&Field> = data.iter().chain(info.fields.iter()).collect();
        w.push(String::new());
        w.push(format!("impl PartialEq for {} {{", info.rust));
        w.push("    fn eq(&self, other: &Self) -> bool {".to_string());
        let cond = own
            .iter()
            .map(|f| {
                let rname = rust_field(field_name(f));
                format!("self.{rname} == other.{rname}")
            })
            .collect::<Vec<_>>()
            .join(" && ");
        w.push(format!("        {cond}"));
        w.push("    }".to_string());
        w.push("}".to_string());
    }
    Ok(())
}

fn emit_arena_ref_alias(w: &mut Vec<String>, info: &TypeInfo) {
    w.push(format!(
        "/// A reference to a `{}` node in the parse arena.",
        info.name
    ));
    w.push(format!(
        "pub type {}Ref<'a> = &'a {}{};",
        info.rust,
        info.rust,
        info.tp()
    ));
    w.push(format!(
        "/// A mutable construction handle to a `{}` node: a",
        info.name
    ));
    w.push("/// node is built and adjusted through this, then frozen into a".to_string());
    w.push("/// `<T>Ref` (an automatic `&mut` -> `&` coercion) when it is linked".to_string());
    w.push("/// into the tree. Only construction code sees these; readers get".to_string());
    w.push("/// `<T>Ref`.".to_string());
    w.push(format!(
        "pub type {}Mut<'a> = &'a mut {}{};",
        info.rust,
        info.rust,
        info.tp()
    ));
    w.push(String::new());
}

fn emit_owned_ref_alias(w: &mut Vec<String>, info: &TypeInfo) {
    w.push(format!("/// A reference to an owned `{}` node.", info.name));
    w.push(format!(
        "pub type {}Ref<'a> = &'a {};",
        info.rust, info.rust
    ));
    w.push(String::new());
}

fn emit_product(
    w: &mut Vec<String>,
    info: &TypeInfo,
    infos: &HashMap<String, TypeInfo>,
    backend: Backend,
) -> Result<(), CodegenError> {
    let attrs = &info.attributes;
    let data = data_attrs(info);
    let tp = if backend == Backend::Arena {
        info.tp()
    } else {
        ""
    };
    let mut derives = Vec::new();
    if backend == Backend::Arena {
        let cloneable = info.plain && !attrs.iter().chain(info.fields.iter()).any(|f| f.seq);
        if info.copyable {
            derives.push("Copy");
        }
        if cloneable {
            derives.push("Clone");
        }
    } else {
        derives.push("Clone");
    }
    derives.push("Debug");
    if data == *attrs {
        derives.extend(["PartialEq", "Eq"]);
    } else {
        derives.push("Eq");
    }
    w.push(format!("#[derive({})]", derives.join(", ")));
    w.push(format!("pub struct {}{tp} {{", info.rust));
    for f in attrs.iter().chain(info.fields.iter()) {
        let rname = rust_field(field_name(f));
        let rty = rust_type(f, infos, "", backend)?;
        w.push(format!("    pub {rname}: {rty},"));
    }
    w.push("}".to_string());
    if data != *attrs {
        let own: Vec<&Field> = data.iter().chain(info.fields.iter()).collect();
        w.push(String::new());
        w.push(format!("impl{tp} PartialEq for {}{tp} {{", info.rust));
        w.push("    fn eq(&self, other: &Self) -> bool {".to_string());
        let cond = own
            .iter()
            .map(|f| {
                let rname = rust_field(field_name(f));
                format!("self.{rname} == other.{rname}")
            })
            .collect::<Vec<_>>()
            .join(" && ");
        w.push(format!("        {cond}"));
        w.push("    }".to_string());
        w.push("}".to_string());
    }
    Ok(())
}

fn payload_tp(
    attrs: &[Field],
    ctor: &Constructor,
    infos: &HashMap<String, TypeInfo>,
) -> &'static str {
    let arena = attrs
        .iter()
        .chain(ctor.fields.iter())
        .any(|f| f.seq || f.ty == "string" || infos.get(&f.ty).is_some_and(|info| info.compound()));
    if arena { "<'a>" } else { "" }
}

fn is_copy_field(f: &Field, infos: &HashMap<String, TypeInfo>) -> bool {
    if f.seq {
        return false;
    }
    if builtin_rust(&f.ty).is_some() {
        return true;
    }
    let Some(info) = infos.get(&f.ty) else {
        return false;
    };
    info.kind == InfoKind::SimpleSum || info.copyable || info.compound()
}

fn emit_compound_sum(
    w: &mut Vec<String>,
    info: &TypeInfo,
    infos: &HashMap<String, TypeInfo>,
    backend: Backend,
) -> Result<(), CodegenError> {
    let attrs = &info.attributes;
    let data = data_attrs(info);
    let mod_name = uncamel(&info.rust);
    let tp = if backend == Backend::Arena {
        info.tp()
    } else {
        ""
    };
    let ctors: Vec<(&Constructor, String, Vec<Field>)> = info
        .constructors
        .iter()
        .map(|c| {
            let mut fields = attrs.clone();
            fields.extend(c.fields.iter().cloned());
            (c, camel(&c.name), fields)
        })
        .collect();
    w.push(format!(
        "/// The `{}` variant payloads: one struct per constructor,",
        info.name
    ));
    w.push("/// each held indirectly so the enum stays one word.".to_string());
    w.push(format!("pub mod {mod_name} {{"));
    if backend == Backend::Arena {
        w.push("    #[allow(unused_imports)]".to_string());
        w.push("    use bumpalo::collections::Vec;".to_string());
    }
    w.push("    #[allow(unused_imports)]".to_string());
    w.push("    use super::*;".to_string());
    for (c, name, fields) in &ctors {
        if fields.is_empty() {
            continue;
        }
        let (ctp, mut derives) = if backend == Backend::Arena {
            let ctp = payload_tp(attrs, c, infos);
            let mut derives = Vec::new();
            if fields.iter().all(|f| is_copy_field(f, infos)) {
                derives.extend(["Clone", "Copy"]);
            }
            (ctp, derives)
        } else {
            ("", vec!["Clone"])
        };
        derives.push("Debug");
        if data == *attrs {
            derives.extend(["PartialEq", "Eq"]);
        } else {
            derives.push("Eq");
        }
        w.push(String::new());
        w.push(format!("    /// `{}::{name}`.", info.rust));
        w.push(format!("    #[derive({})]", derives.join(", ")));
        w.push(format!("    pub struct {name}{ctp} {{"));
        for f in fields {
            let rname = rust_field(field_name(f));
            let rty = rust_type(f, infos, "super::", backend)?;
            w.push(format!("        pub {rname}: {rty},"));
        }
        w.push("    }".to_string());
        if data != *attrs {
            let own: Vec<&Field> = data.iter().chain(c.fields.iter()).collect();
            w.push(String::new());
            w.push(format!("    impl{ctp} PartialEq for {name}{ctp} {{"));
            if !own.is_empty() {
                let cond = own
                    .iter()
                    .map(|f| {
                        let rname = rust_field(field_name(f));
                        format!("self.{rname} == other.{rname}")
                    })
                    .collect::<Vec<_>>()
                    .join(" && ");
                w.push("        fn eq(&self, other: &Self) -> bool {".to_string());
                w.push(format!("            {cond}"));
            } else {
                w.push("        fn eq(&self, _other: &Self) -> bool {".to_string());
                w.push("            true".to_string());
            }
            w.push("        }".to_string());
            w.push("    }".to_string());
        }
        if backend == Backend::Arena {
            w.push(String::new());
            w.push(format!(
                "    /// A mutable construction handle to a `{}` payload.",
                c.name
            ));
            w.push(format!("    pub type {name}Mut<'a> = &'a mut {name}{ctp};"));
        }
    }
    w.push("}".to_string());
    w.push(String::new());

    let mut derives = vec!["Debug", "PartialEq", "Eq"];
    if backend == Backend::Owned {
        derives.insert(0, "Clone");
    }
    w.push(format!("#[derive({})]", derives.join(", ")));
    w.push(format!("pub enum {}{tp} {{", info.rust));
    for (_, name, fields) in &ctors {
        if fields.is_empty() {
            w.push(format!("    {name},"));
        } else if backend == Backend::Arena {
            w.push(format!("    {name}({mod_name}::{name}Mut<'a>),"));
        } else {
            w.push(format!("    {name}(Box<{mod_name}::{name}>),"));
        }
    }
    w.push("}".to_string());
    Ok(())
}

fn to_owned_expr(f: &Field, infos: &HashMap<String, TypeInfo>) -> String {
    let info = infos.get(&f.ty);
    let name = format!("self.{}", rust_field(field_name(f)));
    let per_backend = info.is_some_and(|i| i.kind != InfoKind::SimpleSum && !i.shared);
    if f.seq {
        if f.ty == "string" {
            return format!("{name}.iter().map(|x| (*x).into()).collect()");
        }
        if per_backend {
            return format!("{name}.iter().map(|x| (*x).to_owned()).collect()");
        }
        return format!("{name}.iter().copied().collect()");
    }
    if f.opt {
        if f.ty == "string" {
            return format!("{name}.map(|s| s.into())");
        }
        if per_backend {
            if info.unwrap().compound() {
                return format!("{name}.map(|x| Box::new(x.to_owned()))");
            }
            return format!("{name}.as_ref().map(|x| x.to_owned())");
        }
        return name;
    }
    if f.ty == "string" {
        return format!("{name}.into()");
    }
    if per_backend {
        if info.unwrap().compound() {
            return format!("Box::new({name}.to_owned())");
        }
        return format!("{name}.to_owned()");
    }
    name
}

fn emit_to_owned(w: &mut Vec<String>, info: &TypeInfo, infos: &HashMap<String, TypeInfo>) {
    let tp = info.tp();
    let target = format!("crate::owned::{}", info.rust);
    if info.kind == InfoKind::Product {
        w.push(String::new());
        w.push(format!("impl{tp} {}{tp} {{", info.rust));
        w.push("    /// Deep-convert this node into the self-owned tree.".to_string());
        w.push(format!("    pub fn to_owned(&self) -> {target} {{"));
        w.push(format!("        {target} {{"));
        for f in info.attributes.iter().chain(info.fields.iter()) {
            let rname = rust_field(field_name(f));
            let expr = to_owned_expr(f, infos);
            w.push(format!("            {rname}: {expr},"));
        }
        w.push("        }".to_string());
        w.push("    }".to_string());
        w.push("}".to_string());
        return;
    }
    let mod_name = uncamel(&info.rust);
    for c in &info.constructors {
        let name = camel(&c.name);
        let fields: Vec<&Field> = info.attributes.iter().chain(c.fields.iter()).collect();
        if fields.is_empty() {
            continue;
        }
        let ctp = payload_tp(&info.attributes, c, infos);
        w.push(String::new());
        w.push(format!("impl{ctp} {mod_name}::{name}{ctp} {{"));
        w.push("    /// Deep-convert this payload into the self-owned tree.".to_string());
        w.push(format!(
            "    pub fn to_owned(&self) -> crate::owned::{mod_name}::{name} {{"
        ));
        w.push(format!("        crate::owned::{mod_name}::{name} {{"));
        for f in fields {
            let rname = rust_field(field_name(f));
            let expr = to_owned_expr(f, infos);
            w.push(format!("            {rname}: {expr},"));
        }
        w.push("        }".to_string());
        w.push("    }".to_string());
        w.push("}".to_string());
    }
    w.push(String::new());
    w.push(format!("impl{tp} {}{tp} {{", info.rust));
    w.push("    /// Deep-convert this node into the self-owned tree.".to_string());
    w.push(format!("    pub fn to_owned(&self) -> {target} {{"));
    w.push("        match self {".to_string());
    for c in &info.constructors {
        let name = camel(&c.name);
        if info.attributes.is_empty() && c.fields.is_empty() {
            w.push(format!("            Self::{name} => {target}::{name},"));
        } else {
            w.push(format!("            Self::{name}(p) =>"));
            w.push(format!(
                "                {target}::{name}(Box::new((**p).to_owned())),"
            ));
        }
    }
    w.push("        }".to_string());
    w.push("    }".to_string());
    w.push("}".to_string());
}

fn emit_attr_setters(
    w: &mut Vec<String>,
    info: &TypeInfo,
    infos: &HashMap<String, TypeInfo>,
    backend: Backend,
) -> Result<(), CodegenError> {
    let tp = if backend == Backend::Arena {
        info.tp()
    } else {
        ""
    };
    let mod_name = uncamel(&info.rust);
    for a in &info.attributes {
        let aname = field_name(a);
        let ty = rust_type(a, infos, "", backend)?;
        let field = rust_field(aname);
        w.push(format!("impl{tp} {}{tp} {{", info.rust));
        w.push(format!("    /// Set the `{aname}` attribute."));
        w.push(format!(
            "    pub fn with_{aname}(mut self, v: {ty}) -> Self {{"
        ));
        if info.kind == InfoKind::Product {
            w.push(format!("        self.{field} = v;"));
            w.push("        self".to_string());
        } else {
            w.push(format!("        self.set_{aname}(v);"));
            w.push("        self".to_string());
        }
        w.push("    }".to_string());
        w.push(format!(
            "    /// Set the `{aname}` attribute in place, with no rebuild."
        ));
        w.push(format!("    pub fn set_{aname}(&mut self, v: {ty}) {{"));
        if info.kind == InfoKind::Product {
            w.push(format!("        self.{field} = v;"));
        } else {
            w.push("        match self {".to_string());
            for c in &info.constructors {
                w.push(format!(
                    "            Self::{}(p) => p.set_{aname}(v),",
                    camel(&c.name)
                ));
            }
            w.push("        }".to_string());
        }
        w.push("    }".to_string());
        w.push("}".to_string());
        if info.kind != InfoKind::Product {
            for c in &info.constructors {
                let name = camel(&c.name);
                let ctp = if backend == Backend::Arena {
                    payload_tp(&info.attributes, c, infos)
                } else {
                    ""
                };
                w.push(format!("impl{ctp} {mod_name}::{name}{ctp} {{"));
                w.push(format!("    /// Set the `{aname}` attribute."));
                w.push(format!(
                    "    pub fn with_{aname}(mut self, v: {ty}) -> Self {{"
                ));
                w.push(format!("        self.{field} = v;"));
                w.push("        self".to_string());
                w.push("    }".to_string());
                w.push(format!("    /// Set the `{aname}` attribute in place."));
                w.push(format!("    pub fn set_{aname}(&mut self, v: {ty}) {{"));
                w.push(format!("        self.{field} = v;"));
                w.push("    }".to_string());
                w.push("}".to_string());
            }
        }
        w.push(String::new());
    }
    Ok(())
}

fn accessor_bodies(
    f: &Field,
    infos: &HashMap<String, TypeInfo>,
    place: &str,
) -> (String, String, String) {
    let b = format!("{place}.{}", rust_field(field_name(f)));
    let info = infos.get(&f.ty);
    let r = info.map(|i| i.rust.as_str()).unwrap_or("");
    let shared = info.is_none_or(|i| i.kind == InfoKind::SimpleSum || i.shared);

    if f.seq {
        if f.ty == "identifier" {
            return (
                "dbt_asdl::IdentList<'a>".to_string(),
                format!("&{b}"),
                format!("&{b}"),
            );
        }
        if f.ty == "string" {
            return (
                "dbt_asdl::StringList<'a>".to_string(),
                format!("dbt_asdl::StringList::Owned(&{b})"),
                format!("dbt_asdl::StringList::Arena(&{b})"),
            );
        }
        if is_scalar_builtin(&f.ty) {
            let bty = builtin_rust(&f.ty).unwrap();
            return (format!("&'a [{bty}]"), format!("&{b}"), format!("&{b}"));
        }
        if shared {
            return (format!("&'a [{r}]"), format!("&{b}"), format!("&{b}"));
        }
        return (
            format!("{r}List<'a>"),
            format!("{r}List::Owned(&{b})"),
            format!("{r}List::Arena(&{b})"),
        );
    }
    if f.opt {
        if f.ty == "identifier" {
            return ("Option<dbt_asdl::Identifier>".to_string(), b.clone(), b);
        }
        if f.ty == "string" {
            return ("Option<&'a str>".to_string(), format!("{b}.as_deref()"), b);
        }
        if is_scalar_builtin(&f.ty) || shared {
            let inner = builtin_rust(&f.ty).unwrap_or(r);
            return (format!("Option<{inner}>"), b.clone(), b);
        }
        if info.unwrap().compound() {
            return (
                format!("Option<{r}Ref<'a>>"),
                format!("{b}.as_deref().map({r}Ref::Owned)"),
                format!("{b}.map({r}Ref::Arena)"),
            );
        }
        return (
            format!("Option<{r}Ref<'a>>"),
            format!("{b}.as_ref().map({r}Ref::Owned)"),
            format!("{b}.as_ref().map({r}Ref::Arena)"),
        );
    }
    if f.ty == "identifier" {
        return ("dbt_asdl::Identifier".to_string(), b.clone(), b);
    }
    if f.ty == "string" {
        return ("&'a str".to_string(), format!("&{b}"), b);
    }
    if is_scalar_builtin(&f.ty) || shared {
        let ret = builtin_rust(&f.ty).unwrap_or(r).to_string();
        return (ret, b.clone(), b);
    }
    if info.unwrap().compound() {
        return (
            format!("{r}Ref<'a>"),
            format!("{r}Ref::Owned(&{b})"),
            format!("{r}Ref::Arena({b})"),
        );
    }
    (
        format!("{r}Ref<'a>"),
        format!("{r}Ref::Owned(&{b})"),
        format!("{r}Ref::Arena(&{b})"),
    )
}

fn emit_either_ref(wv: &mut Vec<String>, info: &TypeInfo) {
    let r = &info.rust;
    let tp = info.tp();
    wv.push(format!(
        "/// A read-only reference to a `{}` node, in either tree",
        info.name
    ));
    wv.push("/// representation: what every accessor takes and returns.".to_string());
    wv.push("#[derive(Clone, Copy, Debug)]".to_string());
    wv.push(format!("pub enum {r}Ref<'a> {{"));
    wv.push(format!("    Owned(&'a owned::{r}),"));
    wv.push(format!("    Arena(&'a arena::{r}{tp}),"));
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!("impl<'a> From<&'a owned::{r}> for {r}Ref<'a> {{"));
    wv.push(format!("    fn from(node: &'a owned::{r}) -> Self {{"));
    wv.push(format!("        {r}Ref::Owned(node)"));
    wv.push("    }".to_string());
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!(
        "impl<'a> From<&'a arena::{r}{tp}> for {r}Ref<'a> {{"
    ));
    wv.push(format!("    fn from(node: &'a arena::{r}{tp}) -> Self {{"));
    wv.push(format!("        {r}Ref::Arena(node)"));
    wv.push("    }".to_string());
    wv.push("}".to_string());
    wv.push(String::new());
}

fn emit_product_view(wv: &mut Vec<String>, info: &TypeInfo, infos: &HashMap<String, TypeInfo>) {
    let r = &info.rust;
    let fields: Vec<&Field> = info.attributes.iter().chain(info.fields.iter()).collect();
    if fields.is_empty() {
        return;
    }
    wv.push(format!("impl<'a> {r}Ref<'a> {{"));
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            wv.push(String::new());
        }
        let fname = field_name(f);
        let (ret, ob, ab) = accessor_bodies(f, infos, "n");
        wv.push(format!("    /// The `{fname}` field."));
        wv.push(format!(
            "    pub fn {}(self) -> {ret} {{",
            rust_field(fname)
        ));
        wv.push("        match self {".to_string());
        wv.push(format!("            {r}Ref::Owned(n) => {ob},"));
        wv.push(format!("            {r}Ref::Arena(n) => {ab},"));
        wv.push("        }".to_string());
        wv.push("    }".to_string());
    }
    wv.push("}".to_string());
}

fn emit_product_getters(wv: &mut Vec<String>, info: &TypeInfo, infos: &HashMap<String, TypeInfo>) {
    let fields: Vec<&Field> = info.attributes.iter().chain(info.fields.iter()).collect();
    if fields.is_empty() {
        return;
    }
    let tp = info.tp();
    wv.push(format!("impl{tp} {}{tp} {{", info.rust));
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            wv.push(String::new());
        }
        let fname = field_name(f);
        let rfield = rust_field(fname);
        let (ret, _, _) = accessor_bodies(f, infos, "self");
        let expr = format!("self.{rfield}");
        wv.push(format!("    /// The `{fname}` field."));
        wv.push(format!("    pub fn {rfield}(&self) -> {ret} {{"));
        wv.push(format!("        {expr}"));
        wv.push("    }".to_string());
    }
    wv.push("}".to_string());
}

fn emit_variant_view(
    wv: &mut Vec<String>,
    sum_info: &TypeInfo,
    ctor: &Constructor,
    infos: &HashMap<String, TypeInfo>,
) -> Option<String> {
    let r = &sum_info.rust;
    let name = camel(&ctor.name);
    let vname = format!("{r}{name}View");
    let fields: Vec<&Field> = sum_info
        .attributes
        .iter()
        .chain(ctor.fields.iter())
        .collect();
    if fields.is_empty() {
        return None;
    }
    wv.push(format!("/// `{r}::{name} {{ .. }}`."));
    wv.push("#[derive(Clone, Copy, Debug)]".to_string());
    wv.push(format!("pub struct {vname}<'a> {{"));
    wv.push(format!("    node: {r}Ref<'a>,"));
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!("impl<'a> {vname}<'a> {{"));
    wv.push("    /// The whole node, for `kind()` / `view()` / attribute getters.".to_string());
    wv.push(format!("    pub fn node(self) -> {r}Ref<'a> {{"));
    wv.push("        self.node".to_string());
    wv.push("    }".to_string());
    for f in fields {
        wv.push(String::new());
        let fname = field_name(f);
        let (ret, ob, ab) = accessor_bodies(f, infos, "p");
        wv.push(format!("    /// The `{fname}` field."));
        wv.push(format!(
            "    pub fn {}(self) -> {ret} {{",
            rust_field(fname)
        ));
        wv.push("        match self.node {".to_string());
        wv.push(format!(
            "            {r}Ref::Owned(owned::{r}::{name}(p)) => {ob},"
        ));
        wv.push(format!(
            "            {r}Ref::Arena(arena::{r}::{name}(p)) => {ab},"
        ));
        wv.push(
            "            _ => unreachable!(\"view checked the variant at construction\"),"
                .to_string(),
        );
        wv.push("        }".to_string());
        wv.push("    }".to_string());
    }
    wv.push("}".to_string());
    Some(vname)
}

fn emit_sum_views(wv: &mut Vec<String>, info: &TypeInfo, infos: &HashMap<String, TypeInfo>) {
    let r = &info.rust;
    let ctors: Vec<(&Constructor, String)> = info
        .constructors
        .iter()
        .map(|c| (c, camel(&c.name)))
        .collect();
    let bare: HashSet<String> = ctors
        .iter()
        .filter(|(c, _)| info.attributes.is_empty() && c.fields.is_empty())
        .map(|(_, name)| name.clone())
        .collect();

    wv.push("#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]".to_string());
    wv.push(format!("pub enum {r}Kind {{"));
    for (_, name) in &ctors {
        wv.push(format!("    {name},"));
    }
    wv.push("}".to_string());
    wv.push(String::new());

    emit_either_ref(wv, info);

    wv.push(format!("impl<'a> {r}Ref<'a> {{"));
    wv.push("    /// The node's constructor, without borrowing its payload.".to_string());
    wv.push(format!("    pub fn kind(self) -> {r}Kind {{"));
    wv.push("        match self {".to_string());
    wv.push(format!("            {r}Ref::Owned(n) => match n {{"));
    for (_, name) in &ctors {
        let pat = if bare.contains(name) {
            name.clone()
        } else {
            format!("{name}(_)")
        };
        wv.push(format!(
            "                owned::{r}::{pat} => {r}Kind::{name},"
        ));
    }
    wv.push("            },".to_string());
    wv.push(format!("            {r}Ref::Arena(n) => match n {{"));
    for (_, name) in &ctors {
        let pat = if bare.contains(name) {
            name.clone()
        } else {
            format!("{name}(_)")
        };
        wv.push(format!(
            "                arena::{r}::{pat} => {r}Kind::{name},"
        ));
    }
    wv.push("            },".to_string());
    wv.push("        }".to_string());
    wv.push("    }".to_string());
    for a in &info.attributes {
        let aname = field_name(a);
        let (ret, ob, ab) = accessor_bodies(a, infos, "p");
        wv.push(String::new());
        wv.push(format!("    /// The `{aname}` attribute."));
        wv.push(format!(
            "    pub fn {}(self) -> {ret} {{",
            rust_field(aname)
        ));
        wv.push("        match self {".to_string());
        wv.push(format!("            {r}Ref::Owned(n) => match n {{"));
        for (_, name) in &ctors {
            wv.push(format!("                owned::{r}::{name}(p) => {ob},"));
        }
        wv.push("            },".to_string());
        wv.push(format!("            {r}Ref::Arena(n) => match n {{"));
        for (_, name) in &ctors {
            wv.push(format!("                arena::{r}::{name}(p) => {ab},"));
        }
        wv.push("            },".to_string());
        wv.push("        }".to_string());
        wv.push("    }".to_string());
    }
    wv.push(String::new());
    wv.push("    /// The borrowed view of this node, for matching.".to_string());
    wv.push(format!("    pub fn view(self) -> {r}View<'a> {{"));
    wv.push("        match self {".to_string());
    wv.push(format!("            {r}Ref::Owned(n) => match n {{"));
    for (_, name) in &ctors {
        if bare.contains(name) {
            wv.push(format!(
                "                owned::{r}::{name} => {r}View::{name},"
            ));
        } else {
            wv.push(format!("                owned::{r}::{name}(_) =>"));
            wv.push(format!(
                "                    {r}View::{name}({r}{name}View {{ node: self }}),"
            ));
        }
    }
    wv.push("            },".to_string());
    wv.push(format!("            {r}Ref::Arena(n) => match n {{"));
    for (_, name) in &ctors {
        if bare.contains(name) {
            wv.push(format!(
                "                arena::{r}::{name} => {r}View::{name},"
            ));
        } else {
            wv.push(format!("                arena::{r}::{name}(_) =>"));
            wv.push(format!(
                "                    {r}View::{name}({r}{name}View {{ node: self }}),"
            ));
        }
    }
    wv.push("            },".to_string());
    wv.push("        }".to_string());
    wv.push("    }".to_string());
    for (_, name) in &ctors {
        if bare.contains(name) {
            continue;
        }
        wv.push(String::new());
        wv.push(format!(
            "    /// The node as `{name}`, or None. Composes with `?`."
        ));
        wv.push(format!(
            "    pub fn as_{}(self) -> Option<{r}{name}View<'a>> {{",
            uncamel(name)
        ));
        wv.push("        match self.view() {".to_string());
        wv.push(format!("            {r}View::{name}(v) => Some(v),"));
        wv.push("            _ => None,".to_string());
        wv.push("        }".to_string());
        wv.push("    }".to_string());
    }
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!(
        "/// A borrowed view of a `{}` node: one arm per constructor.",
        info.name
    ));
    wv.push("#[derive(Clone, Copy, Debug)]".to_string());
    wv.push(format!("pub enum {r}View<'a> {{"));
    for (_, name) in &ctors {
        if bare.contains(name) {
            wv.push(format!("    {name},"));
        } else {
            wv.push(format!("    {name}({r}{name}View<'a>),"));
        }
    }
    wv.push("}".to_string());
    for (c, name) in &ctors {
        if bare.contains(name) {
            continue;
        }
        wv.push(String::new());
        emit_variant_view(wv, info, c, infos);
    }
}

fn emit_list_iter(wv: &mut Vec<String>, name: &str, item: &str) {
    wv.push(format!("/// An iterator over a `{name}`."));
    wv.push(format!("pub struct {name}Iter<'a> {{"));
    wv.push(format!("    list: {name}<'a>,"));
    wv.push("    next: usize,".to_string());
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!("impl<'a> Iterator for {name}Iter<'a> {{"));
    wv.push(format!("    type Item = {item};"));
    wv.push(String::new());
    wv.push("    fn next(&mut self) -> Option<Self::Item> {".to_string());
    wv.push("        let item = self.list.get(self.next)?;".to_string());
    wv.push("        self.next += 1;".to_string());
    wv.push("        Some(item)".to_string());
    wv.push("    }".to_string());
    wv.push(String::new());
    wv.push("    fn size_hint(&self) -> (usize, Option<usize>) {".to_string());
    wv.push("        let remaining = self.list.len() - self.next;".to_string());
    wv.push("        (remaining, Some(remaining))".to_string());
    wv.push("    }".to_string());
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!("impl ExactSizeIterator for {name}Iter<'_> {{}}"));
    wv.push(String::new());
    wv.push(format!("impl<'a> IntoIterator for {name}<'a> {{"));
    wv.push(format!("    type Item = {item};"));
    wv.push(format!("    type IntoIter = {name}Iter<'a>;"));
    wv.push(String::new());
    wv.push("    fn into_iter(self) -> Self::IntoIter {".to_string());
    wv.push("        self.iter()".to_string());
    wv.push("    }".to_string());
    wv.push("}".to_string());
}

fn emit_list_view(wv: &mut Vec<String>, info: &TypeInfo) {
    let r = &info.rust;
    let arena_item = if info.compound() {
        format!("arena::{r}Ref<'a>")
    } else {
        format!("arena::{r}{}", info.tp())
    };
    wv.push(format!(
        "/// A borrowed list of `{}` nodes, in either tree",
        info.name
    ));
    wv.push("/// representation.".to_string());
    wv.push("#[derive(Clone, Copy, Debug)]".to_string());
    wv.push(format!("pub enum {r}List<'a> {{"));
    wv.push(format!("    Owned(&'a [owned::{r}]),"));
    wv.push(format!("    Arena(&'a [{arena_item}]),"));
    wv.push("}".to_string());
    wv.push(String::new());
    wv.push(format!("impl<'a> {r}List<'a> {{"));
    wv.push("    /// The number of nodes in the list.".to_string());
    wv.push("    pub fn len(self) -> usize {".to_string());
    wv.push("        match self {".to_string());
    wv.push(format!("            {r}List::Owned(s) => s.len(),"));
    wv.push(format!("            {r}List::Arena(s) => s.len(),"));
    wv.push("        }".to_string());
    wv.push("    }".to_string());
    wv.push(String::new());
    wv.push("    /// Whether the list holds no nodes.".to_string());
    wv.push("    pub fn is_empty(self) -> bool {".to_string());
    wv.push("        self.len() == 0".to_string());
    wv.push("    }".to_string());
    wv.push(String::new());
    wv.push("    /// The node at `i`, or None when out of bounds.".to_string());
    wv.push(format!(
        "    pub fn get(self, i: usize) -> Option<{r}Ref<'a>> {{"
    ));
    wv.push("        match self {".to_string());
    wv.push(format!(
        "            {r}List::Owned(s) => s.get(i).map({r}Ref::Owned),"
    ));
    if info.compound() {
        wv.push(format!(
            "            {r}List::Arena(s) => s.get(i).copied().map({r}Ref::Arena),"
        ));
    } else {
        wv.push(format!(
            "            {r}List::Arena(s) => s.get(i).map({r}Ref::Arena),"
        ));
    }
    wv.push("        }".to_string());
    wv.push("    }".to_string());
    wv.push(String::new());
    wv.push("    /// Iterate over the nodes, as `XRef` handles.".to_string());
    wv.push(format!("    pub fn iter(self) -> {r}ListIter<'a> {{"));
    wv.push(format!("        {r}ListIter {{ list: self, next: 0 }}"));
    wv.push("    }".to_string());
    wv.push("}".to_string());
    wv.push(String::new());
    emit_list_iter(wv, &format!("{r}List"), &format!("{r}Ref<'a>"));
}

fn emit_simple_sum_sexpr(ws: &mut Vec<String>, info: &TypeInfo) {
    ws.push(format!("impl Sexpr for {} {{", info.rust));
    ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
    ws.push("        out.push_str(match self {".to_string());
    for c in &info.constructors {
        let cname = camel(&c.name);
        ws.push(format!("            Self::{cname} => \"{cname}\","));
    }
    ws.push("        });".to_string());
    ws.push("    }".to_string());
    ws.push("}".to_string());
}

fn emit_product_fields_sexpr(ws: &mut Vec<String>, fields: &[Field], value: &str, indent: &str) {
    for f in fields {
        let rname = rust_field(field_name(f));
        emit_field_sexpr(ws, f, &format!("{value}.{rname}()"), indent);
    }
}

fn emit_shared_product_sexpr(ws: &mut Vec<String>, info: &TypeInfo) {
    ws.push(format!("impl Sexpr for {} {{", info.rust));
    ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
    ws.push(format!("        out.push_str(\"({}\");", info.rust));
    for f in data_attrs(info).iter().chain(info.fields.iter()) {
        let rname = rust_field(field_name(f));
        emit_field_sexpr(ws, f, &format!("self.{rname}"), "        ");
    }
    ws.push("        out.push(')');".to_string());
    ws.push("    }".to_string());
    ws.push("}".to_string());
}

fn emit_product_ref_sexpr(ws: &mut Vec<String>, info: &TypeInfo) {
    ws.push(format!("impl Sexpr for {}Ref<'_> {{", info.rust));
    ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
    ws.push(format!("        out.push_str(\"({}\");", info.rust));
    let mut fields = data_attrs(info);
    fields.extend(info.fields.iter().cloned());
    emit_product_fields_sexpr(ws, &fields, "self", "        ");
    ws.push("        out.push(')');".to_string());
    ws.push("    }".to_string());
    ws.push("}".to_string());
}

fn emit_sum_ref_sexpr(ws: &mut Vec<String>, info: &TypeInfo) {
    let r = &info.rust;
    let data = data_attrs(info);
    ws.push(format!("impl Sexpr for {r}Ref<'_> {{"));
    ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
    ws.push("        match self.view() {".to_string());
    for c in &info.constructors {
        let name = camel(&c.name);
        if info.attributes.is_empty() && c.fields.is_empty() {
            ws.push(format!(
                "            {r}View::{name} => out.push_str(\"{name}\"),"
            ));
            continue;
        }
        let mut own = data.clone();
        own.extend(c.fields.iter().cloned());
        if own.is_empty() {
            ws.push(format!(
                "            {r}View::{name}(_) => out.push_str(\"{name}\"),"
            ));
            continue;
        }
        ws.push(format!("            {r}View::{name}(v) => {{"));
        let tests: Vec<Option<String>> = own
            .iter()
            .map(|f| default_test_rs(f, &format!("v.{}()", rust_field(field_name(f)))))
            .collect();
        let atom_cond = if tests.iter().all(Option::is_some) {
            Some(tests.into_iter().flatten().collect::<Vec<_>>().join(" && "))
        } else {
            None
        };
        let indent = if let Some(cond) = &atom_cond {
            ws.push(format!("                if {cond} {{"));
            ws.push(format!("                    out.push_str(\"{name}\");"));
            ws.push("                } else {".to_string());
            "        "
        } else {
            ""
        };
        ws.push(format!("{indent}            out.push_str(\"({name}\");"));
        let field_indent = format!("            {indent}");
        for f in &own {
            let rname = rust_field(field_name(f));
            emit_field_sexpr(ws, f, &format!("v.{rname}()"), &field_indent);
        }
        ws.push(format!("{indent}            out.push(')');"));
        if atom_cond.is_some() {
            ws.push("                }".to_string());
        }
        ws.push("            }".to_string());
    }
    ws.push("        }".to_string());
    ws.push("    }".to_string());
    ws.push("}".to_string());
}

fn emit_list_sexpr(ws: &mut Vec<String>, name: &str) {
    ws.push(format!("impl Sexpr for {name}<'_> {{"));
    ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
    ws.push("        out.push('[');".to_string());
    ws.push("        for (i, item) in self.iter().enumerate() {".to_string());
    ws.push("            if i > 0 {".to_string());
    ws.push("                out.push(' ');".to_string());
    ws.push("            }".to_string());
    ws.push("            item.write_sexpr(out);".to_string());
    ws.push("        }".to_string());
    ws.push("        out.push(']');".to_string());
    ws.push("    }".to_string());
    ws.push("}".to_string());
}

fn emit_delegating_sexpr(ws: &mut Vec<String>, info: &TypeInfo) {
    let r = &info.rust;
    ws.push(format!("impl Sexpr for owned::{r} {{"));
    ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
    ws.push(format!("        {r}Ref::Owned(self).write_sexpr(out);"));
    ws.push("    }".to_string());
    ws.push("}".to_string());
    if info.arena {
        ws.push(String::new());
        ws.push(format!("impl<'a> Sexpr for &'a arena::{r}<'a> {{"));
        ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
        ws.push(format!("        {r}Ref::Arena(self).write_sexpr(out);"));
        ws.push("    }".to_string());
        ws.push("}".to_string());
    } else {
        ws.push(String::new());
        ws.push(format!("impl Sexpr for arena::{r} {{"));
        ws.push("    fn write_sexpr(&self, out: &mut String) {".to_string());
        ws.push(format!("        {r}Ref::Arena(self).write_sexpr(out);"));
        ws.push("    }".to_string());
        ws.push("}".to_string());
    }
}

pub(crate) fn generate(mod_node: &Module, input_name: &str) -> Result<GeneratedRust, CodegenError> {
    let mut builtins: Vec<&str> = asdl::BUILTIN_TYPES.to_vec();
    for &(k, _) in BUILTIN_RUST {
        if !builtins.contains(&k) {
            builtins.push(k);
        }
    }
    if asdl::check_with_builtins(mod_node, &builtins).is_err() {
        return Err(CodegenError(format!("{input_name}: ASDL check failed")));
    }

    let order: Vec<String> = mod_node.dfns.iter().map(|d| d.name.clone()).collect();
    let mut infos: HashMap<String, TypeInfo> = mod_node
        .dfns
        .iter()
        .map(|dfn| (dfn.name.clone(), TypeInfo::from_dfn(dfn)))
        .collect();

    validate(mod_node)?;
    compute_value_semantics(&order, &mut infos);
    validate_view_names(mod_node, &order, &infos)?;

    let mut shared = Vec::new();
    let mut arena = Vec::new();
    let mut owned = Vec::new();
    let mut views = Vec::new();
    let mut sexpr = Vec::new();

    let header =
        format!("// File automatically generated by dbt-asdl from {input_name}. Do not edit.");

    let shared_names: Vec<&str> = mod_node
        .dfns
        .iter()
        .filter_map(|d| {
            let info = &infos[&d.name];
            (info.kind == InfoKind::SimpleSum || info.shared).then_some(info.rust.as_str())
        })
        .collect();
    let shared_joined = shared_names.join(", ");

    shared.push(header.clone());
    shared.push(String::new());
    arena.push(header.clone());
    arena.push(String::new());
    arena.push("use bumpalo::collections::Vec;".to_string());
    arena.push(String::new());
    arena.push(format!("pub use crate::shared::{{{shared_joined}}};"));
    arena.push(String::new());

    owned.push(header.clone());
    owned.push(String::new());
    owned.push(format!("pub use crate::shared::{{{shared_joined}}};"));
    owned.push(String::new());

    views.push(header.clone());
    views.push(String::new());
    views.push("use crate::shared::*;".to_string());
    views.push("use crate::{arena, owned};".to_string());
    views.push(String::new());

    sexpr.push(header);
    sexpr.push(String::new());
    sexpr.push("use dbt_asdl::Sexpr;".to_string());
    sexpr.push("use crate::shared::*;".to_string());
    sexpr.push("use crate::gen_views::*;".to_string());
    sexpr.push("use crate::{arena, owned};".to_string());
    sexpr.push(String::new());

    let seq_types = seq_list_types(mod_node, &infos);
    let mut list_infos = Vec::new();
    for dfn in &mod_node.dfns {
        let info = &infos[&dfn.name];
        if info.kind == InfoKind::Product && info.shared {
            emit_shared_product(&mut shared, info, &infos)?;
            if !info.attributes.is_empty() {
                emit_attr_setters(&mut shared, info, &infos, Backend::Arena)?;
            }
            emit_product_getters(&mut views, info, &infos);
            emit_shared_product_sexpr(&mut sexpr, info);
        } else if info.kind == InfoKind::Product {
            emit_arena_ref_alias(&mut arena, info);
            emit_product(&mut arena, info, &infos, Backend::Arena)?;
            emit_owned_ref_alias(&mut owned, info);
            emit_product(&mut owned, info, &infos, Backend::Owned)?;
            if !info.attributes.is_empty() {
                emit_attr_setters(&mut arena, info, &infos, Backend::Arena)?;
                emit_attr_setters(&mut owned, info, &infos, Backend::Owned)?;
            }
            emit_to_owned(&mut arena, info, &infos);
            emit_either_ref(&mut views, info);
            emit_product_view(&mut views, info, &infos);
            if seq_types.contains(&dfn.name) {
                emit_list_view(&mut views, info);
                list_infos.push(info);
            }
            emit_product_ref_sexpr(&mut sexpr, info);
            emit_delegating_sexpr(&mut sexpr, info);
        } else if info.kind == InfoKind::SimpleSum {
            emit_simple_sum(&mut shared, info);
            emit_simple_sum_sexpr(&mut sexpr, info);
        } else {
            emit_arena_ref_alias(&mut arena, info);
            emit_compound_sum(&mut arena, info, &infos, Backend::Arena)?;
            emit_owned_ref_alias(&mut owned, info);
            emit_compound_sum(&mut owned, info, &infos, Backend::Owned)?;
            if !info.attributes.is_empty() {
                emit_attr_setters(&mut arena, info, &infos, Backend::Arena)?;
                emit_attr_setters(&mut owned, info, &infos, Backend::Owned)?;
            }
            emit_to_owned(&mut arena, info, &infos);
            emit_sum_views(&mut views, info, &infos);
            if seq_types.contains(&dfn.name) {
                emit_list_view(&mut views, info);
                list_infos.push(info);
            }
            emit_sum_ref_sexpr(&mut sexpr, info);
            emit_delegating_sexpr(&mut sexpr, info);
        }
        shared.push(String::new());
        arena.push(String::new());
        owned.push(String::new());
        views.push(String::new());
        sexpr.push(String::new());
    }

    for info in list_infos {
        emit_list_sexpr(&mut sexpr, &format!("{}List", info.rust));
    }

    Ok(GeneratedRust {
        shared: format!("{}\n", shared.join("\n")),
        arena: format!("{}\n", arena.join("\n")),
        owned: format!("{}\n", owned.join("\n")),
        views: format!("{}\n", views.join("\n")),
        sexpr: format!("{}\n", sexpr.join("\n")),
    })
}

/// Generates the five Rust AST source files from an ASDL specification file.
///
/// The generator reads the ASDL specification at `src` and writes five files
/// into the parent directory of `dst`:
///
/// - `<stem>.rs`: shared types (fieldless enums and plain copyable structs).
/// - `<stem>_arena.rs`: the arena-allocated tree, which uses a `bumpalo` arena lifetime.
/// - `<stem>_owned.rs`: the self-owned tree, which uses `Box` links and standard `Vec` containers.
/// - `<stem>_views.rs`: the read-only view API over both tree representations.
/// - `<stem>_sexpr.rs`: the [`Sexpr`](crate::Sexpr) implementations for the generated AST types.
///
/// This function returns the paths of the five generated files.
///
/// # Errors
///
/// If the generator cannot read `src` or write an output file, this function
/// returns an I/O error. If the ASDL specification is not valid, this function
/// returns a syntax or code-generation error.
///
/// # Examples
///
/// ```
/// use std::fs;
///
/// let dir = std::env::temp_dir().join("dbt_asdl_doctest");
/// fs::create_dir_all(&dir)?;
///
/// let spec = dir.join("expr.asdl");
/// fs::write(&spec, "module Expr { expr = Lit(int value) | Add(expr left, expr right) }")?;
///
/// let files = dbt_asdl::codegen::rust::generate_trees(&spec, dir.join("gen.rs"))?;
/// assert_eq!(files.len(), 5);
/// assert!(dir.join("gen_arena.rs").is_file());
/// assert!(dir.join("gen_owned.rs").is_file());
///
/// fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn generate_trees(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    let input_name = src
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input.asdl");
    let mod_node = asdl::parse(src)?;
    let generated = generate(&mod_node, input_name)?;

    let stem = dst.file_stem().and_then(|s| s.to_str()).unwrap_or("gen");
    let ext = dst
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    let mut written = Vec::with_capacity(5);
    fs::write(dst, &generated.shared)?;
    written.push(dst.to_path_buf());

    for (suffix, content) in [
        ("_arena", &generated.arena),
        ("_owned", &generated.owned),
        ("_views", &generated.views),
        ("_sexpr", &generated.sexpr),
    ] {
        let sibling = dst.with_file_name(format!("{stem}{suffix}{ext}"));
        fs::write(&sibling, content)?;
        written.push(sibling);
    }

    Ok(written)
}
