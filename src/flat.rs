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

            if kind == InfoKind::Product {
                let sec_idx = sections.len();
                if let Some(info) = infos.get_mut(&d.name) {
                    info.node_section = Some(sec_idx);
                }
                sections.push((format!("{name}_nodes"), rust));
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
        _ if infos[t].kind == InfoKind::SimpleSum => {
            (if opt { "OptSimpleSum" } else { "SimpleSum" }, Some(layout.type_index[t]))
        }
        _ => (if opt { "OptNode" } else { "Node" }, Some(layout.type_index[t])),
    }
}

fn emit_ref_newtypes(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    emit!(w, "// A typed node reference per product and compound sum: a");
    emit!(w, "// repr(transparent) u32 index into the type's node section. NONE is");
    emit!(w, "// the None of an optional reference; check with is_none().");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if !matches!(info.kind, InfoKind::Product | InfoKind::CompoundSum) {
            continue;
        }
        emit!(w, "");
        emit!(w, "/// A reference to a `{}` node.", info.name);
        emit!(w, "#[repr(transparent)]");
        emit!(w, "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]");
        emit!(w, "pub struct {}Ref(pub u32);", info.rust);
        emit!(w, "");
        emit!(w, "impl {}Ref {{", info.rust);
        emit!(w, "    /// The None of an optional reference.");
        emit!(w, "    pub const NONE: Self = {}Ref(NONE_U32);", info.rust);
        emit!(w, "");
        emit!(w, "    /// Whether this is the None sentinel.");
        emit!(w, "    pub fn is_none(self) -> bool {{");
        emit!(w, "        self.0 == NONE_U32");
        emit!(w, "    }}");
        emit!(w, "}}");
    }
    emit!(w, "");
}

fn emit_tag_enums(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
) -> Result<(), CodegenError> {
    emit!(w, "// The constructor tag of every compound sum: the index within the");
    emit!(w, "// sum, in sql.asdl declaration order. Append new constructors at the");
    emit!(w, "// end; a reorder is a schema break.");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind != InfoKind::CompoundSum {
            continue;
        }
        if info.constructors.len() > 0x100 {
            return Err(CodegenError(format!(
                "{}: too many constructors for an 8-bit packed tag",
                info.name
            )));
        }
        emit!(w, "");
        emit!(w, "/// The constructor tag of a `{}` node.", info.name);
        emit!(w, "#[repr(u16)]");
        emit!(w, "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]");
        emit!(w, "pub enum {}Tag {{", info.rust);
        for (tag, c) in info.constructors.iter().enumerate() {
            emit!(w, "    {} = {tag},", camel(&c.name));
        }
        emit!(w, "}}");
        emit!(w, "");
        emit!(w, "impl {}Tag {{", info.rust);
        emit!(w, "    /// The tag's constructor, or None for a value this schema");
        emit!(w, "    /// does not know (a skewed library's later addition).");
        emit!(w, "    pub fn from_u16(v: u16) -> Option<Self> {{");
        emit!(w, "        Some(match v {{");
        for (tag, c) in info.constructors.iter().enumerate() {
            emit!(w, "            {tag} => {}Tag::{},", info.rust, camel(&c.name));
        }
        emit!(w, "            _ => return None,");
        emit!(w, "        }})");
        emit!(w, "    }}");
        emit!(w, "}}");
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
    emit!(w, "");
    Ok(())
}

fn emit_records(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    emit!(w, "// The record types: one #[repr(C)] struct per product and per");
    emit!(w, "// payload-carrying constructor. Field order is attributes first,");
    emit!(w, "// then declaration order. For @stable types, this layout is");
    emit!(w, "// the public ABI contract and guaranteed by the load handshake,");
    emit!(w, "// and thus can be directly projected over the flat buffers.");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            emit!(w, "");
            if info.stable {
                emit!(w, "/// The `{}` record (@stable: shape guaranteed by", info.name);
                emit!(w, "/// the `abi_version` load handshake).");
            } else {
                emit!(w, "/// The `{}` record.", info.name);
            }
            emit!(w, "#[repr(C)]");
            emit!(w, "#[derive(Clone, Copy, Debug)]");
            emit!(w, "pub struct {} {{", info.rust);
            for f in info.record_fields(None) {
                let fname = f.name.as_deref().unwrap();
                emit!(w, "    pub {}: {},", rust_field(fname), record_ty(&f, infos));
            }
            emit!(w, "}}");
        } else if info.kind == InfoKind::CompoundSum {
            if !info.constructors.iter().any(|c| info.ctor_has_record(c)) {
                continue;
            }
            emit!(w, "");
            emit!(w, "/// The `{}` payload records: one struct per", info.name);
            emit!(w, "/// constructor, attributes first.");
            if info.stable {
                emit!(w, "///");
                emit!(w, "/// @stable: shape guaranteed by the `abi_version` load");
                emit!(w, "/// handshake.");
            }
            emit!(w, "pub mod {} {{", info.name);
            emit!(w, "    use super::*;");
            for c in &info.constructors {
                if !info.ctor_has_record(c) {
                    continue;
                }
                emit!(w, "");
                emit!(w, "    /// `{}::{}`.", info.rust, camel(&c.name));
                emit!(w, "    #[repr(C)]");
                emit!(w, "    #[derive(Clone, Copy, Debug)]");
                emit!(w, "    pub struct {} {{", camel(&c.name));
                for f in info.record_fields(Some(c)) {
                    let fname = f.name.as_deref().unwrap();
                    emit!(w, "        pub {}: {},", rust_field(fname), record_ty(&f, infos));
                }
                emit!(w, "    }}");
            }
            emit!(w, "}}");
        }
    }
    emit!(w, "");
}

fn emit_builder(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
    layout: &Layout,
) {
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "// The library-side builder: the freeze buffer the converter writes");
    emit!(w, "// into. Hosts never build trees, so all of it is behind `convert`.");
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "");
    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "/// The flat tree under construction: one growable buffer per section.");
    emit!(w, "/// Indices need no fixups on growth, so nodes can be pushed in any");
    emit!(w, "/// order (children first is the natural order for lowerings). `finish`");
    emit!(w, "/// freezes the buffers into a `FlatTree` for the host.");
    emit!(w, "pub struct TreeBuilder {{");
    for (name, elem) in &layout.sections {
        emit!(w, "    {name}: Vec<{elem}>,");
    }
    emit!(w, "}}");
    emit!(w, "");
    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "impl TreeBuilder {{");
    emit!(w, "    /// An empty tree under construction.");
    emit!(w, "    pub fn new() -> Self {{");
    emit!(w, "        TreeBuilder {{");
    for (name, _) in &layout.sections {
        emit!(w, "            {name}: Vec::new(),");
    }
    emit!(w, "        }}");
    emit!(w, "    }}");
    emit!(w, "");
    emit!(w, "    /// A string into the tree's UTF-8 blob.");
    emit!(w, "    pub fn push_str(&mut self, s: &str) -> FlatStr {{");
    emit!(w, "        let off = self.strings.len() as u32;");
    emit!(w, "        self.strings.extend_from_slice(s.as_bytes());");
    emit!(w, "        FlatStr {{ off, len: s.len() as u32 }}");
    emit!(w, "    }}");

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            emit!(w, "");
            emit!(w, "    /// Push a `{}` node.", info.name);
            emit!(
                w,
                "    pub fn push_{}(&mut self, r: {}) -> {}Ref {{",
                info.name,
                info.rust,
                info.rust
            );
            emit!(w, "        let i = self.{}_nodes.len() as u32;", info.name);
            emit!(w, "        self.{}_nodes.push(r);", info.name);
            emit!(w, "        {}Ref(i)", info.rust);
            emit!(w, "    }}");
        } else if info.kind == InfoKind::CompoundSum {
            for c in &info.constructors {
                let cname = camel(&c.name);
                let uname = uncamel(&c.name);
                emit!(w, "");
                if info.ctor_has_record(c) {
                    emit!(w, "    /// Push a `{cname}` node of `{}`.", info.name);
                    emit!(
                        w,
                        "    pub fn push_{}_{uname}(&mut self, r: {}::{cname}) -> {}Ref {{",
                        info.name,
                        info.name,
                        info.rust
                    );
                    emit!(w, "        let payload = self.{}_{uname}.len() as u32;", info.name);
                    emit!(w, "        self.{}_{uname}.push(r);", info.name);
                    emit!(
                        w,
                        "        {}Ref(pack_sum_ref({}Tag::{cname} as u16, payload))",
                        info.rust,
                        info.rust
                    );
                    emit!(w, "    }}");
                } else {
                    emit!(w, "    /// Push a bare `{cname}` node of `{}`.", info.name);
                    emit!(
                        w,
                        "    pub fn push_{}_{uname}(&mut self) -> {}Ref {{",
                        info.name,
                        info.rust
                    );
                    emit!(
                        w,
                        "        {}Ref(pack_sum_ref({}Tag::{cname} as u16, 0))",
                        info.rust,
                        info.rust
                    );
                    emit!(w, "    }}");
                }
            }
        }
    }

    for (t, elem, _) in &layout.seq_pools {
        emit!(w, "");
        emit!(w, "    /// A sequence of `{t}` into the pool.");
        emit!(w, "    pub fn push_{t}_seq(&mut self, items: &[{elem}]) -> FlatSeq {{");
        emit!(w, "        let off = self.{t}_seq.len() as u32;");
        emit!(w, "        self.{t}_seq.extend_from_slice(items);");
        emit!(w, "        FlatSeq {{ off, len: items.len() as u32 }}");
        emit!(w, "    }}");
    }

    emit!(w, "");
    emit!(w, "    /// Freeze the buffers into a host-ready `FlatTree`. The buffers are");
    emit!(w, "    /// boxed in place and the header points at them; `release` (called");
    emit!(w, "    /// by the host, or by `Tree`'s Drop) frees everything.");
    emit!(w, "    pub fn finish(self, root: FileRef) -> *mut FlatTree {{");
    emit!(w, "        let mut sections: Vec<FlatSection> = Vec::new();");
    emit!(w, "        macro_rules! sec {{");
    emit!(w, "            ($v:expr, $elem:ty) => {{{{");
    emit!(w, "                let b = $v.into_boxed_slice();");
    emit!(w, "                let len = b.len() as u32;");
    emit!(w, "                let ptr = Box::into_raw(b) as *mut u8;");
    emit!(w, "                sections.push(FlatSection {{");
    emit!(w, "                    ptr,");
    emit!(w, "                    len,");
    emit!(w, "                    stride: std::mem::size_of::<$elem>() as u32,");
    emit!(w, "                }});");
    emit!(w, "            }}}};");
    emit!(w, "        }}");
    for (name, elem) in &layout.sections {
        emit!(w, "        sec!(self.{name}, {elem});");
    }
    emit!(w, "        let sections = Box::into_raw(sections.into_boxed_slice());");
    emit!(w, "        let tree = Box::new(FlatTree {{");
    emit!(w, "            section_count: {},", layout.sections.len());
    emit!(w, "            root_index: root.0,");
    emit!(w, "            schema: &SCHEMA,");
    emit!(w, "            sections: sections as *mut FlatSection,");
    emit!(w, "            release,");
    emit!(w, "        }});");
    emit!(w, "        Box::into_raw(tree)");
    emit!(w, "    }}");
    emit!(w, "}}");
    emit!(w, "");
    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "impl Default for TreeBuilder {{");
    emit!(w, "    fn default() -> Self {{");
    emit!(w, "        Self::new()");
    emit!(w, "    }}");
    emit!(w, "}}");
    emit!(w, "");
    emit!(w, "/// The tree destructor named by the header: frees the sections (each a");
    emit!(w, "/// boxed slice of plain data) and the header itself.");
    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "unsafe extern \"C\" fn release(tree: *mut FlatTree) {{");
    emit!(w, "    unsafe {{");
    emit!(w, "        let tree = Box::from_raw(tree);");
    emit!(w, "        let sections = Box::from_raw(std::ptr::slice_from_raw_parts_mut(");
    emit!(w, "            tree.sections,");
    emit!(w, "            tree.section_count as usize,");
    emit!(w, "        ));");
    emit!(w, "        for (i, s) in sections.iter().enumerate() {{");
    emit!(w, "            let ptr = s.ptr;");
    emit!(w, "            let len = s.len as usize;");
    emit!(w, "            match i {{");
    for (i, (_, elem)) in layout.sections.iter().enumerate() {
        emit!(w, "                {i} => {{");
        emit!(w, "                    drop(Box::from_raw(");
        let cast_ptr = if elem == "u8" {
            "ptr".to_string()
        } else {
            format!("ptr as *mut {elem}")
        };
        emit!(
            w,
            "                        std::ptr::slice_from_raw_parts_mut({cast_ptr}, len),"
        );
        emit!(w, "                    ));");
        emit!(w, "                }}");
    }
    emit!(
        w,
        "                _ => unreachable!(\"section_count is fixed by this build\"),"
    );
    emit!(w, "            }}");
    emit!(w, "        }}");
    emit!(w, "    }}");
    emit!(w, "}}");
    emit!(w, "");
}

fn emit_schema(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
    layout: &Layout,
) -> Result<(), CodegenError> {
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "// The schema statics: the schema-as-data the host's introspection");
    emit!(w, "// reads through the tree header. Identity is by name; indices are");
    emit!(w, "// this build's own. Library-side: behind `builder` like TreeBuilder.");
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "");

    struct StringPool {
        bytes: String,
        offsets: HashMap<String, (u16, u16)>,
    }

    impl StringPool {
        fn new() -> Self {
            Self {
                bytes: String::new(),
                offsets: HashMap::new(),
            }
        }

        fn intern(&mut self, s: &str) -> Result<(u16, u16), CodegenError> {
            if let Some(&entry) = self.offsets.get(s) {
                return Ok(entry);
            }
            let off = u16::try_from(self.bytes.len()).map_err(|_| {
                CodegenError("schema string pool exceeds u16::MAX bytes".to_string())
            })?;
            let len = u16::try_from(s.len()).map_err(|_| {
                CodegenError(format!("schema identifier '{s}' exceeds u16::MAX bytes"))
            })?;
            self.bytes.push_str(s);
            self.offsets.insert(s.to_string(), (off, len));
            Ok((off, len))
        }
    }

    struct FieldRow {
        name: (u16, u16),
        owner: String,
        rust_field: String,
        tidx_str: String,
        enc: &'static str,
    }

    let mut pool = StringPool::new();
    let mut field_rows: Vec<FieldRow> = Vec::new();
    let mut product_field_range: HashMap<String, (u16, u16)> = HashMap::new();
    let mut ctor_field_range: HashMap<(String, usize), (u16, u16)> = HashMap::new();

    let push_fields = |owner: &str,
                       fields: &[Field],
                       pool: &mut StringPool,
                       rows: &mut Vec<FieldRow>|
     -> Result<(u16, u16), CodegenError> {
        if fields.is_empty() {
            return Ok((0, 0));
        }
        let foff = u16::try_from(rows.len())
            .map_err(|_| CodegenError("schema field table exceeds u16::MAX entries".to_string()))?;
        let fcnt = u16::try_from(fields.len())
            .map_err(|_| CodegenError("record field count exceeds u16::MAX".to_string()))?;
        for f in fields {
            let fname = f.name.as_deref().unwrap();
            let name = pool.intern(fname)?;
            let (enc, tidx) = field_enc(f, infos, layout);
            let tidx_str = tidx.map_or_else(|| "NONE_U16".to_string(), |idx| idx.to_string());
            rows.push(FieldRow {
                name,
                owner: owner.to_string(),
                rust_field: rust_field(fname),
                tidx_str,
                enc,
            });
        }
        Ok((foff, fcnt))
    };

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            let rf = info.record_fields(None);
            let range = push_fields(&info.rust, &rf, &mut pool, &mut field_rows)?;
            product_field_range.insert(info.name.clone(), range);
        } else if info.kind == InfoKind::CompoundSum {
            for (tag, c) in info.constructors.iter().enumerate() {
                if info.ctor_has_record(c) {
                    let owner = format!("{}::{}", info.name, camel(&c.name));
                    let rf = info.record_fields(Some(c));
                    let range = push_fields(&owner, &rf, &mut pool, &mut field_rows)?;
                    ctor_field_range.insert((info.name.clone(), tag), range);
                }
            }
        }
    }

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if matches!(info.kind, InfoKind::SimpleSum | InfoKind::CompoundSum) {
            for c in &info.constructors {
                pool.intern(&camel(&c.name))?;
            }
        }
    }

    for t in &layout.types {
        pool.intern(t)?;
    }

    emit!(w, "#[cfg(feature = \"builder\")]");
    if 34 + pool.bytes.len() <= 80 {
        emit!(w, "static SCHEMA_STRINGS: &[u8] = b\"{}\";", pool.bytes);
    } else {
        emit!(w, "static SCHEMA_STRINGS: &[u8] = b\"\\");
        let chunks: Vec<&str> = pool
            .bytes
            .as_bytes()
            .chunks(74)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        for (i, chunk) in chunks.iter().enumerate() {
            if i + 1 < chunks.len() {
                emit!(w, "    {chunk}\\");
            } else {
                emit!(w, "    {chunk}\";");
            }
        }
    }
    emit!(w, "");

    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "static FIELDS: &[FlatFieldInfo] = &[");
    for row in &field_rows {
        let (off, len) = row.name;
        emit!(w, "    FlatFieldInfo {{");
        emit!(w, "        name: FlatName {{ off: {off}, len: {len} }},");
        emit!(
            w,
            "        offset: std::mem::offset_of!({}, {}) as u16,",
            row.owner,
            row.rust_field
        );
        emit!(w, "        type_index: {},", row.tidx_str);
        emit!(w, "        enc: FlatEnc::{} as u8,", row.enc);
        emit!(w, "    }},");
    }
    emit!(w, "];");
    emit!(w, "");

    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "static CTORS: &[FlatCtorInfo] = &[");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if !matches!(info.kind, InfoKind::SimpleSum | InfoKind::CompoundSum) {
            continue;
        }
        for (tag, c) in info.constructors.iter().enumerate() {
            let (off, len) = pool.intern(&camel(&c.name))?;
            let psec = layout
                .payload_section
                .get(&(info.name.clone(), tag))
                .map_or_else(|| "NONE_U16".to_string(), |sec| sec.to_string());
            let (foff, fcnt) = ctor_field_range
                .get(&(info.name.clone(), tag))
                .copied()
                .unwrap_or((0, 0));
            emit!(w, "    FlatCtorInfo {{");
            emit!(w, "        name: FlatName {{ off: {off}, len: {len} }},");
            emit!(w, "        tag: {tag},");
            emit!(w, "        type_index: {},", info.type_index.unwrap());
            emit!(w, "        payload_section: {psec},");
            emit!(w, "        field_off: {foff},");
            emit!(w, "        field_count: {fcnt},");
            emit!(w, "    }},");
        }
    }
    emit!(w, "];");
    emit!(w, "");

    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "static TYPES: &[FlatTypeInfo] = &[");
    for t in &layout.types {
        let (off, len) = pool.intern(t)?;
        let (kind, enc, node_section, seq_section, ctor_off, ctor_count, foff, fcnt) =
            if BUILTIN_TYPES.contains(&t.as_str()) {
                let kind = "Builtin";
                let enc = builtin_enc(t).unwrap();
                let node_section = "NONE_U16".to_string();
                let seq_section = layout
                    .seq_pool_section
                    .get(t)
                    .map_or_else(|| "NONE_U16".to_string(), |idx| idx.to_string());
                (
                    kind,
                    enc,
                    node_section,
                    seq_section,
                    "0".to_string(),
                    "0".to_string(),
                    0u16,
                    0u16,
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
                    .map_or_else(|| "NONE_U16".to_string(), |idx| idx.to_string());
                let seq_section = info
                    .seq_section
                    .map_or_else(|| "NONE_U16".to_string(), |idx| idx.to_string());
                let (ctor_off, ctor_count) =
                    if matches!(info.kind, InfoKind::SimpleSum | InfoKind::CompoundSum) {
                        (info.ctor_off.unwrap().to_string(), info.constructors.len().to_string())
                    } else {
                        ("0".to_string(), "0".to_string())
                    };
                let (foff, fcnt) = if info.kind == InfoKind::Product {
                    product_field_range[&info.name]
                } else {
                    (0, 0)
                };
                (kind, enc, node_section, seq_section, ctor_off, ctor_count, foff, fcnt)
            };
        emit!(w, "    FlatTypeInfo {{");
        emit!(w, "        name: FlatName {{ off: {off}, len: {len} }},");
        emit!(w, "        kind: FlatTypeKind::{kind} as u8,");
        emit!(w, "        enc: FlatEnc::{enc} as u8,");
        emit!(w, "        node_section: {node_section},");
        emit!(w, "        seq_section: {seq_section},");
        emit!(w, "        ctor_off: {ctor_off},");
        emit!(w, "        ctor_count: {ctor_count},");
        emit!(w, "        field_off: {foff},");
        emit!(w, "        field_count: {fcnt},");
        emit!(w, "    }},");
    }
    emit!(w, "];");
    emit!(w, "");

    let Some(&root_type) = layout.type_index.get("file") else {
        return Err(CodegenError("flat layout requires a 'file' root type".to_string()));
    };

    emit!(w, "/// The schema of the sql.asdl this build was generated from.");
    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "pub static SCHEMA: FlatSchema = FlatSchema {{");
    emit!(w, "    runtime_abi_version: RUNTIME_ABI_VERSION,");
    emit!(w, "    abi_version: ABI_VERSION,");
    emit!(w, "    schema_hash: SCHEMA_HASH,");
    emit!(w, "    strings_len: SCHEMA_STRINGS.len() as u32,");
    emit!(w, "    type_count: TYPES.len() as u16,");
    emit!(w, "    ctor_count: CTORS.len() as u16,");
    emit!(w, "    field_count: FIELDS.len() as u16,");
    emit!(w, "    root_type: {root_type},");
    emit!(w, "    strings: SCHEMA_STRINGS.as_ptr(),");
    emit!(w, "    types: TYPES.as_ptr(),");
    emit!(w, "    ctors: CTORS.as_ptr(),");
    emit!(w, "    fields: FIELDS.as_ptr(),");
    emit!(w, "}};");
    emit!(w, "");

    emit!(w, "/// The type indices of this build's schema: host code resolves types");
    emit!(w, "/// by name through the descriptor; these constants serve the library's");
    emit!(w, "/// own builder and tests.");
    emit!(w, "#[cfg(feature = \"builder\")]");
    emit!(w, "pub mod ty {{");
    for t in &layout.types {
        emit!(w, "    pub const {}: u16 = {};", t.to_uppercase(), layout.type_index[t]);
    }
    emit!(w, "}}");
    emit!(w, "");
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
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "// The @stable typed projections");
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "");

    for info in &stable {
        if info.kind != InfoKind::CompoundSum {
            continue;
        }
        let r = &info.rust;
        emit!(w, "/// A @stable `{}` node: the typed projection over the", info.name);
        emit!(w, "/// frozen layout.");
        emit!(w, "#[derive(Clone, Copy)]");
        emit!(w, "pub struct Stable{r}<'t> {{");
        emit!(w, "    node: Node<'t>,");
        emit!(w, "}}");
        emit!(w, "");
        emit!(w, "impl<'t> Stable{r}<'t> {{");
        emit!(w, "    /// The generic cursor, for ports and introspection.");
        emit!(w, "    pub fn node(self) -> Node<'t> {{");
        emit!(w, "        self.node");
        emit!(w, "    }}");
        emit!(w, "");
        emit!(w, "    /// The constructor tag.");
        emit!(w, "    pub fn tag(self) -> Option<{r}Tag> {{");
        emit!(w, "        let tree = self.node.tree();");
        emit!(w, "        let ty = tree.type_by_name(\"{}\")?;", info.name);
        emit!(w, "        {r}Tag::from_u16(tree.sum_tag(ty, self.node.index())?)");
        emit!(w, "    }}");
        for c in &info.constructors {
            if !info.ctor_has_record(c) {
                continue;
            }
            let cname = camel(&c.name);
            emit!(w, "");
            emit!(w, "    /// The payload as `{cname}`, when that is the constructor.");
            emit!(
                w,
                "    pub fn as_{}(self) -> Option<&'t {}::{cname}> {{",
                uncamel(&cname),
                info.name
            );
            emit!(w, "        let tree = self.node.tree();");
            emit!(w, "        let ty = tree.type_by_name(\"{}\")?;", info.name);
            emit!(w, "        let (ptr, stride) =");
            emit!(
                w,
                "            tree.payload_ptr(ty, self.node.index(), {r}Tag::{cname} as u16)?;"
            );
            emit!(w, "        if stride != std::mem::size_of::<{}::{cname}>() {{", info.name);
            emit!(w, "            return None;");
            emit!(w, "        }}");
            emit!(w, "        Some(unsafe {{ &*(ptr as *const {}::{cname}) }})", info.name);
            emit!(w, "    }}");
        }
        emit!(w, "}}");
        emit!(w, "");
    }

    let projectable: Vec<&&TypeInfo> = stable
        .iter()
        .filter(|info| matches!(info.kind, InfoKind::Product | InfoKind::CompoundSum))
        .collect();
    if !projectable.is_empty() {
        emit!(w, "/// Typed `@stable` projections on [`Node`].");
        emit!(w, "pub trait StableNodeExt<'t>: Copy {{");
        for (i, info) in projectable.iter().enumerate() {
            if i > 0 {
                emit!(w, "");
            }
            let r = &info.rust;
            match info.kind {
                InfoKind::Product => {
                    emit!(w, "    /// This node as the @stable `{}` record, when it IS", info.name);
                    emit!(w, "    /// a `{}`. The record shape is guaranteed by the", info.name);
                    emit!(w, "    /// load-time `abi_version` handshake.");
                    emit!(w, "    fn as_{}(self) -> Option<&'t {r}>;", info.name);
                }
                InfoKind::CompoundSum => {
                    emit!(w, "    /// This node as a @stable `{}`, when it IS one.", info.name);
                    emit!(w, "    /// The record shape is guaranteed by the load-time");
                    emit!(w, "    /// `abi_version` handshake.");
                    emit!(w, "    fn as_{}(self) -> Option<Stable{r}<'t>>;", info.name);
                }
                InfoKind::SimpleSum => {}
            }
        }
        emit!(w, "}}");
        emit!(w, "");
        emit!(w, "impl<'t> StableNodeExt<'t> for Node<'t> {{");
        for (i, info) in projectable.iter().enumerate() {
            if i > 0 {
                emit!(w, "");
            }
            let r = &info.rust;
            match info.kind {
                InfoKind::Product => {
                    emit!(w, "    fn as_{}(self) -> Option<&'t {r}> {{", info.name);
                    emit!(w, "        let tree = self.tree();");
                    emit!(w, "        let ty = tree.type_by_name(\"{}\")?;", info.name);
                    emit!(w, "        if ty != self.type_index() {{");
                    emit!(w, "            return None;");
                    emit!(w, "        }}");
                    emit!(w, "        let (ptr, stride) = tree.product_ptr(ty, self.index())?;");
                    emit!(w, "        if stride != std::mem::size_of::<{r}>() {{");
                    emit!(w, "            return None;");
                    emit!(w, "        }}");
                    emit!(w, "        Some(unsafe {{ &*(ptr as *const {r}) }})");
                    emit!(w, "    }}");
                }
                InfoKind::CompoundSum => {
                    emit!(w, "    fn as_{}(self) -> Option<Stable{r}<'t>> {{", info.name);
                    emit!(w, "        let tree = self.tree();");
                    emit!(
                        w,
                        "        if tree.type_by_name(\"{}\") != Some(self.type_index()) {{",
                        info.name
                    );
                    emit!(w, "            return None;");
                    emit!(w, "        }}");
                    emit!(w, "        Some(Stable{r} {{ node: self }})");
                    emit!(w, "    }}");
                }
                InfoKind::SimpleSum => {}
            }
        }
        emit!(w, "}}");
        emit!(w, "");
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
            _ => format!("{e}.map(|x| lower_{t}({b}, x)).unwrap_or({}Ref::NONE)", camel(t)),
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
            emit!(
                lines,
                "    writeln!(out, \"{indent}{fname}: {label} @{{}}\", std::mem::offset_of!({rec}, {rf})).unwrap();"
            );
        }
        lines
    }

    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "// The @stable freeze dump");
    emit!(w, "// ------------------------------------------------------------------");
    emit!(w, "");
    emit!(w, "/// A canonical text dump of the `@stable` region's layout: record");
    emit!(w, "/// sizes/alignments, field offsets and types, and variant");
    emit!(w, "/// discriminants. `tests/freeze.rs` byte-compares it against the");
    emit!(w, "/// checked-in snapshot `sql-abi/stable_layout.txt`; regenerating a");
    emit!(w, "/// changed layout requires an `ABI_VERSION` bump.");
    emit!(w, "pub fn freeze_stable_layout() -> String {{");
    emit!(w, "    let mut out = String::new();");
    emit!(w, "    use std::fmt::Write as _;");
    emit!(w, "    writeln!(out, \"abi_version: {{}}\", ABI_VERSION).unwrap();");

    for info in stable {
        let r = &info.rust;
        emit!(w, "    writeln!(out).unwrap();");
        match info.kind {
            InfoKind::Product => {
                emit!(w, "    writeln!(out, \"type {}: product size {{}} align {{}}\",", info.name);
                emit!(
                    w,
                    "        std::mem::size_of::<{r}>(), std::mem::align_of::<{r}>()).unwrap();"
                );
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
                emit!(w, "    writeln!(out, \"type {}: sum\").unwrap();", info.name);
                for (tag, c) in info.constructors.iter().enumerate() {
                    let cname = camel(&c.name);
                    if info.ctor_has_record(c) {
                        let rec = format!("{}::{cname}", info.name);
                        emit!(
                            w,
                            "    writeln!(out, \"    variant {tag}: {cname} size {{}} align {{}}\","
                        );
                        emit!(
                            w,
                            "        std::mem::size_of::<{rec}>(), std::mem::align_of::<{rec}>()).unwrap();"
                        );
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
                        emit!(
                            w,
                            "    writeln!(out, \"    variant {tag}: {cname} (bare)\").unwrap();"
                        );
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
        emit!(w, "    writeln!(out).unwrap();");
        emit!(
            w,
            "    writeln!(out, \"type {}: simple_sum (referenced by a stable field)\").unwrap();",
            d.name
        );
        for (tag, c) in info.constructors.iter().enumerate() {
            emit!(w, "    writeln!(out, \"    variant {tag}: {}\").unwrap();", camel(&c.name));
        }
    }

    emit!(w, "    out");
    emit!(w, "}}");
    emit!(w, "");
}

fn emit_convert(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    emit!(w, "use crate::*;");
    emit!(w, "#[allow(unused_imports)]");
    emit!(w, "use dbt_asdl::*;");
    emit!(w, "");
    emit!(w, "/// Deep-convert a finished arena/owned tree (behind the read API) into");
    emit!(w, "/// the flat boundary representation: one O(nodes) pass, children");
    emit!(w, "/// first. Dialect libraries that lower into the arena tree call this");
    emit!(w, "/// at the boundary.");
    emit!(w, "pub fn convert_file(root: ast::FileRef) -> *mut FlatTree {{");
    emit!(w, "    let mut b = TreeBuilder::new();");
    emit!(w, "    let root = lower_file(&mut b, root);");
    emit!(w, "    b.finish(root)");
    emit!(w, "}}");

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::SimpleSum {
            continue;
        }
        emit!(w, "");
        if info.kind == InfoKind::Product {
            let arg = if info.shared {
                format!("ast::{}", info.rust)
            } else {
                format!("ast::{}Ref", info.rust)
            };
            emit!(
                w,
                "fn lower_{}(b: &mut TreeBuilder, r: {arg}) -> {}Ref {{",
                info.name,
                info.rust
            );
            emit!(w, "    let r = {} {{", info.rust);
            for f in info.record_fields(None) {
                let fname = f.name.as_deref().unwrap();
                let rf = rust_field(fname);
                let e = format!("r.{rf}()");
                emit!(w, "        {rf}: {},", convert_expr(&f, infos, &e, "b"));
            }
            emit!(w, "    }};");
            emit!(w, "    b.push_{}(r)", info.name);
            emit!(w, "}}");
        } else {
            let r = &info.rust;
            emit!(w, "fn lower_{}(b: &mut TreeBuilder, r: ast::{r}Ref) -> {r}Ref {{", info.name);
            emit!(w, "    match r.view() {{");
            for c in &info.constructors {
                let cname = camel(&c.name);
                if info.attributes.is_empty() && c.fields.is_empty() {
                    emit!(
                        w,
                        "        ast::{r}View::{cname} => b.push_{}_{}(),",
                        info.name,
                        uncamel(&c.name)
                    );
                    continue;
                }
                emit!(w, "        ast::{r}View::{cname}(v) => {{");
                emit!(w, "            let r = {}::{cname} {{", info.name);
                for f in info.record_fields(Some(c)) {
                    let fname = f.name.as_deref().unwrap();
                    let rf = rust_field(fname);
                    let e = format!("v.{rf}()");
                    emit!(w, "                {rf}: {},", convert_expr(&f, infos, &e, "b"));
                }
                emit!(w, "            }};");
                emit!(w, "            b.push_{}_{}(r)", info.name, uncamel(&c.name));
                emit!(w, "        }}");
            }
            emit!(w, "    }}");
            emit!(w, "}}");
        }
    }
    emit!(w, "");
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
    emit!(flat, "");
    emit!(flat, "use dbt_asdl::*;");
    emit!(flat, "");
    emit!(flat, "/// ABI version of the `@stable` region from the asdl specification.");
    emit!(flat, "pub const ABI_VERSION: u32 = {};", mod_node.version);
    emit!(flat, "");
    emit!(flat, "/// FNV-1a/64 of the asdl source bytes this build was generated from.");
    emit!(flat, "/// Library-side: the host reads the hash from the descriptor.");
    emit!(flat, "#[cfg(feature = \"builder\")]");
    emit!(flat, "pub const SCHEMA_HASH: u64 = 0x{schema_hash:016x};");
    emit!(flat, "");

    emit_ref_newtypes(&mut flat, mod_node, &infos);
    emit_tag_enums(&mut flat, mod_node, &infos)?;
    emit_records(&mut flat, mod_node, &infos);
    emit_builder(&mut flat, mod_node, &infos, &layout);
    emit_stable(&mut flat, mod_node, &infos);
    emit_freeze(&mut flat, mod_node, &infos);
    emit_schema(&mut flat, mod_node, &infos, &layout)?;

    conv.push(header);
    emit!(conv, "");
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
/// - `dst` (for example, `gen_flat.rs`): `#[repr(C)]` record structs, typed node
///   reference newtypes, constructor tag enums, `TreeBuilder`, `@stable` typed
///   projections, layout freeze dump, and schema descriptor tables.
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

fn cpp_field(name: &str) -> String {
    match name {
        "alignas" | "alignof" | "and" | "and_eq" | "asm" | "auto" | "bitand" | "bitor" | "bool"
        | "break" | "case" | "catch" | "char" | "char8_t" | "char16_t" | "char32_t" | "class"
        | "compl" | "concept" | "const" | "consteval" | "constexpr" | "constinit"
        | "const_cast" | "continue" | "co_await" | "co_return" | "co_yield" | "decltype"
        | "default" | "delete" | "do" | "double" | "dynamic_cast" | "else" | "enum"
        | "explicit" | "export" | "extern" | "false" | "float" | "for" | "friend" | "goto"
        | "if" | "inline" | "int" | "long" | "mutable" | "namespace" | "new" | "noexcept"
        | "not" | "not_eq" | "nullptr" | "operator" | "or" | "or_eq" | "private" | "protected"
        | "public" | "register" | "reinterpret_cast" | "requires" | "return" | "short"
        | "signed" | "sizeof" | "static" | "static_assert" | "static_cast" | "struct"
        | "switch" | "template" | "this" | "thread_local" | "throw" | "true" | "try"
        | "typedef" | "typeid" | "typename" | "union" | "unsigned" | "using" | "virtual"
        | "void" | "volatile" | "wchar_t" | "while" | "xor" | "xor_eq" => {
            format!("{name}_")
        }
        _ => name.to_string(),
    }
}

fn cpp_record_ty(f: &Field, infos: &HashMap<String, TypeInfo>) -> String {
    let t = f.ty.as_str();
    if f.seq {
        return "dbt_asdl::FlatSeq".to_string();
    }
    if f.opt {
        return match t {
            "bool" => "dbt_asdl::OptBool".to_string(),
            "byte" => "dbt_asdl::OptByte".to_string(),
            "int" => "dbt_asdl::OptInt".to_string(),
            "long" => "dbt_asdl::OptLong".to_string(),
            "identifier" | "string" => "dbt_asdl::FlatStr".to_string(),
            _ if infos[t].kind == InfoKind::SimpleSum => "uint8_t".to_string(),
            _ => format!("{}Ref", camel(t)),
        };
    }
    match t {
        "bool" | "byte" => "uint8_t".to_string(),
        "int" => "int32_t".to_string(),
        "long" => "int64_t".to_string(),
        "identifier" | "string" => "dbt_asdl::FlatStr".to_string(),
        _ if infos[t].kind == InfoKind::SimpleSum => "uint8_t".to_string(),
        _ => format!("{}Ref", camel(t)),
    }
}

fn emit_cpp_ref_newtypes(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
) {
    emit!(w, "// Typed node references for products and compound sums.");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if !matches!(info.kind, InfoKind::Product | InfoKind::CompoundSum) {
            continue;
        }
        let r = &info.rust;
        emit!(w, "");
        emit!(w, "struct {r}Ref {{");
        emit!(w, "    uint32_t value;");
        emit!(w, "");
        emit!(w, "    static const {r}Ref NONE;");
        emit!(w, "");
        emit!(
            w,
            "    constexpr bool is_none() const noexcept {{ return value == dbt_asdl::NONE_U32; }}"
        );
        emit!(w, "");
        emit!(w, "    constexpr bool operator==(const {r}Ref& other) const noexcept {{");
        emit!(w, "        return value == other.value;");
        emit!(w, "    }}");
        emit!(w, "    constexpr bool operator!=(const {r}Ref& other) const noexcept {{");
        emit!(w, "        return value != other.value;");
        emit!(w, "    }}");
        emit!(
            w,
            "    constexpr bool operator==(uint32_t raw) const noexcept {{ return value == raw; }}"
        );
        emit!(
            w,
            "    constexpr bool operator!=(uint32_t raw) const noexcept {{ return value != raw; }}"
        );
        emit!(w, "}};");
        emit!(w, "");
        emit!(w, "inline constexpr {r}Ref {r}Ref::NONE{{dbt_asdl::NONE_U32}};");
    }
    emit!(w, "");
}

fn emit_cpp_enums(
    w: &mut Vec<String>,
    mod_node: &Module,
    infos: &HashMap<String, TypeInfo>,
) -> Result<(), CodegenError> {
    emit!(w, "// Simple sum enums and compound sum constructor tags.");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        match info.kind {
            InfoKind::Product => continue,
            InfoKind::SimpleSum => {
                if info.constructors.len() > 0xFF {
                    return Err(CodegenError(format!(
                        "{}: a simple sum with > 255 constructors does not fit the u8 encoding",
                        info.name
                    )));
                }
                let r = &info.rust;
                emit!(w, "");
                emit!(w, "enum class {r} : uint8_t {{");
                for (tag, c) in info.constructors.iter().enumerate() {
                    emit!(w, "    {} = {tag},", camel(&c.name));
                }
                emit!(w, "}};");
                emit!(w, "");
                emit!(
                    w,
                    "constexpr std::optional<{r}> {}_from_u8(uint8_t v) noexcept {{",
                    info.name
                );
                emit!(w, "    switch (v) {{");
                for (tag, c) in info.constructors.iter().enumerate() {
                    emit!(w, "        case {tag}: return {r}::{};", camel(&c.name));
                }
                emit!(w, "        default: return std::nullopt;");
                emit!(w, "    }}");
                emit!(w, "}}");
            }
            InfoKind::CompoundSum => {
                if info.constructors.len() > 0x100 {
                    return Err(CodegenError(format!(
                        "{}: too many constructors for an 8-bit packed tag",
                        info.name
                    )));
                }
                let r = &info.rust;
                emit!(w, "");
                emit!(w, "enum class {r}Tag : uint16_t {{");
                for (tag, c) in info.constructors.iter().enumerate() {
                    emit!(w, "    {} = {tag},", camel(&c.name));
                }
                emit!(w, "}};");
                emit!(w, "");
                emit!(
                    w,
                    "constexpr std::optional<{r}Tag> {}_tag_from_u16(uint16_t v) noexcept {{",
                    info.name
                );
                emit!(w, "    switch (v) {{");
                for (tag, c) in info.constructors.iter().enumerate() {
                    emit!(w, "        case {tag}: return {r}Tag::{};", camel(&c.name));
                }
                emit!(w, "        default: return std::nullopt;");
                emit!(w, "    }}");
                emit!(w, "}}");
            }
        }
    }
    emit!(w, "");
    Ok(())
}

fn emit_cpp_records(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    emit!(w, "// C-compatible record structs for products and constructor payloads.");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind == InfoKind::Product {
            emit!(w, "");
            emit!(w, "struct {} {{", info.rust);
            for f in info.record_fields(None) {
                let fname = f.name.as_deref().unwrap();
                emit!(w, "    {} {};", cpp_record_ty(&f, infos), cpp_field(fname));
            }
            emit!(w, "}};");
        } else if info.kind == InfoKind::CompoundSum {
            if !info.constructors.iter().any(|c| info.ctor_has_record(c)) {
                continue;
            }
            let ns = cpp_field(&info.name);
            emit!(w, "");
            emit!(w, "namespace {ns} {{");
            for c in &info.constructors {
                if !info.ctor_has_record(c) {
                    continue;
                }
                emit!(w, "");
                emit!(w, "struct {} {{", camel(&c.name));
                for f in info.record_fields(Some(c)) {
                    let fname = f.name.as_deref().unwrap();
                    emit!(w, "    {} {};", cpp_record_ty(&f, infos), cpp_field(fname));
                }
                emit!(w, "}};");
            }
            emit!(w, "");
            emit!(w, "}} // namespace {ns}");
        }
    }
    emit!(w, "");
}

fn emit_cpp_views(w: &mut Vec<String>, mod_node: &Module, infos: &HashMap<String, TypeInfo>) {
    emit!(w, "// Typed views and projections over dbt_asdl::Node and dbt_asdl::TreeView.");
    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        if info.kind != InfoKind::CompoundSum {
            continue;
        }
        let r = &info.rust;
        let ns = cpp_field(&info.name);
        emit!(w, "");
        emit!(w, "class {r}View {{");
        emit!(w, "public:");
        emit!(
            w,
            "    constexpr explicit {r}View(dbt_asdl::Node node) noexcept : node_(node) {{}}"
        );
        emit!(w, "");
        emit!(w, "    constexpr dbt_asdl::Node node() const noexcept {{ return node_; }}");
        emit!(w, "");
        emit!(w, "    std::optional<{r}Tag> tag() const noexcept {{");
        emit!(w, "        auto tree = node_.tree();");
        emit!(w, "        auto ty = tree.type_by_name(\"{}\");", info.name);
        emit!(w, "        if (!ty) {{");
        emit!(w, "            return std::nullopt;");
        emit!(w, "        }}");
        emit!(w, "        auto raw_tag = tree.sum_tag(*ty, node_.index());");
        emit!(w, "        if (!raw_tag) {{");
        emit!(w, "            return std::nullopt;");
        emit!(w, "        }}");
        emit!(w, "        return {}_tag_from_u16(*raw_tag);", info.name);
        emit!(w, "    }}");
        for c in &info.constructors {
            if !info.ctor_has_record(c) {
                continue;
            }
            let cname = camel(&c.name);
            let uname = uncamel(&cname);
            emit!(w, "");
            emit!(w, "    const {ns}::{cname}* as_{uname}() const noexcept {{");
            emit!(
                w,
                "        return node_.as_payload<{ns}::{cname}>(\"{}\", static_cast<uint16_t>({r}Tag::{cname}));",
                info.name
            );
            emit!(w, "    }}");
        }
        emit!(w, "");
        emit!(w, "private:");
        emit!(w, "    dbt_asdl::Node node_;");
        emit!(w, "}};");
    }

    for d in &mod_node.dfns {
        let info = &infos[&d.name];
        let r = &info.rust;
        match info.kind {
            InfoKind::Product => {
                emit!(w, "");
                emit!(w, "inline const {r}* as_{}(dbt_asdl::Node node) noexcept {{", info.name);
                emit!(w, "    return node.as_product<{r}>(\"{}\");", info.name);
                emit!(w, "}}");
                emit!(w, "");
                emit!(
                    w,
                    "inline const {r}* as_{}(dbt_asdl::TreeView tree, {r}Ref ref) noexcept {{",
                    info.name
                );
                emit!(w, "    if (ref.is_none()) {{");
                emit!(w, "        return nullptr;");
                emit!(w, "    }}");
                emit!(w, "    auto ty = tree.type_by_name(\"{}\");", info.name);
                emit!(w, "    if (!ty) {{");
                emit!(w, "        return nullptr;");
                emit!(w, "    }}");
                emit!(w, "    return tree.product<{r}>(*ty, ref.value);");
                emit!(w, "}}");
            }
            InfoKind::CompoundSum => {
                emit!(w, "");
                emit!(
                    w,
                    "inline std::optional<{r}View> as_{}(dbt_asdl::Node node) noexcept {{",
                    info.name
                );
                emit!(
                    w,
                    "    if (node.tree().type_by_name(\"{}\") != node.type_index()) {{",
                    info.name
                );
                emit!(w, "        return std::nullopt;");
                emit!(w, "    }}");
                emit!(w, "    return {r}View(node);");
                emit!(w, "}}");
                emit!(w, "");
                emit!(
                    w,
                    "inline std::optional<{r}View> as_{}(dbt_asdl::TreeView tree, {r}Ref ref) noexcept {{",
                    info.name
                );
                emit!(w, "    if (ref.is_none()) {{");
                emit!(w, "        return std::nullopt;");
                emit!(w, "    }}");
                emit!(w, "    auto ty = tree.type_by_name(\"{}\");", info.name);
                emit!(w, "    if (!ty || !tree.sum_tag(*ty, ref.value)) {{");
                emit!(w, "        return std::nullopt;");
                emit!(w, "    }}");
                emit!(w, "    return {r}View(dbt_asdl::Node(tree.raw(), *ty, ref.value));");
                emit!(w, "}}");
            }
            InfoKind::SimpleSum => {}
        }
    }
    emit!(w, "");
}

pub(crate) fn generate_cpp_flat_from_module(
    mod_node: &Module,
    input_name: &str,
    schema_hash: u64,
) -> Result<String, CodegenError> {
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
    let _layout = Layout::build(mod_node, &mut infos)?;
    if !infos.contains_key("file") {
        return Err(CodegenError("flat layout requires a 'file' root type".to_string()));
    }

    let mod_snake = uncamel(&mod_node.name);
    let ns = cpp_field(&mod_snake);
    let guard = format!("DBT_ASDL_{}_FLAT_HPP", mod_snake.to_uppercase());

    let mut w = Vec::new();
    emit!(w, "// File automatically generated by dbt-asdl from {input_name}. Do not edit.");
    emit!(w, "");
    emit!(w, "#ifndef {guard}");
    emit!(w, "#define {guard}");
    emit!(w, "");
    emit!(w, "#include <dbt_asdl/flat_runtime.hpp>");
    emit!(w, "");
    emit!(w, "namespace {ns} {{");
    emit!(w, "");
    emit!(w, "inline constexpr uint32_t ABI_VERSION = {};", mod_node.version);
    emit!(w, "inline constexpr uint64_t SCHEMA_HASH = 0x{schema_hash:016x}ULL;");
    emit!(w, "");

    emit_cpp_ref_newtypes(&mut w, mod_node, &infos);
    emit_cpp_enums(&mut w, mod_node, &infos)?;
    emit_cpp_records(&mut w, mod_node, &infos);
    emit_cpp_views(&mut w, mod_node, &infos);

    emit!(w, "}} // namespace {ns}");
    emit!(w, "");
    emit!(w, "#endif // {guard}");

    Ok(format!("{}\n", w.join("\n")))
}

/// Generates the C++17 flat AST header from an ASDL file.
///
/// The generator reads the ASDL specification at `src` and writes the C++17
/// header to `dst`. The header defines node reference structs, enum tags,
/// record structs, and typed view projections for all types in the
/// specification.
///
/// # Errors
///
/// If the generator cannot read `src` or write `dst`, this function returns an
/// I/O error. If the ASDL specification is not valid, this function returns a
/// syntax or code-generation error.
///
/// # Examples
///
/// ```
/// use std::fs;
///
/// let dir = std::env::temp_dir().join("dbt_asdl_cpp_flat_doctest");
/// fs::create_dir_all(&dir)?;
///
/// let spec = dir.join("sql.asdl");
/// fs::write(
///     &spec,
///     "module Sql { expr = Lit(int value) | Add(expr left, expr right)\n file = (expr* exprs) }",
/// )?;
///
/// let files = dbt_asdl::codegen::cpp::generate_flat(&spec, dir.join("gen_flat.hpp"))?;
/// assert_eq!(files, vec![dir.join("gen_flat.hpp")]);
/// assert!(dir.join("gen_flat.hpp").is_file());
///
/// fs::remove_dir_all(&dir)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn generate_cpp_flat(
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
    let generated = generate_cpp_flat_from_module(&mod_node, input_name, schema_hash)?;

    if let Some(parent) = dst.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(dst, &generated)?;

    Ok(vec![dst.to_path_buf()])
}
