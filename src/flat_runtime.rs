use std::fmt::Write as _;

/// Layout version of the boundary: the descriptor and tree-record layout in
/// this file, plus the `@stable` region's record layouts. A host supports a
/// range of versions (backwards compatible, never forwards); a library
/// outside the range is refused at load time, before any table is read.
/// `@stable` is strict-only — the typed projections are integral to host
/// operation — so a change to a `@stable` record's shape bumps this and
/// raises the supported floor with it: within the supported range the
/// stable records are byte-identical.
pub const ABI_VERSION: u32 = 1;
/// The oldest ABI version this build can read.
pub const ABI_VERSION_MIN: u32 = 1;

/// The None sentinel for optional node references and optional string
/// offsets.
pub const NONE_U32: u32 = u32::MAX;
/// The None sentinel for optional simple-sum values.
pub const NONE_U8: u8 = u8::MAX;

/// By convention, section 0 of every tree is the UTF-8 string blob.
pub const SECTION_STRINGS: u32 = 0;

// ---------------------------------------------------------------------------
// Scalar and small-record boundary types
// ---------------------------------------------------------------------------

/// A string in the tree's UTF-8 blob, as `{off, len}`. For optional strings,
/// `off == NONE_U32` is None; an empty string is `{0, 0}`, distinct from None.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SqlStr {
    pub off: u32,
    pub len: u32,
}

impl SqlStr {
    pub const NONE: Self = SqlStr { off: NONE_U32, len: 0 };

    pub fn is_none(self) -> bool {
        self.off == NONE_U32
    }
}

/// A sequence field: `{off, len}` elements in the element type's pool
/// section.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SqlSeq {
    pub off: u32,
    pub len: u32,
}

/// Optional scalars: explicit tags, no niche tricks (the layout is defined
/// by the platform C ABI, and niche encoding is rustc-internal).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptBool {
    pub is_some: u32,
    pub value: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptByte {
    pub is_some: u32,
    pub value: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptInt {
    pub is_some: u32,
    pub value: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptLong {
    pub is_some: u32,
    pub value: i64,
}

macro_rules! opt_scalar {
    ($t:ident, $v:ty) => {
        impl $t {
            pub const NONE: Self = $t { is_some: 0, value: 0 };

            pub fn get(self) -> Option<$v> {
                (self.is_some != 0).then_some(self.value)
            }
        }

        impl From<Option<$v>> for $t {
            fn from(v: Option<$v>) -> Self {
                match v {
                    Some(value) => $t { is_some: 1, value },
                    None => $t::NONE,
                }
            }
        }
    };
}

opt_scalar!(OptByte, u8);
opt_scalar!(OptInt, i32);
opt_scalar!(OptLong, i64);

impl OptBool {
    pub const NONE: Self = OptBool { is_some: 0, value: 0 };

    pub fn get(self) -> Option<bool> {
        (self.is_some != 0).then_some(self.value != 0)
    }
}

impl From<Option<bool>> for OptBool {
    fn from(v: Option<bool>) -> Self {
        match v {
            Some(value) => OptBool { is_some: 1, value: value as u8 },
            None => OptBool::NONE,
        }
    }
}

/// One node of a compound sum: the constructor tag (index into the sum's
/// constructors, in `sql.asdl` declaration order) and the index of the
/// payload record in that constructor's section (`NONE_U32` for a bare
/// constructor of an attribute-less sum).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SqlNode {
    pub tag: u16,
    pub _pad: u16,
    pub payload: u32,
}

// ---------------------------------------------------------------------------
// The descriptor types: the dialect library's metadata and the schema-as-data
// the host's introspection reads. `abi_version` is the first field of both
// SqlDialect and SqlTree, readable without knowing the rest of the layout.
// ---------------------------------------------------------------------------

/// A name (or any static string) held by a descriptor: a pointer into the
/// library's rodata plus its length. Thin pointer + length by hand: fat
/// pointers never cross the boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlName {
    pub ptr: *const u8,
    pub len: u32,
}

impl SqlName {
    pub const fn from_static(s: &'static str) -> Self {
        SqlName { ptr: s.as_ptr(), len: s.len() as u32 }
    }

    /// The name as a str. Safe to call on any descriptor the handshake
    /// accepted: the pointed-to memory is the library's static rodata, and
    /// libraries are never unloaded.
    pub fn as_str<'t>(self) -> &'t str {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(self.ptr, self.len as usize))
        }
    }
}

/// The field encodings: how one field's bytes at `record + offset` read.
/// Stored as `u8` in the tables and converted through [`SqlEnc::from_u8`]
/// so an unrecognized value is data, not UB.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlEnc {
    Bool = 0,
    Byte = 1,
    Int = 2,
    Long = 3,
    /// A `u8` discriminant of the simple sum `type_index`.
    SimpleSum = 4,
    /// A `SqlStr`.
    Str = 5,
    /// A `u32` node index of type `type_index`.
    Node = 6,
    OptBool = 7,
    OptByte = 8,
    OptInt = 9,
    OptLong = 10,
    /// A `u8` discriminant, `NONE_U8` for None.
    OptSimpleSum = 11,
    /// A `SqlStr`, `off == NONE_U32` for None.
    OptStr = 12,
    /// A `u32` node index of type `type_index`, `NONE_U32` for None.
    OptNode = 13,
    /// A `SqlSeq` of the element type `type_index`.
    Seq = 14,
}

impl SqlEnc {
    pub fn from_u8(v: u8) -> Option<SqlEnc> {
        Some(match v {
            0 => SqlEnc::Bool,
            1 => SqlEnc::Byte,
            2 => SqlEnc::Int,
            3 => SqlEnc::Long,
            4 => SqlEnc::SimpleSum,
            5 => SqlEnc::Str,
            6 => SqlEnc::Node,
            7 => SqlEnc::OptBool,
            8 => SqlEnc::OptByte,
            9 => SqlEnc::OptInt,
            10 => SqlEnc::OptLong,
            11 => SqlEnc::OptSimpleSum,
            12 => SqlEnc::OptStr,
            13 => SqlEnc::OptNode,
            14 => SqlEnc::Seq,
            _ => return None,
        })
    }
}

/// The kind of a schema type, stored as `u8` like [`SqlEnc`].
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlTypeKind {
    /// A builtin scalar or text type: `bool`, `byte`, `int`, `long`,
    /// `identifier`, `string`.
    Builtin = 0,
    /// A sum whose constructors are all fieldless.
    SimpleSum = 1,
    /// A product type.
    Product = 2,
    /// A sum with at least one payload-carrying constructor.
    Sum = 3,
}

impl SqlTypeKind {
    pub fn from_u8(v: u8) -> Option<SqlTypeKind> {
        Some(match v {
            0 => SqlTypeKind::Builtin,
            1 => SqlTypeKind::SimpleSum,
            2 => SqlTypeKind::Product,
            3 => SqlTypeKind::Sum,
            _ => return None,
        })
    }
}

/// One field of a constructor payload or product record.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlFieldInfo {
    pub name: SqlName,
    /// Byte offset of the field within its record.
    pub offset: u32,
    /// A `SqlEnc` value.
    pub enc: u8,
    /// The referenced/element type, for `SimpleSum`/`Node`/`OptNode`/`Seq`/
    /// `OptSimpleSum` encodings; `u16::MAX` otherwise.
    pub type_index: u16,
}

/// One constructor of a sum (compound or simple).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlCtorInfo {
    pub name: SqlName,
    /// The tag value within its sum.
    pub tag: u16,
    /// The owning sum's index in the type table.
    pub type_index: u16,
    /// The section holding this constructor's payload records, or -1.
    pub payload_section: i32,
    pub fields: *const SqlFieldInfo,
    pub field_count: u32,
}

/// One type of the schema.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlTypeInfo {
    pub name: SqlName,
    /// A `SqlTypeKind` value.
    pub kind: u8,
    /// The `SqlEnc` of a value of this type when it appears inline or as a
    /// sequence element (`Node` for products and compound sums, `Str` for
    /// `identifier`/`string`, the scalar encodings for the rest).
    pub enc: u8,
    /// The section holding this type's node records, or -1 for builtins and
    /// simple sums (never nodes).
    pub node_section: i32,
    /// The section holding this type's sequence pool, or -1 if the type
    /// never appears as `T*`.
    pub seq_section: i32,
    /// For sums: the constructors are `ctors[ctor_off .. ctor_off +
    /// ctor_count]`, tags indexing within.
    pub ctor_off: u32,
    pub ctor_count: u32,
    /// For products: the product's fields.
    pub fields: *const SqlFieldInfo,
    pub field_count: u32,
}

/// The schema of the `sql.asdl` a library was generated from, as static data
/// inside the library. The host reads it through this layout; identity is
/// by name, resolved at run time, never by hardcoded index.
#[repr(C)]
pub struct SqlSchema {
    pub abi_version: u32,
    pub schema_hash: u64,
    pub types: *const SqlTypeInfo,
    pub type_count: u32,
    pub ctors: *const SqlCtorInfo,
    pub ctor_count: u32,
    /// The type index of the root (`file`).
    pub root_type: u32,
}

/// One section of a tree: a typed array the library owns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SqlSection {
    pub ptr: *mut u8,
    /// Element count.
    pub len: u32,
    /// Element size in bytes.
    pub stride: u32,
}

/// A parsed tree, owned by the library. The host holds the pointer, walks
/// the sections in place, and frees through `release` — never directly, so
/// trees from N dialect libraries can coexist without the host tracking
/// which library owns what.
#[repr(C)]
pub struct SqlTree {
    pub abi_version: u32,
    pub schema_hash: u64,
    /// Library-side destructor: frees the tree and every section.
    pub release: unsafe extern "C" fn(*mut SqlTree),
    pub schema: *const SqlSchema,
    pub sections: *mut SqlSection,
    pub section_count: u32,
    /// The root node's index in the schema's `root_type` section.
    pub root_index: u32,
}

/// A loaded dialect library's descriptor: metadata + entry points.
#[repr(C)]
pub struct SqlDialect {
    pub abi_version: u32,
    pub schema_hash: u64,
    pub name: SqlName,
    pub dialect_version: SqlName,
    pub schema: *const SqlSchema,
    pub parse: SqlParseFn,
}

/// The parse entry point. Reads `src[..src_len]` synchronously. Returns the
/// tree, or null on error with up to `err_buf_cap` bytes of message written
/// to `err_buf` and the message length to `err_len`. `*err_len` is 0 on
/// success. No memory changes hands: the caller provides the error buffer.
pub type SqlParseFn = unsafe extern "C" fn(
    src: *const u8,
    src_len: usize,
    err_buf: *mut u8,
    err_buf_cap: usize,
    err_len: *mut usize,
) -> *mut SqlTree;

// The descriptor tables are static rodata shared across threads; the raw
// pointers inside point at other statics of the same library, which is
// never unloaded (the load-once policy), so the shared references are sound.
unsafe impl Sync for SqlName {}
unsafe impl Sync for SqlFieldInfo {}
unsafe impl Sync for SqlCtorInfo {}
unsafe impl Sync for SqlTypeInfo {}
unsafe impl Sync for SqlSchema {}
unsafe impl Sync for SqlDialect {}

// ---------------------------------------------------------------------------
// Host-side introspection
// ---------------------------------------------------------------------------

/// Why a tree or descriptor was refused at the boundary.
#[derive(Debug)]
pub enum AbiError {
    /// `abi_version` outside the supported range.
    AbiVersion { found: u32, min: u32, max: u32 },
    NullTree,
    NullSchema,
}

impl std::fmt::Display for AbiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbiError::AbiVersion { found, min, max } => write!(
                f,
                "unsupported sql-abi version {found} (this host reads {min}..={max})"
            ),
            AbiError::NullTree => write!(f, "null tree"),
            AbiError::NullSchema => write!(f, "null schema"),
        }
    }
}

impl std::error::Error for AbiError {}

/// Deep validation of a library's schema, run once per library at load
/// time: every `kind`/`enc` byte must decode and every table index must be
/// in range, so the per-node reads afterwards never fail on skewed data.
/// [`Tree::from_raw`] deliberately stays cheap; this is the handshake's
/// second half (the first being the `abi_version` range check).
///
/// # Safety
/// `schema` must be the descriptor of a dialect library whose
/// `abi_version` this host supports; the tables are read in place from the
/// library's memory.
pub unsafe fn validate_schema(schema: &SqlSchema) -> Result<(), String> {
    if !(ABI_VERSION_MIN..=ABI_VERSION).contains(&schema.abi_version) {
        return Err(format!(
            "schema abi_version {} outside the supported range {ABI_VERSION_MIN}..={ABI_VERSION}",
            schema.abi_version
        ));
    }
    if schema.type_count > 0 && schema.types.is_null() {
        return Err("null type table".to_string());
    }
    if schema.ctor_count > 0 && schema.ctors.is_null() {
        return Err("null constructor table".to_string());
    }
    if schema.root_type >= schema.type_count {
        return Err(format!("root type {} out of range", schema.root_type));
    }
    let types = unsafe { table(schema.types, schema.type_count) };
    let ctors = unsafe { table(schema.ctors, schema.ctor_count) };
    for (i, t) in types.iter().enumerate() {
        let name = t.name.as_str();
        let kind = SqlTypeKind::from_u8(t.kind)
            .ok_or_else(|| format!("type {name}: unknown kind {}", t.kind))?;
        let enc = SqlEnc::from_u8(t.enc)
            .ok_or_else(|| format!("type {name}: unknown encoding {}", t.enc))?;
        let enc_ok = match (kind, enc) {
            (SqlTypeKind::Product | SqlTypeKind::Sum, SqlEnc::Node) => true,
            (SqlTypeKind::SimpleSum, SqlEnc::SimpleSum) => true,
            (
                SqlTypeKind::Builtin,
                SqlEnc::Bool | SqlEnc::Byte | SqlEnc::Int | SqlEnc::Long | SqlEnc::Str,
            ) => true,
            _ => false,
        };
        if !enc_ok {
            return Err(format!("type {name}: kind {kind:?} with encoding {enc:?}"));
        }
        if t.node_section < -1 || t.seq_section < -1 {
            return Err(format!("type {name}: section index below -1"));
        }
        if kind == SqlTypeKind::Product {
            validate_field_table(name, t.fields, t.field_count, schema.type_count)?;
        }
        if kind == SqlTypeKind::SimpleSum || kind == SqlTypeKind::Sum {
            if t.ctor_off + t.ctor_count > schema.ctor_count {
                return Err(format!("type {name}: constructor range out of bounds"));
            }
            for j in 0..t.ctor_count {
                let c = &ctors[(t.ctor_off + j) as usize];
                let cname = c.name.as_str();
                if c.type_index != i as u16 {
                    return Err(format!(
                        "constructor {cname}: lists owning type {}",
                        c.type_index
                    ));
                }
                if c.tag != j as u16 {
                    return Err(format!(
                        "constructor {cname}: tag {} != its position {j}",
                        c.tag
                    ));
                }
                if c.payload_section < -1 {
                    return Err(format!("constructor {cname}: section index below -1"));
                }
                validate_field_table(cname, c.fields, c.field_count, schema.type_count)?;
            }
        }
    }
    Ok(())
}

/// A descriptor table as a slice. A null pointer is fine for an empty
/// table (`from_raw_parts` alone forbids it): bare constructors have no
/// field table.
unsafe fn table<'a, T>(ptr: *const T, len: u32) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len as usize) }
    }
}

/// One field table of a product or constructor payload: every encoding
/// byte decodes, and every encoding that references another type names a
/// real one.
fn validate_field_table(
    owner: &str,
    fields: *const SqlFieldInfo,
    field_count: u32,
    type_count: u32,
) -> Result<(), String> {
    if field_count > 0 && fields.is_null() {
        return Err(format!("{owner}: null field table with {field_count} fields"));
    }
    let fields = unsafe { table(fields, field_count) };
    for f in fields {
        let fname = f.name.as_str();
        let enc = SqlEnc::from_u8(f.enc)
            .ok_or_else(|| format!("{owner}.{fname}: unknown encoding {}", f.enc))?;
        let references_type = matches!(
            enc,
            SqlEnc::SimpleSum
                | SqlEnc::Node
                | SqlEnc::OptSimpleSum
                | SqlEnc::OptNode
                | SqlEnc::Seq
        );
        if references_type && (f.type_index as u32) >= type_count {
            return Err(format!(
                "{owner}.{fname}: type index {} out of range",
                f.type_index
            ));
        }
    }
    Ok(())
}

/// A library-owned flat tree under host management: walks in place, frees
/// through the header's `release` on drop.
pub struct Tree {
    raw: *mut SqlTree,
}

impl Tree {
    /// Take ownership of a library-produced tree. `raw` must come from a
    /// dialect library's `parse` (or from `convert_file`); the tree is freed
    /// through its own `release`.
    ///
    /// The version handshake is the loader's job, done once per library on
    /// its `SqlDialect` descriptor; typed projections are not gated beyond
    /// that (strict-only). The checks here are the per-tree invariants that
    /// keep the handshake sound afterwards: a host may hold trees from
    /// several libraries built with different versions, and a `*mut
    /// SqlTree` does not say which library minted it, so adoption re-reads
    /// the header (three scalar comparisons).
    ///
    /// # Safety
    /// `raw` must be a valid `*mut SqlTree` produced by a library whose
    /// schema lies within this host's supported ABI range.
    pub unsafe fn from_raw(raw: *mut SqlTree) -> Result<Tree, AbiError> {
        if raw.is_null() {
            return Err(AbiError::NullTree);
        }
        let abi = unsafe { (*raw).abi_version };
        if !(ABI_VERSION_MIN..=ABI_VERSION).contains(&abi) {
            return Err(AbiError::AbiVersion {
                found: abi,
                min: ABI_VERSION_MIN,
                max: ABI_VERSION,
            });
        }
        if unsafe { (*raw).schema.is_null() } {
            return Err(AbiError::NullSchema);
        }
        Ok(Tree { raw })
    }

    pub fn abi_version(&self) -> u32 {
        unsafe { (*self.raw).abi_version }
    }

    pub fn schema_hash(&self) -> u64 {
        unsafe { (*self.raw).schema_hash }
    }

    fn schema(&self) -> &SqlSchema {
        unsafe { &*(*self.raw).schema }
    }

    fn types(&self) -> &[SqlTypeInfo] {
        let s = self.schema();
        unsafe { table(s.types, s.type_count) }
    }

    fn ctors(&self) -> &[SqlCtorInfo] {
        let s = self.schema();
        unsafe { table(s.ctors, s.ctor_count) }
    }

    fn type_info(&self, ty: u16) -> &SqlTypeInfo {
        &self.types()[ty as usize]
    }

    fn section(&self, i: i32) -> (usize, usize) {
        let t = unsafe { &*self.raw };
        let s = unsafe { &*t.sections.add(i as usize) };
        (s.ptr as usize, s.stride as usize)
    }

    /// The root node (the `file`).
    pub fn root(&self) -> Node<'_> {
        Node {
            tree: self,
            ty: self.schema().root_type as u16,
            index: unsafe { (*self.raw).root_index },
        }
    }

    /// A type's index in the schema, by name. Linear scan over the type
    /// table: resolve once, then keep the index.
    pub fn type_by_name(&self, name: &str) -> Option<u16> {
        self.types()
            .iter()
            .position(|t| t.name.as_str() == name)
            .map(|i| i as u16)
    }

    /// A constructor's global index in the schema, by name. Constructor
    /// names are unique across the schema (the ASDL check enforces it), so
    /// one name identifies one (sum, tag) pair. Resolve once, then keep
    /// the index.
    pub fn kind_by_name(&self, ctor_name: &str) -> Option<u32> {
        self.ctors()
            .iter()
            .position(|c| c.name.as_str() == ctor_name)
            .map(|i| i as u32)
    }

    /// The variant name of a simple-sum value, for renderings.
    pub fn simple_variant_name(&self, ty: u16, tag: u8) -> Option<&str> {
        let t = self.type_info(ty);
        let ctor = self.ctors().get(t.ctor_off as usize + tag as usize)?;
        (tag as u32).lt(&t.ctor_count).then(|| ctor.name.as_str())
    }

    fn str_at(&self, s: SqlStr) -> &str {
        let (base, _) = self.section(SECTION_STRINGS as i32);
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                (base + s.off as usize) as *const u8,
                s.len as usize,
            ))
        }
    }

    /// The node's constructor tag within its sum, for typed projections.
    pub(crate) fn sum_tag(&self, ty: u16, index: u32) -> Option<u16> {
        let t = self.type_info(ty);
        if SqlTypeKind::from_u8(t.kind) != Some(SqlTypeKind::Sum) || t.node_section < 0 {
            return None;
        }
        let (base, stride) = self.section(t.node_section);
        let node: SqlNode =
            unsafe { ((base + index as usize * stride) as *const SqlNode).read() };
        Some(node.tag)
    }

    /// (record pointer, section stride) of a product node, for typed
    /// projections. None when `ty` is not a product.
    pub(crate) fn product_ptr(&self, ty: u16, index: u32) -> Option<(*const u8, usize)> {
        let t = self.type_info(ty);
        if SqlTypeKind::from_u8(t.kind) != Some(SqlTypeKind::Product) || t.node_section < 0 {
            return None;
        }
        let (base, stride) = self.section(t.node_section);
        Some(((base + index as usize * stride) as *const u8, stride))
    }

    /// (payload pointer, section stride) of a sum node's payload when its
    /// tag is `tag`, for typed projections.
    pub(crate) fn payload_ptr(&self, ty: u16, index: u32, tag: u16) -> Option<(*const u8, usize)> {
        let t = self.type_info(ty);
        if SqlTypeKind::from_u8(t.kind) != Some(SqlTypeKind::Sum) || t.node_section < 0 {
            return None;
        }
        if self.sum_tag(ty, index)? != tag {
            return None;
        }
        let ctor = &self.ctors()[t.ctor_off as usize + tag as usize];
        if ctor.payload_section < 0 {
            return None;
        }
        let (pbase, pstride) = self.section(ctor.payload_section);
        let (base, stride) = self.section(t.node_section);
        let node: SqlNode =
            unsafe { ((base + index as usize * stride) as *const SqlNode).read() };
        Some(((pbase + node.payload as usize * pstride) as *const u8, pstride))
    }

    /// The record base pointer of a node: `(record, fields, ctor)` — the
    /// payload address, its field table, and (for sums) the constructor.
    fn node_record(
        &self,
        ty: u16,
        index: u32,
    ) -> (*const u8, &'static [SqlFieldInfo], Option<&SqlCtorInfo>) {
        let t = self.type_info(ty);
        let (base, stride) = self.section(t.node_section);
        match SqlTypeKind::from_u8(t.kind) {
            Some(SqlTypeKind::Product) => {
                let fields = unsafe { table(t.fields, t.field_count) };
                ((base + index as usize * stride) as *const u8, fields, None)
            }
            _ => {
                let node: SqlNode =
                    unsafe { ((base + index as usize * stride) as *const SqlNode).read() };
                let ctor = &self.ctors()[t.ctor_off as usize + node.tag as usize];
                if ctor.payload_section < 0 {
                    return (std::ptr::null(), &[], Some(ctor));
                }
                let (pbase, pstride) = self.section(ctor.payload_section);
                let fields = unsafe { table(ctor.fields, ctor.field_count) };
                (
                    (pbase + node.payload as usize * pstride) as *const u8,
                    fields,
                    Some(ctor),
                )
            }
        }
    }

    fn read_field(&self, base: *const u8, f: &SqlFieldInfo) -> Value<'_> {
        let p = unsafe { base.add(f.offset as usize) };
        let enc = SqlEnc::from_u8(f.enc).expect("schema and host ABI agree (handshake)");
        match enc {
            SqlEnc::Bool => Value::Bool(unsafe { p.cast::<u8>().read() } != 0),
            SqlEnc::Byte => Value::Byte(unsafe { p.cast::<u8>().read() }),
            SqlEnc::Int => Value::Int(unsafe { p.cast::<i32>().read() }),
            SqlEnc::Long => Value::Long(unsafe { p.cast::<i64>().read() }),
            SqlEnc::SimpleSum => Value::SimpleSum(f.type_index, unsafe { p.cast::<u8>().read() }),
            SqlEnc::Str => Value::Str(self.str_at(unsafe { p.cast::<SqlStr>().read() })),
            SqlEnc::Node => Value::Node(Node {
                tree: self,
                ty: f.type_index,
                index: unsafe { p.cast::<u32>().read() },
            }),
            SqlEnc::OptBool => match unsafe { p.cast::<OptBool>().read() }.get() {
                Some(v) => Value::Bool(v),
                None => Value::None,
            },
            SqlEnc::OptByte => match unsafe { p.cast::<OptByte>().read() }.get() {
                Some(v) => Value::Byte(v),
                None => Value::None,
            },
            SqlEnc::OptInt => match unsafe { p.cast::<OptInt>().read() }.get() {
                Some(v) => Value::Int(v),
                None => Value::None,
            },
            SqlEnc::OptLong => match unsafe { p.cast::<OptLong>().read() }.get() {
                Some(v) => Value::Long(v),
                None => Value::None,
            },
            SqlEnc::OptSimpleSum => {
                let v = unsafe { p.cast::<u8>().read() };
                if v == NONE_U8 {
                    Value::None
                } else {
                    Value::SimpleSum(f.type_index, v)
                }
            }
            SqlEnc::OptStr => {
                let s = unsafe { p.cast::<SqlStr>().read() };
                if s.is_none() {
                    Value::None
                } else {
                    Value::Str(self.str_at(s))
                }
            }
            SqlEnc::OptNode => {
                let index = unsafe { p.cast::<u32>().read() };
                if index == NONE_U32 {
                    Value::None
                } else {
                    Value::Node(Node { tree: self, ty: f.type_index, index })
                }
            }
            SqlEnc::Seq => Value::Seq(SeqView {
                tree: self,
                elem_ty: f.type_index,
                seq: unsafe { p.cast::<SqlSeq>().read() },
            }),
        }
    }

    /// The tree's canonical S-expression.
    pub fn to_sexpr(&self) -> String {
        let mut out = String::new();
        write_node(self.root(), &mut out);
        out
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        unsafe { ((*self.raw).release)(self.raw) }
    }
}

/// A by-value cursor into a library-owned tree (TSNode-style): the type
/// index plus the index into the type's node section. Copy, cheap, and
/// valid for as long as the tree is.
#[derive(Clone, Copy)]
pub struct Node<'t> {
    tree: &'t Tree,
    ty: u16,
    index: u32,
}

impl<'t> Node<'t> {
    /// The tree this cursor walks.
    pub fn tree(self) -> &'t Tree {
        self.tree
    }

    /// The node's type index in the schema.
    pub fn type_index(self) -> u16 {
        self.ty
    }

    /// The node's index in its type's node section.
    pub fn index(self) -> u32 {
        self.index
    }

    /// The node's type name (`expr`, `struct_field`, ...).
    pub fn type_name(self) -> &'t str {
        self.tree.type_info(self.ty).name.as_str()
    }

    /// The node's constructor name, for sum nodes; None for products.
    pub fn kind(self) -> Option<&'t str> {
        let (_, _, ctor) = self.tree.node_record(self.ty, self.index);
        ctor.map(|c| c.name.as_str())
    }

    /// The node's constructor as a global schema index (compare against
    /// [`Tree::kind_by_name`]); None for products.
    pub fn kind_id(self) -> Option<u32> {
        let t = self.tree.type_info(self.ty);
        if SqlTypeKind::from_u8(t.kind) != Some(SqlTypeKind::Sum) {
            return None;
        }
        let (base, stride) = self.tree.section(t.node_section);
        let node: SqlNode =
            unsafe { ((base + self.index as usize * stride) as *const SqlNode).read() };
        Some(t.ctor_off + node.tag as u32)
    }

    /// The node's fields, as raw descriptors (name + encoding + offset).
    pub fn fields(self) -> &'t [SqlFieldInfo] {
        let (_, fields, _) = self.tree.node_record(self.ty, self.index);
        fields
    }

    /// The field named `name`, or None when the node's constructor has no
    /// such field (skew included).
    pub fn field(self, name: &str) -> Option<Value<'t>> {
        let (base, fields, _) = self.tree.node_record(self.ty, self.index);
        let f = fields.iter().find(|f| f.name.as_str() == name)?;
        Some(self.tree.read_field(base, f))
    }

    /// The `i`th field, or None when out of range.
    pub fn field_at(self, i: usize) -> Option<Value<'t>> {
        let (base, fields, _) = self.tree.node_record(self.ty, self.index);
        let f = fields.get(i)?;
        Some(self.tree.read_field(base, f))
    }
}

/// A field's value, read through introspection.
pub enum Value<'t> {
    /// An optional field holding None.
    None,
    Bool(bool),
    Byte(u8),
    Int(i32),
    Long(i64),
    Str(&'t str),
    /// A simple-sum value: `(type_index, tag)`. The variant name resolves
    /// through [`Tree::simple_variant_name`].
    SimpleSum(u16, u8),
    Node(Node<'t>),
    Seq(SeqView<'t>),
}

/// A sequence field's elements, read lazily.
#[derive(Clone, Copy)]
pub struct SeqView<'t> {
    tree: &'t Tree,
    elem_ty: u16,
    seq: SqlSeq,
}

impl<'t> SeqView<'t> {
    pub fn len(self) -> usize {
        self.seq.len as usize
    }

    pub fn is_empty(self) -> bool {
        self.seq.len == 0
    }

    /// The element type's index in the schema.
    pub fn elem_type(self) -> u16 {
        self.elem_ty
    }

    /// The `i`th element, or None when out of bounds.
    pub fn get(self, i: usize) -> Option<Value<'t>> {
        if i >= self.seq.len as usize {
            return None;
        }
        let t = self.tree.type_info(self.elem_ty);
        let (base, stride) = self.tree.section(t.seq_section);
        let p = (base + (self.seq.off as usize + i) * stride) as *const u8;
        let enc = SqlEnc::from_u8(t.enc).expect("schema and host ABI agree (handshake)");
        Some(match enc {
            SqlEnc::Str => Value::Str(self.tree.str_at(unsafe { p.cast::<SqlStr>().read() })),
            SqlEnc::SimpleSum => Value::SimpleSum(self.elem_ty, unsafe { p.cast::<u8>().read() }),
            SqlEnc::Int => Value::Int(unsafe { p.cast::<i32>().read() }),
            SqlEnc::Long => Value::Long(unsafe { p.cast::<i64>().read() }),
            SqlEnc::Byte | SqlEnc::Bool => Value::Byte(unsafe { p.cast::<u8>().read() }),
            SqlEnc::Node => Value::Node(Node {
                tree: self,
                ty: self.elem_ty,
                index: unsafe { p.cast::<u32>().read() },
            }),
            _ => unreachable!("element encodings are never optional"),
        })
    }

    /// Iterate the elements.
    pub fn iter(self) -> impl Iterator<Item = Value<'t>> {
        (0..self.seq.len as usize).map(move |i| self.get(i).expect("in bounds"))
    }
}

// ---------------------------------------------------------------------------
// The dialect-library export
// ---------------------------------------------------------------------------

/// The trampoline every exported parse entry funnels through: decode the
/// input, run the library's parse under `catch_unwind`, and report errors
/// through the caller's buffer. Nothing unwinds across the boundary.
///
/// # Safety
/// Raw parts of the exported entry point's contract; called only by the
/// wrapper `export_sql_dialect!` generates.
pub unsafe fn parse_entry(
    parse: fn(&str) -> Result<*mut SqlTree, String>,
    src: *const u8,
    src_len: usize,
    err_buf: *mut u8,
    err_buf_cap: usize,
    err_len: *mut usize,
) -> *mut SqlTree {
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let bytes = unsafe { std::slice::from_raw_parts(src, src_len) };
        match std::str::from_utf8(bytes) {
            Ok(s) => parse(s),
            Err(e) => Err(format!("input is not UTF-8: {e}")),
        }
    }));
    let result = match run {
        Ok(r) => r,
        Err(_) => Err("panic in parse".to_string()),
    };
    match result {
        Ok(tree) => {
            unsafe { *err_len = 0 };
            tree
        }
        Err(msg) => {
            let bytes = msg.as_bytes();
            let n = bytes.len().min(err_buf_cap);
            if n > 0 {
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), err_buf, n) };
            }
            unsafe { *err_len = n };
            std::ptr::null_mut()
        }
    }
}

/// Export a dialect library's descriptor under a per-dialect symbol.
///
/// ```ignore
/// export_sql_dialect!(sql_dialect_snowflake, "snowflake", "1.0.0", parse);
/// ```
///
/// expands to a `static` [`SqlDialect`] plus
/// `pub extern "C" fn sql_dialect_snowflake() -> *const SqlDialect`. `parse`
/// is a `fn(&str) -> Result<*mut SqlTree, String>` — typically the lowering
/// plus [`TreeBuilder::finish`]. Per-dialect symbols (not a generic entry)
/// keep static linking possible. Requires the `builder` feature: the
/// descriptor references the library-side schema statics.
#[macro_export]
macro_rules! export_sql_dialect {
    ($entry:ident, $name:expr, $version:expr, $parse:expr) => {
        static DIALECT: $crate::SqlDialect = $crate::SqlDialect {
            abi_version: $crate::ABI_VERSION,
            schema_hash: $crate::SCHEMA_HASH,
            name: $crate::SqlName::from_static($name),
            dialect_version: $crate::SqlName::from_static($version),
            schema: &$crate::SCHEMA,
            parse: {
                unsafe extern "C" fn wrapped(
                    src: *const u8,
                    src_len: usize,
                    err_buf: *mut u8,
                    err_buf_cap: usize,
                    err_len: *mut usize,
                ) -> *mut $crate::SqlTree {
                    unsafe {
                        $crate::parse_entry($parse, src, src_len, err_buf, err_buf_cap, err_len)
                    }
                }
                wrapped
            },
        };

        /// The dialect descriptor entry point.
        #[unsafe(no_mangle)]
        pub extern "C" fn $entry() -> *const $crate::SqlDialect {
            &DIALECT
        }
    };
}

/// Escape and quote a string the way the Sexpr rendering does (C-style
/// escapes; control characters as `\u{..}`). Shared by the introspection
/// renderer.
pub(crate) fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{{{:x}}}", c as u32).expect("writing to a String cannot fail");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn camel_sexpr(name: &str) -> String {
    name.split('_')
        .map(|p| p[..1].to_uppercase() + &p[1..])
        .collect()
}

fn write_node(node: Node, out: &mut String) {
    let is_sum = node.kind().is_some();
    let name_owned;
    let name = match node.kind() {
        Some(ctor) => ctor,
        None => {
            name_owned = camel_sexpr(node.type_name());
            &name_owned
        }
    };
    let fields = node.fields();

    let printable: Vec<usize> = (0..fields.len())
        .filter(|&i| !fields[i].name.as_str().starts_with("span"))
        .collect();

    let mut rendered: Vec<(usize, Value)> = Vec::new();
    for &i in &printable {
        let value = node.field_at(i).expect("index in range");
        let elided = match &value {
            Value::None => true,
            Value::Bool(b) => !b,
            Value::Seq(s) => s.is_empty(),
            _ => false,
        };
        if !elided {
            rendered.push((i, value));
        }
    }

    if is_sum && rendered.is_empty() {
        out.push_str(name);
        return;
    }
    out.push('(');
    out.push_str(name);
    let node_tree = node.tree;
    for (i, value) in rendered {
        out.push_str(" (");
        out.push_str(fields[i].name.as_str());
        out.push(' ');
        write_value(node_tree, value, out);
        out.push(')');
    }
    out.push(')');
}

fn write_value(tree: &Tree, value: Value, out: &mut String) {
    match value {
        Value::None => out.push_str("None"),
        Value::Bool(b) => out.push_str(if b { "true" } else { "false" }),
        Value::Byte(v) => push_int(v as i64, out),
        Value::Int(v) => push_int(v as i64, out),
        Value::Long(v) => push_int(v, out),
        Value::Str(s) => write_escaped(s, out),
        Value::SimpleSum(ty, tag) => {
            let name = tree.simple_variant_name(ty, tag).expect("tag in range");
            out.push_str(name);
        }
        Value::Node(n) => write_node(n, out),
        Value::Seq(s) => {
            out.push('[');
            for (i, item) in s.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(tree, item, out);
            }
            out.push(']');
        }
    }
}

fn push_int(v: i64, out: &mut String) {
    write!(out, "{v}").expect("writing to a String cannot fail");
}

/// The host-side loader: `dlopen` a dialect library, run the once-per-library
/// handshake, and hand out parsed trees.
#[allow(unexpected_cfgs)]
#[cfg(feature = "loader")]
pub mod loader {
    use std::path::Path;

    use super::{ABI_VERSION, ABI_VERSION_MIN, AbiError, SqlDialect, Tree, validate_schema};

    /// A loaded dialect library: its descriptor plus the parse entry point.
    ///
    /// The library is never unloaded (the load-once policy), so the descriptor
    /// and every tree it produces stay valid for the rest of the process.
    pub struct Dialect {
        descriptor: &'static SqlDialect,
    }

    /// Why a library was not loaded.
    #[derive(Debug)]
    pub enum LoadError {
        /// `dlopen` itself failed.
        Open(String),
        /// The library exports no `sql_dialect_<name>` entry point.
        MissingSymbol(String),
        /// The descriptor failed the version handshake.
        Abi(AbiError),
        /// The schema failed load-time validation.
        BadSchema(String),
    }

    impl std::fmt::Display for LoadError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                LoadError::Open(e) => write!(f, "cannot load the dialect library: {e}"),
                LoadError::MissingSymbol(s) => {
                    write!(f, "the library exports no `{s}` entry point")
                }
                LoadError::Abi(e) => write!(f, "the dialect library was refused: {e}"),
                LoadError::BadSchema(e) => {
                    write!(f, "the dialect library's schema is invalid: {e}")
                }
            }
        }
    }

    impl std::error::Error for LoadError {}

    impl Dialect {
        /// Load the dialect library at `path` under the dialect name `name` and
        /// run the handshake. The entry symbol is `sql_dialect_<name>` —
        /// per-dialect symbols keep static linking possible.
        ///
        /// # Safety
        /// `path` must name a trusted dialect library: loading runs its
        /// initializers and calls its entry point, and the handshake checks
        /// version compatibility, not malicious intent.
        pub unsafe fn load(path: &Path, name: &str) -> Result<Dialect, LoadError> {
            let lib = unsafe { libloading::Library::new(path) }
                .map_err(|e| LoadError::Open(e.to_string()))?;
            let symbol = format!("sql_dialect_{name}");
            let entry = unsafe {
                lib.get::<unsafe extern "C" fn() -> *const SqlDialect>(symbol.as_bytes())
            }
            .map_err(|_| LoadError::MissingSymbol(symbol))?;
            let raw = unsafe { entry() };
            if raw.is_null() {
                return Err(LoadError::BadSchema("null descriptor".to_string()));
            }
            let descriptor = unsafe { &*raw };
            if !(ABI_VERSION_MIN..=ABI_VERSION).contains(&descriptor.abi_version) {
                return Err(LoadError::Abi(AbiError::AbiVersion {
                    found: descriptor.abi_version,
                    min: ABI_VERSION_MIN,
                    max: ABI_VERSION,
                }));
            }
            if descriptor.schema.is_null() {
                return Err(LoadError::Abi(AbiError::NullSchema));
            }
            unsafe { validate_schema(&*descriptor.schema) }.map_err(LoadError::BadSchema)?;
            std::mem::forget(lib);
            Ok(Dialect { descriptor })
        }

        /// The library's descriptor.
        pub fn descriptor(&self) -> &SqlDialect {
            self.descriptor
        }

        /// The dialect's name.
        pub fn name(&self) -> &str {
            self.descriptor.name.as_str()
        }

        /// The dialect library's own version string.
        pub fn dialect_version(&self) -> &str {
            self.descriptor.dialect_version.as_str()
        }

        /// The hash of the `sql.asdl` the library was generated from.
        pub fn schema_hash(&self) -> u64 {
            self.descriptor.schema_hash
        }

        /// Parse `sql`, returning the library-owned tree under host management.
        /// Errors arrive through the caller-provided buffer of the boundary
        /// convention; nothing unwinds across it.
        pub fn parse(&self, sql: &str) -> Result<Tree, String> {
            let mut err_buf = [0u8; 4096];
            let mut err_len = 0usize;
            let raw = unsafe {
                (self.descriptor.parse)(
                    sql.as_ptr(),
                    sql.len(),
                    err_buf.as_mut_ptr(),
                    err_buf.len(),
                    &mut err_len,
                )
            };
            if raw.is_null() {
                return Err(String::from_utf8_lossy(&err_buf[..err_len]).into_owned());
            }
            let tree = unsafe { Tree::from_raw(raw) };
            Ok(tree.expect("the library passed the load-time handshake"))
        }
    }
}

#[allow(unexpected_cfgs)]
#[cfg(feature = "loader")]
pub use loader::{Dialect, LoadError};
