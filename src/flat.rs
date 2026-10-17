use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::asdl::{self, Constructor, Field, Module, Type, TypeKind};
use crate::rust::{CodegenError, camel, rust_field, uncamel};

const BUILTIN_TYPES: &[&str] = &["identifier", "string", "byte", "int", "long", "bool"];

fn builtin_enc(ty: &str) -> Option<&'static str> {
    match ty {
        "identifier" | "string" => Some("Str"),
        "byte" => Some("Byte"),
        "int" => Some("Int"),
        "long" => Some("Long"),
        "bool" => Some("Bool"),
        _ => None,
    }
}

pub(crate) fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedFlat {
    pub(crate) flat: String,
    pub(crate) convert: String,
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
    stable: bool,
    attributes: Vec<Field>,
    kind: InfoKind,
    fields: Vec<Field>,
    constructors: Vec<Constructor>,
    type_index: Option<usize>,
    node_section: Option<usize>,
    seq_section: Option<usize>,
    ctor_off: Option<usize>,
    shared: bool,
}

impl TypeInfo {
    fn from_dfn(dfn: &Type) -> Self {
        let name = dfn.name.clone();
        let rust = camel(&dfn.name);
        let stable = dfn.annotations.iter().any(|a| a == "stable");
        let attributes = dfn.value.attributes().to_vec();
        match &dfn.value {
            TypeKind::Product(prod) => Self {
                name,
                rust,
                stable,
                attributes,
                kind: InfoKind::Product,
                fields: prod.fields.clone(),
                constructors: Vec::new(),
                type_index: None,
                node_section: None,
                seq_section: None,
                ctor_off: None,
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
                    stable,
                    attributes,
                    kind,
                    fields: Vec::new(),
                    constructors: sum.types.clone(),
                    type_index: None,
                    node_section: None,
                    seq_section: None,
                    ctor_off: None,
                    shared: false,
                }
            }
        }
    }

    fn ctor_has_record(&self, ctor: &Constructor) -> bool {
        !ctor.fields.is_empty() || !self.attributes.is_empty()
    }

    fn record_fields(&self, ctor: Option<&Constructor>) -> Vec<Field> {
        let mut out = self.attributes.clone();
        match ctor {
            Some(c) => out.extend(c.fields.iter().cloned()),
            None => out.extend(self.fields.iter().cloned()),
        }
        out
    }
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

fn compute_shared(order: &[String], infos: &mut HashMap<String, TypeInfo>) {
    fn is_plain(
        name: &str,
        infos: &HashMap<String, TypeInfo>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let Some(info) = infos.get(name) else {
            return false;
        };
        if info.kind != InfoKind::Product || visiting.contains(name) {
            return false;
        }
        visiting.insert(name.to_string());
        let ok = all_fields(info).iter().all(|f| {
            let t = f.ty.as_str();
            if BUILTIN_TYPES.contains(&t) {
                return true;
            }
            if infos
                .get(t)
                .is_some_and(|ti| ti.kind == InfoKind::SimpleSum)
            {
                return true;
            }
            is_plain(t, infos, visiting)
        });
        visiting.remove(name);
        ok
    }

    fn is_shared(
        name: &str,
        infos: &HashMap<String, TypeInfo>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        let Some(info) = infos.get(name) else {
            return false;
        };
        if info.kind != InfoKind::Product || visiting.contains(name) {
            return false;
        }
        visiting.insert(name.to_string());
        let ok = all_fields(info).iter().all(|f| {
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
        ok
    }

    let mut shared_map = HashMap::new();
    for name in order {
        if infos[name].kind == InfoKind::Product {
            let mut v1 = HashSet::new();
            let mut v2 = HashSet::new();
            let s = is_plain(name, infos, &mut v1) && is_shared(name, infos, &mut v2);
            shared_map.insert(name.clone(), s);
        }
    }
    for (name, s) in shared_map {
        if let Some(info) = infos.get_mut(&name) {
            info.shared = s;
        }
    }
}

struct Layout {
    types: Vec<String>,
    type_index: HashMap<String, usize>,
    sections: Vec<(String, String)>,
    payload_section: HashMap<(String, usize), usize>,
    seq_pools: Vec<(String, String, usize)>,
    seq_pool_section: HashMap<String, usize>,
}

impl Layout {
    fn build(
        mod_node: &Module,
        infos: &mut HashMap<String, TypeInfo>,
    ) -> Result<Self, CodegenError> {
        let mut types: Vec<String> = BUILTIN_TYPES.iter().map(|&s| s.to_string()).collect();
        types.extend(mod_node.dfns.iter().map(|d| d.name.clone()));

        let type_index: HashMap<String, usize> = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), i))
            .collect();

        for (i, name) in types.iter().enumerate() {
            if let Some(info) = infos.get_mut(name) {
                info.type_index = Some(i);
            }
        }

        let mut seq_used = Vec::new();
        for d in &mod_node.dfns {
            for f in all_fields(&infos[&d.name]) {
                if f.seq && !seq_used.contains(&f.ty) {
                    seq_used.push(f.ty.clone());
                }
            }
        }

        let mut sections = vec![("strings".to_string(), "u8".to_string())];
        let mut payload_section = HashMap::new();
        let mut seq_pools = Vec::new();

        for d in &mod_node.dfns {
            let (kind, name, rust, constructors, has_records): (
                InfoKind,
                String,
                String,
                Vec<Constructor>,
                Vec<bool>,
            ) = {
                let info = &infos[&d.name];
                let has_records = info
                    .constructors
                    .iter()
                    .map(|c| info.ctor_has_record(c))
                    .collect();
                (
                    info.kind,
                    info.name.clone(),
                    info.rust.clone(),
                    info.constructors.clone(),
                    has_records,
                )
            };

            if matches!(kind, InfoKind::Product | InfoKind::CompoundSum) {
                let sec_idx = sections.len();
                if let Some(info) = infos.get_mut(&d.name) {
                    info.node_section = Some(sec_idx);
                }
                let elem = if kind == InfoKind::Product {
                    rust
                } else {
                    "FlatNode".to_string()
                };
                sections.push((format!("{name}_nodes"), elem));
            }

            if kind == InfoKind::CompoundSum {
                for (tag, c) in constructors.iter().enumerate() {
                    if has_records[tag] {
                        payload_section.insert((name.clone(), tag), sections.len());
                        sections.push((
                            format!("{name}_{}", uncamel(&c.name)),
                            format!("{name}::{}", camel(&c.name)),
                        ));
                    }
                }
            }
        }

        for t in &types {
            if !seq_used.contains(t) {
                continue;
            }
            let elem = if t == "identifier" || t == "string" {
                "FlatStr".to_string()
            } else {
                let info = &infos[t];
                if info.kind == InfoKind::SimpleSum {
                    return Err(CodegenError(format!(
                        "{t}*: sequences of simple sums are unsupported"
                    )));
                }
                if !matches!(info.kind, InfoKind::Product | InfoKind::CompoundSum) {
                    return Err(CodegenError(format!(
                        "{t}*: sequences of scalars are unsupported"
                    )));
                }
                format!("{}Ref", info.rust)
            };
            let idx = sections.len();
            sections.push((format!("{t}_seq"), elem.clone()));
            seq_pools.push((t.clone(), elem, idx));
            if let Some(info) = infos.get_mut(t) {
                info.seq_section = Some(idx);
            }
        }

        let seq_pool_section: HashMap<String, usize> = seq_pools
            .iter()
            .map(|(t, _, idx)| (t.clone(), *idx))
            .collect();

        let mut ctor_count = 0usize;
        for d in &mod_node.dfns {
            let info = infos.get_mut(&d.name).unwrap();
            if !matches!(info.kind, InfoKind::SimpleSum | InfoKind::CompoundSum) {
                continue;
            }
            info.ctor_off = Some(ctor_count);
            ctor_count += info.constructors.len();
        }

        Ok(Self {
            types,
            type_index,
            sections,
            payload_section,
            seq_pools,
            seq_pool_section,
        })
    }
}

fn record_ty(f: &Field, infos: &HashMap<String, TypeInfo>) -> String {
    let t = f.ty.as_str();
    if f.seq {
        return "FlatSeq".to_string();
    }
    if f.opt {
        return match t {
            "bool" => "OptBool".to_string(),
            "byte" => "OptByte".to_string(),
            "int" => "OptInt".to_string(),
            "long" => "OptLong".to_string(),
            "identifier" | "string" => "FlatStr".to_string(),
            _ if infos[t].kind == InfoKind::SimpleSum => "u8".to_string(),
            _ => format!("{}Ref", camel(t)),
        };
    }
    match t {
        "bool" | "byte" => "u8".to_string(),
        "int" => "i32".to_string(),
        "long" => "i64".to_string(),
        "identifier" | "string" => "FlatStr".to_string(),
        _ if infos[t].kind == InfoKind::SimpleSum => "u8".to_string(),
        _ => format!("{}Ref", camel(t)),
    }
}

fn field_enc(
    f: &Field,
    infos: &HashMap<String, TypeInfo>,
    layout: &Layout,
) -> (&'static str, Option<usize>) {
    let t = f.ty.as_str();
    if f.seq {
        return ("Seq", Some(layout.type_index[t]));
    }
    let opt = f.opt;
    match t {
        "bool" => (if opt { "OptBool" } else { "Bool" }, None),
        "byte" => (if opt { "OptByte" } else { "Byte" }, None),
        "int" => (if opt { "OptInt" } else { "Int" }, None),
        "long" => (if opt { "OptLong" } else { "Long" }, None),
        "identifier" | "string" => (if opt { "OptStr" } else { "Str" }, None),
        _ if infos[t].kind == InfoKind::SimpleSum => (
            if opt { "OptSimpleSum" } else { "SimpleSum" },
            Some(layout.type_index[t]),
        ),
        _ => (
            if opt { "OptNode" } else { "Node" },
            Some(layout.type_index[t]),
        ),
    }
}

fn emit_ref_newtypes(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    w.push("// A typed node reference per product and compound sum: a".to_string());
    w.push("// repr(transparent) u32 index into the type's node section. NONE is".to_string());
    w.push("// the None of an optional reference; check with is_none().".to_string());
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if !matches!(info.kind, InfoKind::Product | InfoKind::CompoundSum) {
            continue;
        }
        w.push(String::new());
        w.push(format!("/// A reference to a `{}` node.", info.name));
        w.push("#[repr(transparent)]".to_string());
        w.push("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]".to_string());
        w.push(format!("pub struct {}Ref(pub u32);", info.rust));
        w.push(String::new());
        w.push(format!("impl {}Ref {{", info.rust));
        w.push("    /// The None of an optional reference.".to_string());
        w.push(format!(
            "    pub const NONE: Self = {}Ref(NONE_U32);",
            info.rust
        ));
        w.push(String::new());
        w.push("    /// Whether this is the None sentinel.".to_string());
        w.push("    pub fn is_none(self) -> bool {".to_string());
        w.push("        self.0 == NONE_U32".to_string());
        w.push("    }".to_string());
        w.push("}".to_string());
    }
    w.push(String::new());
}

fn emit_tag_enums(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
) -> Result<(), CodegenError> {
    w.push("// The constructor tag of every compound sum: the index within the".to_string());
    w.push("// sum, in sql.asdl declaration order. Append new constructors at the".to_string());
    w.push("// end; a reorder is a schema break.".to_string());
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind != InfoKind::CompoundSum {
            continue;
        }
        if info.constructors.len() > 0xFFFF {
            return Err(CodegenError(format!(
                "{}: too many constructors for a u16 tag",
                info.name
            )));
        }
        w.push(String::new());
        w.push(format!(
            "/// The constructor tag of a `{}` node.",
            info.name
        ));
        w.push("#[repr(u16)]".to_string());
        w.push("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]".to_string());
        w.push(format!("pub enum {}Tag {{", info.rust));
        for (tag, c) in info.constructors.iter().enumerate() {
            w.push(format!("    {} = {tag},", camel(&c.name)));
        }
        w.push("}".to_string());
        w.push(String::new());
        w.push(format!("impl {}Tag {{", info.rust));
        w.push("    /// The tag's constructor, or None for a value this schema".to_string());
        w.push("    /// does not know (a skewed library's later addition).".to_string());
        w.push("    pub fn from_u16(v: u16) -> Option<Self> {".to_string());
        w.push("        Some(match v {".to_string());
        for (tag, c) in info.constructors.iter().enumerate() {
            w.push(format!(
                "            {tag} => {}Tag::{},",
                info.rust,
                camel(&c.name)
            ));
        }
        w.push("            _ => return None,".to_string());
        w.push("        })".to_string());
        w.push("    }".to_string());
        w.push("}".to_string());
    }
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::SimpleSum && info.constructors.len() > 0xFF {
            return Err(CodegenError(format!(
                "{}: a simple sum with > 255 constructors does not fit the u8 encoding",
                info.name
            )));
        }
    }
    w.push(String::new());
    Ok(())
}

fn emit_records(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    w.push("// The record types: one #[repr(C)] struct per product and per".to_string());
    w.push("// payload-carrying constructor. Field order is attributes first,".to_string());
    w.push("// then declaration order; the C ABI fixes the layout. These same".to_string());
    w.push("// structs are the @stable types' typed projections: stability is a".to_string());
    w.push("// contract over this layout, not a second encoding.".to_string());
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            w.push(String::new());
            if info.stable {
                w.push(format!(
                    "/// The `{}` record (@stable: shape guaranteed by",
                    info.name
                ));
                w.push("/// the `abi_version` load handshake).".to_string());
            } else {
                w.push(format!("/// The `{}` record.", info.name));
            }
            w.push("#[repr(C)]".to_string());
            w.push("#[derive(Clone, Copy, Debug)]".to_string());
            w.push(format!("pub struct {} {{", info.rust));
            for f in info.record_fields(None) {
                let fname = f.name.as_deref().unwrap();
                w.push(format!(
                    "    pub {}: {},",
                    rust_field(fname),
                    record_ty(&f, infos)
                ));
            }
            w.push("}".to_string());
        } else if info.kind == InfoKind::CompoundSum {
            if !info.constructors.iter().any(|c| info.ctor_has_record(c)) {
                continue;
            }
            w.push(String::new());
            w.push(format!(
                "/// The `{}` payload records: one struct per",
                info.name
            ));
            w.push("/// constructor, attributes first.".to_string());
            if info.stable {
                w.push("///".to_string());
                w.push("/// @stable: shape guaranteed by the `abi_version` load".to_string());
                w.push("/// handshake.".to_string());
            }
            w.push(format!("pub mod {} {{", info.name));
            w.push("    use super::*;".to_string());
            for c in &info.constructors {
                if !info.ctor_has_record(c) {
                    continue;
                }
                w.push(String::new());
                w.push(format!("    /// `{}::{}`.", info.rust, camel(&c.name)));
                w.push("    #[repr(C)]".to_string());
                w.push("    #[derive(Clone, Copy, Debug)]".to_string());
                w.push(format!("    pub struct {} {{", camel(&c.name)));
                for f in info.record_fields(Some(c)) {
                    let fname = f.name.as_deref().unwrap();
                    w.push(format!(
                        "        pub {}: {},",
                        rust_field(fname),
                        record_ty(&f, infos)
                    ));
                }
                w.push("    }".to_string());
            }
            w.push("}".to_string());
        }
    }
    w.push(String::new());
}

fn emit_builder(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
    layout: &Layout,
) {
    w.push("// ------------------------------------------------------------------".to_string());
    w.push("// The library-side builder: the freeze buffer the converter writes".to_string());
    w.push("// into. Hosts never build trees, so all of it is behind `convert`.".to_string());
    w.push("// ------------------------------------------------------------------".to_string());
    w.push(String::new());
    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("/// The flat tree under construction: one growable buffer per section.".to_string());
    w.push("/// Indices need no fixups on growth, so nodes can be pushed in any".to_string());
    w.push("/// order (children first is the natural order for lowerings). `finish`".to_string());
    w.push("/// freezes the buffers into a `FlatTree` for the host.".to_string());
    w.push("pub struct TreeBuilder {".to_string());
    for (name, elem) in &layout.sections {
        w.push(format!("    {name}: Vec<{elem}>,"));
    }
    w.push("}".to_string());
    w.push(String::new());
    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("impl TreeBuilder {".to_string());
    w.push("    /// An empty tree under construction.".to_string());
    w.push("    pub fn new() -> Self {".to_string());
    w.push("        TreeBuilder {".to_string());
    for (name, _) in &layout.sections {
        w.push(format!("            {name}: Vec::new(),"));
    }
    w.push("        }".to_string());
    w.push("    }".to_string());
    w.push(String::new());
    w.push("    /// A string into the tree's UTF-8 blob.".to_string());
    w.push("    pub fn push_str(&mut self, s: &str) -> FlatStr {".to_string());
    w.push("        let off = self.strings.len() as u32;".to_string());
    w.push("        self.strings.extend_from_slice(s.as_bytes());".to_string());
    w.push("        FlatStr { off, len: s.len() as u32 }".to_string());
    w.push("    }".to_string());

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            w.push(String::new());
            w.push(format!("    /// Push a `{}` node.", info.name));
            w.push(format!(
                "    pub fn push_{}(&mut self, r: {}) -> {}Ref {{",
                info.name, info.rust, info.rust
            ));
            w.push(format!(
                "        let i = self.{}_nodes.len() as u32;",
                info.name
            ));
            w.push(format!("        self.{}_nodes.push(r);", info.name));
            w.push(format!("        {}Ref(i)", info.rust));
            w.push("    }".to_string());
        } else if info.kind == InfoKind::CompoundSum {
            for c in &info.constructors {
                let cname = camel(&c.name);
                let uname = uncamel(&c.name);
                w.push(String::new());
                if info.ctor_has_record(c) {
                    w.push(format!("    /// Push a `{cname}` node of `{}`.", info.name));
                    w.push(format!(
                        "    pub fn push_{}_{uname}(&mut self, r: {}::{cname}) -> {}Ref {{",
                        info.name, info.name, info.rust
                    ));
                    w.push(format!(
                        "        let payload = self.{}_{uname}.len() as u32;",
                        info.name
                    ));
                    w.push(format!("        self.{}_{uname}.push(r);", info.name));
                    w.push(format!(
                        "        let i = self.{}_nodes.len() as u32;",
                        info.name
                    ));
                    w.push(format!("        self.{}_nodes.push(FlatNode {{", info.name));
                    w.push(format!(
                        "            tag: {}Tag::{cname} as u16,",
                        info.rust
                    ));
                    w.push("            _pad: 0,".to_string());
                    w.push("            payload,".to_string());
                    w.push("        });".to_string());
                    w.push(format!("        {}Ref(i)", info.rust));
                    w.push("    }".to_string());
                } else {
                    w.push(format!(
                        "    /// Push a bare `{cname}` node of `{}`.",
                        info.name
                    ));
                    w.push(format!(
                        "    pub fn push_{}_{uname}(&mut self) -> {}Ref {{",
                        info.name, info.rust
                    ));
                    w.push(format!(
                        "        let i = self.{}_nodes.len() as u32;",
                        info.name
                    ));
                    w.push(format!("        self.{}_nodes.push(FlatNode {{", info.name));
                    w.push(format!(
                        "            tag: {}Tag::{cname} as u16,",
                        info.rust
                    ));
                    w.push("            _pad: 0,".to_string());
                    w.push("            payload: NONE_U32,".to_string());
                    w.push("        });".to_string());
                    w.push(format!("        {}Ref(i)", info.rust));
                    w.push("    }".to_string());
                }
            }
        }
    }

    for (t, elem, _) in &layout.seq_pools {
        w.push(String::new());
        w.push(format!("    /// A sequence of `{t}` into the pool."));
        w.push(format!(
            "    pub fn push_{t}_seq(&mut self, items: &[{elem}]) -> FlatSeq {{"
        ));
        w.push(format!("        let off = self.{t}_seq.len() as u32;"));
        w.push(format!("        self.{t}_seq.extend_from_slice(items);"));
        w.push("        FlatSeq { off, len: items.len() as u32 }".to_string());
        w.push("    }".to_string());
    }

    w.push(String::new());
    w.push("    /// Freeze the buffers into a host-ready `FlatTree`. The buffers are".to_string());
    w.push("    /// boxed in place and the header points at them; `release` (called".to_string());
    w.push("    /// by the host, or by `Tree`'s Drop) frees everything.".to_string());
    w.push("    pub fn finish(self, root: FileRef) -> *mut FlatTree {".to_string());
    w.push("        let mut sections: Vec<FlatSection> = Vec::new();".to_string());
    w.push("        macro_rules! sec {".to_string());
    w.push("            ($v:expr, $elem:ty) => {{".to_string());
    w.push("                let b = $v.into_boxed_slice();".to_string());
    w.push("                let len = b.len() as u32;".to_string());
    w.push("                let ptr = Box::into_raw(b) as *mut u8;".to_string());
    w.push("                sections.push(FlatSection {".to_string());
    w.push("                    ptr,".to_string());
    w.push("                    len,".to_string());
    w.push("                    stride: std::mem::size_of::<$elem>() as u32,".to_string());
    w.push("                });".to_string());
    w.push("            }};".to_string());
    w.push("        }".to_string());
    for (name, elem) in &layout.sections {
        w.push(format!("        sec!(self.{name}, {elem});"));
    }
    w.push("        let sections = Box::into_raw(sections.into_boxed_slice());".to_string());
    w.push("        let tree = Box::new(FlatTree {".to_string());
    w.push("            abi_version: ABI_VERSION,".to_string());
    w.push("            schema_hash: SCHEMA_HASH,".to_string());
    w.push("            release,".to_string());
    w.push("            schema: &SCHEMA,".to_string());
    w.push("            sections: sections as *mut FlatSection,".to_string());
    w.push(format!(
        "            section_count: {},",
        layout.sections.len()
    ));
    w.push("            root_index: root.0,".to_string());
    w.push("        });".to_string());
    w.push("        Box::into_raw(tree)".to_string());
    w.push("    }".to_string());
    w.push("}".to_string());
    w.push(String::new());
    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("impl Default for TreeBuilder {".to_string());
    w.push("    fn default() -> Self {".to_string());
    w.push("        Self::new()".to_string());
    w.push("    }".to_string());
    w.push("}".to_string());
    w.push(String::new());
    w.push("/// The tree destructor named by the header: frees the sections (each a".to_string());
    w.push("/// boxed slice of plain data) and the header itself.".to_string());
    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("unsafe extern \"C\" fn release(tree: *mut FlatTree) {".to_string());
    w.push("    unsafe {".to_string());
    w.push("        let tree = Box::from_raw(tree);".to_string());
    w.push("        let sections = Box::from_raw(std::ptr::slice_from_raw_parts_mut(".to_string());
    w.push("            tree.sections,".to_string());
    w.push("            tree.section_count as usize,".to_string());
    w.push("        ));".to_string());
    w.push("        for (i, s) in sections.iter().enumerate() {".to_string());
    w.push("            let ptr = s.ptr;".to_string());
    w.push("            let len = s.len as usize;".to_string());
    w.push("            match i {".to_string());
    for (i, (_, elem)) in layout.sections.iter().enumerate() {
        w.push(format!("                {i} => {{"));
        w.push("                    drop(Box::from_raw(".to_string());
        w.push(format!(
            "                        std::ptr::slice_from_raw_parts_mut(ptr as *mut {elem}, len),"
        ));
        w.push("                    ));".to_string());
        w.push("                }".to_string());
    }
    w.push(
        "                _ => unreachable!(\"section_count is fixed by this build\"),".to_string(),
    );
    w.push("            }".to_string());
    w.push("        }".to_string());
    w.push("    }".to_string());
    w.push("}".to_string());
    w.push(String::new());
}

fn emit_schema(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
    layout: &Layout,
) -> Result<(), CodegenError> {
    w.push("// ------------------------------------------------------------------".to_string());
    w.push("// The schema statics: the schema-as-data the host's introspection".to_string());
    w.push("// reads through the tree header. Identity is by name; indices are".to_string());
    w.push("// this build's own. Library-side: behind `builder` like TreeBuilder.".to_string());
    w.push("// ------------------------------------------------------------------".to_string());
    w.push(String::new());

    fn field_table_name(owner: &str) -> String {
        format!("FIELDS_{}", owner.replace("::", "_").to_uppercase())
    }

    fn emit_field_table(
        w: &mut Vec<String>,
        owner: &str,
        fields: &[Field],
        infos: &HashMap<String, TypeInfo>,
        layout: &Layout,
    ) {
        if fields.is_empty() {
            return;
        }
        w.push("#[cfg(feature = \"builder\")]".to_string());
        w.push(format!(
            "static {}: &[FlatFieldInfo] = &[",
            field_table_name(owner)
        ));
        for f in fields {
            let fname = f.name.as_deref().unwrap();
            let (enc, tidx) = field_enc(f, infos, layout);
            let tidx_str = tidx.map_or_else(|| "u16::MAX".to_string(), |idx| idx.to_string());
            w.push("    FlatFieldInfo {".to_string());
            w.push(format!("        name: FlatName::from_static(\"{fname}\"),"));
            w.push(format!(
                "        offset: std::mem::offset_of!({owner}, {}) as u32,",
                rust_field(fname)
            ));
            w.push(format!("        enc: FlatEnc::{enc} as u8,"));
            w.push(format!("        type_index: {tidx_str},"));
            w.push("    },".to_string());
        }
        w.push("];".to_string());
        w.push(String::new());
    }

    fn fields_ptr(owner: &str, fields: &[Field]) -> (String, String) {
        if fields.is_empty() {
            ("std::ptr::null()".to_string(), "0".to_string())
        } else {
            let t = field_table_name(owner);
            (format!("{t}.as_ptr()"), format!("{t}.len() as u32"))
        }
    }

    let mut product_fields: HashMap<String, (String, Vec<Field>)> = HashMap::new();
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            let rf = info.record_fields(None);
            emit_field_table(w, &info.rust, &rf, infos, layout);
            product_fields.insert(info.name.clone(), (info.rust.clone(), rf));
        } else if info.kind == InfoKind::CompoundSum {
            for c in &info.constructors {
                if info.ctor_has_record(c) {
                    let owner = format!("{}::{}", info.name, camel(&c.name));
                    let rf = info.record_fields(Some(c));
                    emit_field_table(w, &owner, &rf, infos, layout);
                }
            }
        }
    }

    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("static CTORS: &[FlatCtorInfo] = &[".to_string());
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if !matches!(info.kind, InfoKind::SimpleSum | InfoKind::CompoundSum) {
            continue;
        }
        for (tag, c) in info.constructors.iter().enumerate() {
            let (fptr, fcnt, psec) =
                if let Some(&sec) = layout.payload_section.get(&(info.name.clone(), tag)) {
                    let owner = format!("{}::{}", info.name, camel(&c.name));
                    let rf = info.record_fields(Some(c));
                    let (fp, fc) = fields_ptr(&owner, &rf);
                    (fp, fc, sec.to_string())
                } else {
                    (
                        "std::ptr::null()".to_string(),
                        "0".to_string(),
                        "-1".to_string(),
                    )
                };
            w.push("    FlatCtorInfo {".to_string());
            w.push(format!(
                "        name: FlatName::from_static(\"{}\"),",
                camel(&c.name)
            ));
            w.push(format!("        tag: {tag},"));
            w.push(format!("        type_index: {},", info.type_index.unwrap()));
            w.push(format!("        payload_section: {psec},"));
            w.push(format!("        fields: {fptr},"));
            w.push(format!("        field_count: {fcnt},"));
            w.push("    },".to_string());
        }
    }
    w.push("];".to_string());
    w.push(String::new());

    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("static TYPES: &[FlatTypeInfo] = &[".to_string());
    for t in &layout.types {
        let (kind, enc, node_section, seq_section, ctor_off, ctor_count, fptr, fcnt) =
            if BUILTIN_TYPES.contains(&t.as_str()) {
                let kind = "Builtin";
                let enc = builtin_enc(t).unwrap();
                let node_section = "-1".to_string();
                let seq_section = layout
                    .seq_pool_section
                    .get(t)
                    .map_or_else(|| "-1".to_string(), |idx| idx.to_string());
                (
                    kind,
                    enc,
                    node_section,
                    seq_section,
                    "0".to_string(),
                    "0".to_string(),
                    "std::ptr::null()".to_string(),
                    "0".to_string(),
                )
            } else {
                let info = &infos[t];
                let kind = match info.kind {
                    InfoKind::Product => "Product",
                    InfoKind::SimpleSum => "SimpleSum",
                    InfoKind::CompoundSum => "Sum",
                };
                let enc = if info.kind == InfoKind::SimpleSum {
                    "SimpleSum"
                } else {
                    "Node"
                };
                let node_section = info
                    .node_section
                    .map_or_else(|| "-1".to_string(), |idx| idx.to_string());
                let seq_section = info
                    .seq_section
                    .map_or_else(|| "-1".to_string(), |idx| idx.to_string());
                let (ctor_off, ctor_count) =
                    if matches!(info.kind, InfoKind::SimpleSum | InfoKind::CompoundSum) {
                        (
                            info.ctor_off.unwrap().to_string(),
                            info.constructors.len().to_string(),
                        )
                    } else {
                        ("0".to_string(), "0".to_string())
                    };
                let (fptr, fcnt) = if info.kind == InfoKind::Product {
                    let (owner, fields) = &product_fields[&info.name];
                    fields_ptr(owner, fields)
                } else {
                    ("std::ptr::null()".to_string(), "0".to_string())
                };
                (
                    kind,
                    enc,
                    node_section,
                    seq_section,
                    ctor_off,
                    ctor_count,
                    fptr,
                    fcnt,
                )
            };
        w.push("    FlatTypeInfo {".to_string());
        w.push(format!("        name: FlatName::from_static(\"{t}\"),"));
        w.push(format!("        kind: FlatTypeKind::{kind} as u8,"));
        w.push(format!("        enc: FlatEnc::{enc} as u8,"));
        w.push(format!("        node_section: {node_section},"));
        w.push(format!("        seq_section: {seq_section},"));
        w.push(format!("        ctor_off: {ctor_off},"));
        w.push(format!("        ctor_count: {ctor_count},"));
        w.push(format!("        fields: {fptr},"));
        w.push(format!("        field_count: {fcnt},"));
        w.push("    },".to_string());
    }
    w.push("];".to_string());
    w.push(String::new());

    let Some(&root_type) = layout.type_index.get("file") else {
        return Err(CodegenError(
            "flat layout requires a 'file' root type".to_string(),
        ));
    };

    w.push("/// The schema of the sql.asdl this build was generated from.".to_string());
    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("pub static SCHEMA: FlatSchema = FlatSchema {".to_string());
    w.push("    abi_version: ABI_VERSION,".to_string());
    w.push("    schema_hash: SCHEMA_HASH,".to_string());
    w.push("    types: TYPES.as_ptr(),".to_string());
    w.push("    type_count: TYPES.len() as u32,".to_string());
    w.push("    ctors: CTORS.as_ptr(),".to_string());
    w.push("    ctor_count: CTORS.len() as u32,".to_string());
    w.push(format!("    root_type: {root_type},"));
    w.push("};".to_string());
    w.push(String::new());

    w.push("/// The type indices of this build's schema: host code resolves types".to_string());
    w.push("/// by name through the descriptor; these constants serve the library's".to_string());
    w.push("/// own builder and tests.".to_string());
    w.push("#[cfg(feature = \"builder\")]".to_string());
    w.push("pub mod ty {".to_string());
    for t in &layout.types {
        w.push(format!(
            "    pub const {}: u16 = {};",
            t.to_uppercase(),
            layout.type_index[t]
        ));
    }
    w.push("}".to_string());
    w.push(String::new());
    Ok(())
}

fn emit_stable(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    let stable: Vec<&TypeInfo> = mod_node
        .dfns
        .iter()
        .map(|d| &infos[&d.name])
        .filter(|info| info.stable)
        .collect();
    if stable.is_empty() {
        return;
    }
    w.push("// ------------------------------------------------------------------".to_string());
    w.push("// The @stable typed projections".to_string());
    w.push("// ------------------------------------------------------------------".to_string());
    w.push(String::new());

    for info in stable {
        let r = &info.rust;
        match info.kind {
            InfoKind::Product => {
                w.push("impl<'t> Node<'t> {".to_string());
                w.push(format!(
                    "    /// This node as the @stable `{}` record, when it IS",
                    info.name
                ));
                w.push(format!(
                    "    /// a `{}`. The record shape is guaranteed by the",
                    info.name
                ));
                w.push(
                    "    /// load-time `abi_version` handshake; the stride check below".to_string(),
                );
                w.push("    /// is the per-call assertion of that contract.".to_string());
                w.push(format!(
                    "    pub fn as_{}(self) -> Option<&'t {r}> {{",
                    info.name
                ));
                w.push("        let tree = self.tree();".to_string());
                w.push(format!(
                    "        let ty = tree.type_by_name(\"{}\")?;",
                    info.name
                ));
                w.push("        if ty != self.type_index() {".to_string());
                w.push("            return None;".to_string());
                w.push("        }".to_string());
                w.push(
                    "        let (ptr, stride) = tree.product_ptr(ty, self.index())?;".to_string(),
                );
                w.push(format!(
                    "        if stride != std::mem::size_of::<{r}>() {{"
                ));
                w.push("            return None;".to_string());
                w.push("        }".to_string());
                w.push(format!("        Some(unsafe {{ &*(ptr as *const {r}) }})"));
                w.push("    }".to_string());
                w.push("}".to_string());
                w.push(String::new());
            }
            InfoKind::CompoundSum => {
                w.push(format!(
                    "/// A @stable `{}` node: the typed projection over the",
                    info.name
                ));
                w.push("/// frozen layout.".to_string());
                w.push("#[derive(Clone, Copy)]".to_string());
                w.push(format!("pub struct Stable{r}<'t> {{"));
                w.push("    node: Node<'t>,".to_string());
                w.push("}".to_string());
                w.push(String::new());
                w.push("impl<'t> Node<'t> {".to_string());
                w.push(format!(
                    "    /// This node as a @stable `{}`, when it IS one.",
                    info.name
                ));
                w.push("    /// The record shape is guaranteed by the load-time".to_string());
                w.push("    /// `abi_version` handshake.".to_string());
                w.push(format!(
                    "    pub fn as_{}(self) -> Option<Stable{r}<'t>> {{",
                    info.name
                ));
                w.push("        let tree = self.tree();".to_string());
                w.push(format!(
                    "        if tree.type_by_name(\"{}\") != Some(self.type_index()) {{",
                    info.name
                ));
                w.push("            return None;".to_string());
                w.push("        }".to_string());
                w.push(format!("        Some(Stable{r} {{ node: self }})"));
                w.push("    }".to_string());
                w.push("}".to_string());
                w.push(String::new());
                w.push(format!("impl<'t> Stable{r}<'t> {{"));
                w.push("    /// The generic cursor, for ports and introspection.".to_string());
                w.push("    pub fn node(self) -> Node<'t> {".to_string());
                w.push("        self.node".to_string());
                w.push("    }".to_string());
                w.push(String::new());
                w.push("    /// The constructor tag.".to_string());
                w.push(format!("    pub fn tag(self) -> Option<{r}Tag> {{"));
                w.push("        let tree = self.node.tree();".to_string());
                w.push(format!(
                    "        let ty = tree.type_by_name(\"{}\")?;",
                    info.name
                ));
                w.push(format!(
                    "        {r}Tag::from_u16(tree.sum_tag(ty, self.node.index())?)"
                ));
                w.push("    }".to_string());
                for c in &info.constructors {
                    if !info.ctor_has_record(c) {
                        continue;
                    }
                    let cname = camel(&c.name);
                    w.push(String::new());
                    w.push(format!(
                        "    /// The payload as `{cname}`, when that is the constructor."
                    ));
                    w.push(format!(
                        "    pub fn as_{}(self) -> Option<&'t {}::{cname}> {{",
                        uncamel(&cname),
                        info.name
                    ));
                    w.push("        let tree = self.node.tree();".to_string());
                    w.push(format!(
                        "        let ty = tree.type_by_name(\"{}\")?;",
                        info.name
                    ));
                    w.push("        let (ptr, stride) =".to_string());
                    w.push(format!(
                        "            tree.payload_ptr(ty, self.node.index(), {r}Tag::{cname} as u16)?;"
                    ));
                    w.push(format!(
                        "        if stride != std::mem::size_of::<{}::{cname}>() {{",
                        info.name
                    ));
                    w.push("            return None;".to_string());
                    w.push("        }".to_string());
                    w.push(format!(
                        "        Some(unsafe {{ &*(ptr as *const {}::{cname}) }})",
                        info.name
                    ));
                    w.push("    }".to_string());
                }
                w.push("}".to_string());
                w.push(String::new());
            }
            InfoKind::SimpleSum => {}
        }
    }
}

fn convert_expr(f: &Field, infos: &HashMap<String, TypeInfo>, e: &str, b: &str) -> String {
    let t = f.ty.as_str();
    if f.seq {
        if t == "string" {
            return format!(
                "{{ let items: Vec<FlatStr> = {e}.iter().map(|s| {b}.push_str(s)).collect(); {b}.push_string_seq(&items) }}"
            );
        }
        if t == "identifier" {
            return format!(
                "{{ let items: Vec<FlatStr> = {e}.iter().map(|u| {b}.push_str(u.as_str())).collect(); {b}.push_identifier_seq(&items) }}"
            );
        }
        let r = camel(t);
        if infos[t].shared {
            return format!(
                "{{ let items: Vec<{r}Ref> = {e}.iter().map(|x| lower_{t}({b}, *x)).collect(); {b}.push_{t}_seq(&items) }}"
            );
        }
        return format!(
            "{{ let items: Vec<{r}Ref> = {e}.iter().map(|x| lower_{t}({b}, x)).collect(); {b}.push_{t}_seq(&items) }}"
        );
    }
    if f.opt {
        return match t {
            "bool" => format!("OptBool::from({e})"),
            "byte" => format!("OptByte::from({e})"),
            "int" => format!("OptInt::from({e})"),
            "long" => format!("OptLong::from({e})"),
            "string" => format!("{e}.map(|s| {b}.push_str(s)).unwrap_or(FlatStr::NONE)"),
            "identifier" => {
                format!("{e}.map(|u| {b}.push_str(u.as_str())).unwrap_or(FlatStr::NONE)")
            }
            _ if infos[t].kind == InfoKind::SimpleSum => {
                format!("{e}.map(|x| x as u8).unwrap_or(NONE_U8)")
            }
            _ => format!(
                "{e}.map(|x| lower_{t}({b}, x)).unwrap_or({}Ref::NONE)",
                camel(t)
            ),
        };
    }
    match t {
        "bool" => format!("{e} as u8"),
        "byte" | "int" | "long" => e.to_string(),
        "string" => format!("{b}.push_str({e})"),
        "identifier" => format!("{b}.push_str({e}.as_str())"),
        _ if infos[t].kind == InfoKind::SimpleSum => format!("{e} as u8"),
        _ => format!("lower_{t}({b}, {e})"),
    }
}

fn freeze_label(f: &Field, infos: &HashMap<String, TypeInfo>) -> String {
    if f.seq {
        let elem_field = Field::new(f.ty.clone(), None, Vec::new());
        return format!("FlatSeq<{}>", record_ty(&elem_field, infos));
    }
    if !BUILTIN_TYPES.contains(&f.ty.as_str()) && infos[&f.ty].kind == InfoKind::SimpleSum {
        let suffix = if f.opt { "?" } else { "" };
        return format!("{}(u8){suffix}", f.ty);
    }
    record_ty(f, infos)
}

fn emit_freeze(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    let stable: Vec<&TypeInfo> = mod_node
        .dfns
        .iter()
        .map(|d| &infos[&d.name])
        .filter(|info| info.stable)
        .collect();
    if stable.is_empty() {
        return;
    }
    let mut referenced_simple_sums = HashSet::new();

    fn field_lines(
        rec: &str,
        fields: &[Field],
        indent: &str,
        infos: &HashMap<String, TypeInfo>,
        referenced_simple_sums: &mut HashSet<String>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for f in fields {
            if !BUILTIN_TYPES.contains(&f.ty.as_str()) && infos[&f.ty].kind == InfoKind::SimpleSum {
                referenced_simple_sums.insert(f.ty.clone());
            }
            let label = freeze_label(f, infos);
            let fname = f.name.as_deref().unwrap();
            let rf = rust_field(fname);
            lines.push(format!(
                "    writeln!(out, \"{indent}{fname}: {label} @{{}}\", std::mem::offset_of!({rec}, {rf})).unwrap();"
            ));
        }
        lines
    }

    w.push("// ------------------------------------------------------------------".to_string());
    w.push("// The @stable freeze dump".to_string());
    w.push("// ------------------------------------------------------------------".to_string());
    w.push(String::new());
    w.push("/// A canonical text dump of the `@stable` region's layout: record".to_string());
    w.push("/// sizes/alignments, field offsets and types, and variant".to_string());
    w.push("/// discriminants. `tests/freeze.rs` byte-compares it against the".to_string());
    w.push("/// checked-in snapshot `sql-abi/stable_layout.txt`; regenerating a".to_string());
    w.push("/// changed layout requires an `ABI_VERSION` bump.".to_string());
    w.push("pub fn freeze_stable_layout() -> String {".to_string());
    w.push("    let mut out = String::new();".to_string());
    w.push("    use std::fmt::Write as _;".to_string());
    w.push("    writeln!(out, \"abi_version: {}\", ABI_VERSION).unwrap();".to_string());

    for info in stable {
        let r = &info.rust;
        w.push("    writeln!(out).unwrap();".to_string());
        match info.kind {
            InfoKind::Product => {
                w.push(format!(
                    "    writeln!(out, \"type {}: product size {{}} align {{}}\",",
                    info.name
                ));
                w.push(format!(
                    "        std::mem::size_of::<{r}>(), std::mem::align_of::<{r}>()).unwrap();"
                ));
                for line in field_lines(
                    r,
                    &info.record_fields(None),
                    "    ",
                    infos,
                    &mut referenced_simple_sums,
                ) {
                    w.push(line);
                }
            }
            InfoKind::CompoundSum => {
                w.push(format!(
                    "    writeln!(out, \"type {}: sum\").unwrap();",
                    info.name
                ));
                for (tag, c) in info.constructors.iter().enumerate() {
                    let cname = camel(&c.name);
                    if info.ctor_has_record(c) {
                        let rec = format!("{}::{cname}", info.name);
                        w.push(format!(
                            "    writeln!(out, \"    variant {tag}: {cname} size {{}} align {{}}\","
                        ));
                        w.push(format!(
                            "        std::mem::size_of::<{rec}>(), std::mem::align_of::<{rec}>()).unwrap();"
                        ));
                        for line in field_lines(
                            &rec,
                            &info.record_fields(Some(c)),
                            "        ",
                            infos,
                            &mut referenced_simple_sums,
                        ) {
                            w.push(line);
                        }
                    } else {
                        w.push(format!(
                            "    writeln!(out, \"    variant {tag}: {cname} (bare)\").unwrap();"
                        ));
                    }
                }
            }
            InfoKind::SimpleSum => {}
        }
    }

    for d in &mod_node.dfns {
        if !referenced_simple_sums.contains(&d.name) {
            continue;
        }
        let info = &infos[&d.name];
        w.push("    writeln!(out).unwrap();".to_string());
        w.push(format!(
            "    writeln!(out, \"type {}: simple_sum (referenced by a stable field)\").unwrap();",
            d.name
        ));
        for (tag, c) in info.constructors.iter().enumerate() {
            w.push(format!(
                "    writeln!(out, \"    variant {tag}: {}\").unwrap();",
                camel(&c.name)
            ));
        }
    }

    w.push("    out".to_string());
    w.push("}".to_string());
    w.push(String::new());
}

fn emit_convert(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    w.push("use crate::*;".to_string());
    w.push("use dbt_sql_ast as ast;".to_string());
    w.push(String::new());
    w.push("/// Deep-convert a finished arena/owned tree (behind the read API) into".to_string());
    w.push("/// the flat boundary representation: one O(nodes) pass, children".to_string());
    w.push("/// first. Dialect libraries that lower into the arena tree call this".to_string());
    w.push("/// at the boundary.".to_string());
    w.push("pub fn convert_file(root: ast::FileRef) -> *mut FlatTree {".to_string());
    w.push("    let mut b = TreeBuilder::new();".to_string());
    w.push("    let root = lower_file(&mut b, root);".to_string());
    w.push("    b.finish(root)".to_string());
    w.push("}".to_string());

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::SimpleSum {
            continue;
        }
        w.push(String::new());
        if info.kind == InfoKind::Product {
            let arg = if info.shared {
                format!("ast::{}", info.rust)
            } else {
                format!("ast::{}Ref", info.rust)
            };
            w.push(format!(
                "fn lower_{}(b: &mut TreeBuilder, r: {arg}) -> {}Ref {{",
                info.name, info.rust
            ));
            w.push(format!("    let r = {} {{", info.rust));
            for f in info.record_fields(None) {
                let fname = f.name.as_deref().unwrap();
                let rf = rust_field(fname);
                let e = format!("r.{rf}()");
                w.push(format!(
                    "        {rf}: {},",
                    convert_expr(&f, infos, &e, "b")
                ));
            }
            w.push("    };".to_string());
            w.push(format!("    b.push_{}(r)", info.name));
            w.push("}".to_string());
        } else {
            let r = &info.rust;
            w.push(format!(
                "fn lower_{}(b: &mut TreeBuilder, r: ast::{r}Ref) -> {r}Ref {{",
                info.name
            ));
            w.push("    match r.view() {".to_string());
            for c in &info.constructors {
                let cname = camel(&c.name);
                if info.attributes.is_empty() && c.fields.is_empty() {
                    w.push(format!(
                        "        ast::{r}View::{cname} => b.push_{}_{}(),",
                        info.name,
                        uncamel(&c.name)
                    ));
                    continue;
                }
                w.push(format!("        ast::{r}View::{cname}(v) => {{"));
                w.push(format!("            let r = {}::{cname} {{", info.name));
                for f in info.record_fields(Some(c)) {
                    let fname = f.name.as_deref().unwrap();
                    let rf = rust_field(fname);
                    let e = format!("v.{rf}()");
                    w.push(format!(
                        "                {rf}: {},",
                        convert_expr(&f, infos, &e, "b")
                    ));
                }
                w.push("            };".to_string());
                w.push(format!(
                    "            b.push_{}_{}(r)",
                    info.name,
                    uncamel(&c.name)
                ));
                w.push("        }".to_string());
            }
            w.push("    }".to_string());
            w.push("}".to_string());
        }
    }
    w.push(String::new());
}

fn validate(mod_node: &Module, infos: &HashMap<String, TypeInfo>) -> Result<(), CodegenError> {
    for dfn in &mod_node.dfns {
        let mut fields: Vec<&Field> = dfn.value.attributes().iter().collect();
        match &dfn.value {
            TypeKind::Product(prod) => {
                fields.extend(prod.fields.iter());
            }
            TypeKind::Sum(sum) => {
                if !sum.attributes.is_empty() && sum.types.iter().all(|c| c.fields.is_empty()) {
                    return Err(CodegenError(format!(
                        "{}: attributes on a field-less sum make no sense",
                        dfn.name
                    )));
                }
                for c in &sum.types {
                    fields.extend(c.fields.iter());
                }
            }
        }
        for f in fields {
            let Some(fname) = f.name.as_deref() else {
                return Err(CodegenError(format!(
                    "{}: fields must be named (got positional '{}')",
                    dfn.name, f.ty
                )));
            };
            if f.quantifiers.len() > 1 {
                return Err(CodegenError(format!(
                    "{}.{fname}: at most one quantifier (? or *) is supported",
                    dfn.name
                )));
            }
            if !BUILTIN_TYPES.contains(&f.ty.as_str()) && !infos.contains_key(&f.ty) {
                return Err(CodegenError(format!(
                    "unsupported field type '{}' (field '{fname}')",
                    f.ty
                )));
            }
            if f.seq && matches!(f.ty.as_str(), "byte" | "int" | "long" | "bool") {
                return Err(CodegenError(format!(
                    "{}.{fname}: sequences of scalars are unsupported",
                    dfn.name
                )));
            }
            if f.seq
                && infos
                    .get(&f.ty)
                    .is_some_and(|info| info.kind == InfoKind::SimpleSum)
            {
                return Err(CodegenError(format!(
                    "{}.{fname}: sequences of simple sums are unsupported",
                    dfn.name
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn generate_flat_from_module(
    mod_node: &Module,
    input_name: &str,
    schema_hash: u64,
) -> Result<GeneratedFlat, CodegenError> {
    let mut builtins: Vec<&str> = asdl::BUILTIN_TYPES.to_vec();
    for &k in BUILTIN_TYPES {
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

    validate(mod_node, &infos)?;
    compute_shared(&order, &mut infos);
    let layout = Layout::build(mod_node, &mut infos)?;

    let mut flat = Vec::new();
    let mut conv = Vec::new();
    let header =
        format!("// File automatically generated by dbt-asdl from {input_name}. Do not edit.");

    flat.push(header.clone());
    flat.push(String::new());
    flat.push(include_str!("flat_runtime.rs").trim_end().to_string());
    flat.push(String::new());
    flat.push("/// FNV-1a/64 of the sql.asdl bytes this build was generated from.".to_string());
    flat.push("/// Library-side: the host reads the hash from the descriptor.".to_string());
    flat.push("#[cfg(feature = \"builder\")]".to_string());
    flat.push(format!(
        "pub const SCHEMA_HASH: u64 = 0x{schema_hash:016x};"
    ));
    flat.push(String::new());

    emit_ref_newtypes(&mut flat, mod_node, &infos);
    emit_tag_enums(&mut flat, mod_node, &infos)?;
    emit_records(&mut flat, mod_node, &infos);
    emit_builder(&mut flat, mod_node, &infos, &layout);
    emit_stable(&mut flat, mod_node, &infos);
    emit_freeze(&mut flat, mod_node, &infos);
    emit_schema(&mut flat, mod_node, &infos, &layout)?;

    conv.push(header);
    conv.push(String::new());
    emit_convert(&mut conv, mod_node, &infos);

    Ok(GeneratedFlat {
        flat: format!("{}\n", flat.join("\n")),
        convert: format!("{}\n", conv.join("\n")),
    })
}

/// Generates the flat `#[repr(C)]` Rust AST and arena-to-flat converter files from an ASDL file.
///
/// The generator reads the ASDL specification at `src` and writes two files
/// into the parent directory of `dst`:
///
/// - `dst` (for example, `gen_flat.rs`): the flat ABI boundary runtime (`FlatTree`,
///   `Tree`, `Node`, `Value`, `export_sql_dialect!`, and `loader`), `#[repr(C)]`
///   record structs, typed node reference newtypes, constructor tag enums,
///   `TreeBuilder`, `@stable` typed projections, layout freeze dump, and schema
///   descriptor tables.
/// - `gen_convert.rs`: functions that convert an arena or owned AST (through the
///   read-only view API) into the flat boundary representation.
///
/// The ASDL specification must define a `file` root type. This function returns
/// the paths of the two generated files.
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
/// let dir = std::env::temp_dir().join("dbt_asdl_flat_doctest");
/// fs::create_dir_all(&dir)?;
///
/// let spec = dir.join("sql.asdl");
/// fs::write(
///     &spec,
///     "module Sql { expr = Lit(int value) | Add(expr left, expr right)\n file = (expr* exprs) }",
/// )?;
///
/// let files = dbt_asdl::codegen::rust::generate_flat(&spec, dir.join("gen_flat.rs"))?;
/// assert_eq!(files.len(), 2);
/// assert!(dir.join("gen_flat.rs").is_file());
/// assert!(dir.join("gen_convert.rs").is_file());
///
/// fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn generate_flat(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let src = src.as_ref();
    let dst = dst.as_ref();
    let input_name = src
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input.asdl");
    let raw_bytes = fs::read(src)?;
    let schema_hash = fnv1a64(&raw_bytes);
    let source = std::str::from_utf8(&raw_bytes)?;
    let mod_node = asdl::parse_str(source)?;
    let generated = generate_flat_from_module(&mod_node, input_name, schema_hash)?;

    let convert_path = dst.with_file_name("gen_convert.rs");
    fs::write(dst, &generated.flat)?;
    fs::write(&convert_path, &generated.convert)?;

    Ok(vec![dst.to_path_buf(), convert_path])
}
