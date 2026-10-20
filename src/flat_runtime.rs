use std::fmt::Write as _;

use crate::sexpr::write_escaped;

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
pub struct FlatStr {
    /// Byte offset into the UTF-8 string section (`NONE_U32` when `None`).
    pub off: u32,
    /// Byte length of the UTF-8 string.
    pub len: u32,
}

impl FlatStr {
    /// The `None` sentinel for an optional string field.
    pub const NONE: Self = FlatStr {
        off: NONE_U32,
        len: 0,
    };

    /// Returns `true` if this string holds the `None` sentinel.
    pub fn is_none(self) -> bool {
        self.off == NONE_U32
    }
}

/// A sequence field: `{off, len}` elements in the element type's pool
/// section.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatSeq {
    /// Element offset into the sequence pool section.
    pub off: u32,
    /// Number of elements in the sequence.
    pub len: u32,
}

/// Optional boolean value in the flat C ABI layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptBool {
    /// Non-zero when a value is present, or `0` for `None`.
    pub is_some: u32,
    /// Boolean value (`0` for `false`, non-zero for `true`).
    pub value: u8,
}

/// Optional byte (`u8`) value in the flat C ABI layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptByte {
    /// Non-zero when a value is present, or `0` for `None`.
    pub is_some: u32,
    /// Byte payload when `is_some != 0`.
    pub value: u8,
}

/// Optional 32-bit signed integer (`i32`) value in the flat C ABI layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptInt {
    /// Non-zero when a value is present, or `0` for `None`.
    pub is_some: u32,
    /// Integer payload when `is_some != 0`.
    pub value: i32,
}

/// Optional 64-bit signed integer (`i64`) value in the flat C ABI layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptLong {
    /// Non-zero when a value is present, or `0` for `None`.
    pub is_some: u32,
    /// Long integer payload when `is_some != 0`.
    pub value: i64,
}

macro_rules! opt_scalar {
    ($t:ident, $v:ty) => {
        impl $t {
            /// The `None` value for this optional scalar type.
            pub const NONE: Self = $t {
                is_some: 0,
                value: 0,
            };

            /// Returns `Some(value)` if `is_some != 0`, or `None` otherwise.
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
    /// The `None` value for an optional boolean field.
    pub const NONE: Self = OptBool {
        is_some: 0,
        value: 0,
    };

    /// Returns `Some(bool)` if `is_some != 0`, or `None` otherwise.
    pub fn get(self) -> Option<bool> {
        (self.is_some != 0).then_some(self.value != 0)
    }
}

impl From<Option<bool>> for OptBool {
    fn from(v: Option<bool>) -> Self {
        match v {
            Some(value) => OptBool {
                is_some: 1,
                value: value as u8,
            },
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
pub struct FlatNode {
    /// Constructor index within the sum, in ASDL declaration order.
    pub tag: u16,
    /// Reserved alignment padding (zero).
    pub _pad: u16,
    /// Index of the payload record in the constructor section (`NONE_U32` when bare).
    pub payload: u32,
}

// ---------------------------------------------------------------------------
// The descriptor types: the dialect library's metadata and the schema-as-data
// the host's introspection reads. `abi_version` is the first field of both
// FlatDialect and FlatTree, readable without knowing the rest of the layout.
// ---------------------------------------------------------------------------

/// A name (or any static string) held by a descriptor: a pointer into the
/// library's rodata plus its length. Thin pointer + length by hand: fat
/// pointers never cross the boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlatName {
    /// Pointer to static UTF-8 bytes in the library's read-only data.
    pub ptr: *const u8,
    /// Byte length of the UTF-8 string.
    pub len: u32,
}

impl FlatName {
    /// Creates a `FlatName` from a `'static` string slice.
    pub const fn from_static(s: &'static str) -> Self {
        FlatName {
            ptr: s.as_ptr(),
            len: s.len() as u32,
        }
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
/// Stored as `u8` in the tables and converted through [`FlatEnc::from_u8`]
/// so an unrecognized value is data, not UB.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatEnc {
    /// A `u8` boolean (`0` or `1`).
    Bool = 0,
    /// A `u8` byte value.
    Byte = 1,
    /// An `i32` integer value.
    Int = 2,
    /// An `i64` integer value.
    Long = 3,
    /// A `u8` discriminant of the simple sum `type_index`.
    SimpleSum = 4,
    /// A `FlatStr`.
    Str = 5,
    /// A `u32` node index of type `type_index`.
    Node = 6,
    /// An `OptBool`.
    OptBool = 7,
    /// An `OptByte`.
    OptByte = 8,
    /// An `OptInt`.
    OptInt = 9,
    /// An `OptLong`.
    OptLong = 10,
    /// A `u8` discriminant, `NONE_U8` for None.
    OptSimpleSum = 11,
    /// A `FlatStr`, `off == NONE_U32` for None.
    OptStr = 12,
    /// A `u32` node index of type `type_index`, `NONE_U32` for None.
    OptNode = 13,
    /// A `FlatSeq` of the element type `type_index`.
    Seq = 14,
}

impl FlatEnc {
    /// Converts a raw `u8` byte into a [`FlatEnc`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<FlatEnc> {
        Some(match v {
            0 => FlatEnc::Bool,
            1 => FlatEnc::Byte,
            2 => FlatEnc::Int,
            3 => FlatEnc::Long,
            4 => FlatEnc::SimpleSum,
            5 => FlatEnc::Str,
            6 => FlatEnc::Node,
            7 => FlatEnc::OptBool,
            8 => FlatEnc::OptByte,
            9 => FlatEnc::OptInt,
            10 => FlatEnc::OptLong,
            11 => FlatEnc::OptSimpleSum,
            12 => FlatEnc::OptStr,
            13 => FlatEnc::OptNode,
            14 => FlatEnc::Seq,
            _ => return None,
        })
    }
}

/// The kind of a schema type, stored as `u8` like [`FlatEnc`].
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatTypeKind {
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

impl FlatTypeKind {
    /// Converts a raw `u8` byte into a [`FlatTypeKind`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<FlatTypeKind> {
        Some(match v {
            0 => FlatTypeKind::Builtin,
            1 => FlatTypeKind::SimpleSum,
            2 => FlatTypeKind::Product,
            3 => FlatTypeKind::Sum,
            _ => return None,
        })
    }
}

/// One field of a constructor payload or product record.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlatFieldInfo {
    /// Field name in the ASDL specification.
    pub name: FlatName,
    /// Byte offset of the field within its record.
    pub offset: u32,
    /// A `FlatEnc` value.
    pub enc: u8,
    /// The referenced/element type, for `SimpleSum`/`Node`/`OptNode`/`Seq`/
    /// `OptSimpleSum` encodings; `u16::MAX` otherwise.
    pub type_index: u16,
}

/// One constructor of a sum (compound or simple).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlatCtorInfo {
    /// Constructor name in the ASDL specification.
    pub name: FlatName,
    /// The tag value within its sum.
    pub tag: u16,
    /// The owning sum's index in the type table.
    pub type_index: u16,
    /// The section holding this constructor's payload records, or -1.
    pub payload_section: i32,
    /// Pointer to the constructor's field descriptor table, or null when `field_count == 0`.
    pub fields: *const FlatFieldInfo,
    /// Number of fields in `fields`.
    pub field_count: u32,
}

/// One type of the schema.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlatTypeInfo {
    /// Type name in the ASDL specification.
    pub name: FlatName,
    /// A `FlatTypeKind` value.
    pub kind: u8,
    /// The `FlatEnc` of a value of this type when it appears inline or as a
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
    /// Number of constructors for sum types, or `0` for products and builtins.
    pub ctor_count: u32,
    /// For products: the product's fields.
    pub fields: *const FlatFieldInfo,
    /// Number of fields for product types, or `0` otherwise.
    pub field_count: u32,
}

/// The schema of the `sql.asdl` a library was generated from, as static data
/// inside the library. The host reads it through this layout; identity is
/// by name, resolved at run time, never by hardcoded index.
#[repr(C)]
pub struct FlatSchema {
    /// Boundary ABI version (`ABI_VERSION`).
    pub abi_version: u32,
    /// 64-bit FNV-1a hash of the source `.asdl` bytes.
    pub schema_hash: u64,
    /// Pointer to the schema's type descriptor table.
    pub types: *const FlatTypeInfo,
    /// Number of entries in `types`.
    pub type_count: u32,
    /// Pointer to the schema's constructor descriptor table.
    pub ctors: *const FlatCtorInfo,
    /// Number of entries in `ctors`.
    pub ctor_count: u32,
    /// The type index of the root (`file`).
    pub root_type: u32,
}

/// One section of a tree: a typed array the library owns.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlatSection {
    /// Base pointer to the section's array buffer.
    pub ptr: *mut u8,
    /// Element count.
    pub len: u32,
    /// Element size in bytes.
    pub stride: u32,
}

/// A parsed tree, owned by the library.
#[repr(C)]
pub struct FlatTree {
    /// Boundary ABI version (`ABI_VERSION`).
    pub abi_version: u32,
    /// 64-bit FNV-1a hash of the source `.asdl` bytes.
    pub schema_hash: u64,
    /// Library-side destructor: frees the tree and every section.
    pub release: unsafe extern "C" fn(*mut FlatTree),
    /// Pointer to the static schema descriptor of the library.
    pub schema: *const FlatSchema,
    /// Pointer to the section array of length `section_count`.
    pub sections: *mut FlatSection,
    /// Number of sections in `sections`.
    pub section_count: u32,
    /// The root node's index in the schema's `root_type` section.
    pub root_index: u32,
}

/// A loaded dialect library's descriptor: metadata + entry points.
#[repr(C)]
pub struct FlatDialect {
    /// Boundary ABI version (`ABI_VERSION`).
    pub abi_version: u32,
    /// 64-bit FNV-1a hash of the source `.asdl` bytes.
    pub schema_hash: u64,
    /// Dialect identifier name (for example, `"snowflake"`).
    pub name: FlatName,
    /// Dialect library version string (for example, `"1.0.0"`).
    pub dialect_version: FlatName,
    /// Pointer to the static schema descriptor of the library.
    pub schema: *const FlatSchema,
    /// Synchronous parse entry point of the dialect library.
    pub parse: FlatParseFn,
}

/// The parse entry point. Reads `src[..src_len]` synchronously. Returns the
/// tree, or null on error with up to `err_buf_cap` bytes of message written
/// to `err_buf` and the message length to `err_len`. `*err_len` is 0 on
/// success. No memory changes hands: the caller provides the error buffer.
pub type FlatParseFn = unsafe extern "C" fn(
    src: *const u8,
    src_len: usize,
    err_buf: *mut u8,
    err_buf_cap: usize,
    err_len: *mut usize,
) -> *mut FlatTree;

// The descriptor tables are static rodata shared across threads; the raw
// pointers inside point at other statics of the same library, which is
// never unloaded (the load-once policy), so the shared references are sound.
unsafe impl Sync for FlatName {}
unsafe impl Sync for FlatFieldInfo {}
unsafe impl Sync for FlatCtorInfo {}
unsafe impl Sync for FlatTypeInfo {}
unsafe impl Sync for FlatSchema {}
unsafe impl Sync for FlatDialect {}

// ---------------------------------------------------------------------------
// Host-side introspection
// ---------------------------------------------------------------------------

/// Why a tree or descriptor was refused at the boundary.
#[derive(Debug)]
pub enum AbiError {
    /// `abi_version` outside the supported range.
    AbiVersion {
        /// ABI version reported by the library or tree.
        found: u32,
        /// Minimum ABI version supported by the host.
        min: u32,
        /// Maximum ABI version supported by the host.
        max: u32,
    },
    /// The `*mut FlatTree` pointer was null.
    NullTree,
    /// The `*const FlatSchema` pointer was null.
    NullSchema,
}

impl std::fmt::Display for AbiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbiError::AbiVersion { found, min, max } => {
                write!(f, "unsupported sql-abi version {found} (this host reads {min}..={max})")
            }
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
pub unsafe fn validate_schema(schema: &FlatSchema) -> Result<(), String> {
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
        let kind = FlatTypeKind::from_u8(t.kind)
            .ok_or_else(|| format!("type {name}: unknown kind {}", t.kind))?;
        let enc = FlatEnc::from_u8(t.enc)
            .ok_or_else(|| format!("type {name}: unknown encoding {}", t.enc))?;
        let enc_ok = matches!(
            (kind, enc),
            (FlatTypeKind::Product | FlatTypeKind::Sum, FlatEnc::Node)
                | (FlatTypeKind::SimpleSum, FlatEnc::SimpleSum)
                | (
                    FlatTypeKind::Builtin,
                    FlatEnc::Bool | FlatEnc::Byte | FlatEnc::Int | FlatEnc::Long | FlatEnc::Str,
                )
        );
        if !enc_ok {
            return Err(format!("type {name}: kind {kind:?} with encoding {enc:?}"));
        }
        if t.node_section < -1 || t.seq_section < -1 {
            return Err(format!("type {name}: section index below -1"));
        }
        if kind == FlatTypeKind::Product {
            validate_field_table(name, t.fields, t.field_count, schema.type_count)?;
        }
        if kind == FlatTypeKind::SimpleSum || kind == FlatTypeKind::Sum {
            if t.ctor_off + t.ctor_count > schema.ctor_count {
                return Err(format!("type {name}: constructor range out of bounds"));
            }
            for j in 0..t.ctor_count {
                let c = &ctors[(t.ctor_off + j) as usize];
                let cname = c.name.as_str();
                if c.type_index != i as u16 {
                    return Err(format!("constructor {cname}: lists owning type {}", c.type_index));
                }
                if c.tag != j as u16 {
                    return Err(format!("constructor {cname}: tag {} != its position {j}", c.tag));
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
    fields: *const FlatFieldInfo,
    field_count: u32,
    type_count: u32,
) -> Result<(), String> {
    if field_count > 0 && fields.is_null() {
        return Err(format!("{owner}: null field table with {field_count} fields"));
    }
    let fields = unsafe { table(fields, field_count) };
    for f in fields {
        let fname = f.name.as_str();
        let enc = FlatEnc::from_u8(f.enc)
            .ok_or_else(|| format!("{owner}.{fname}: unknown encoding {}", f.enc))?;
        let references_type = matches!(
            enc,
            FlatEnc::SimpleSum
                | FlatEnc::Node
                | FlatEnc::OptSimpleSum
                | FlatEnc::OptNode
                | FlatEnc::Seq
        );
        if references_type && (f.type_index as u32) >= type_count {
            return Err(format!("{owner}.{fname}: type index {} out of range", f.type_index));
        }
    }
    Ok(())
}

/// A library-owned flat tree under host management: walks in place, frees
/// through the header's `release` on drop.
pub struct Tree {
    raw: *mut FlatTree,
}

impl Tree {
    /// Take ownership of a library-produced tree. `raw` must come from a
    /// dialect library's `parse` (or from `convert_file`); the tree is freed
    /// through its own `release`.
    ///
    /// The version handshake is the loader's job, done once per library on
    /// its `FlatDialect` descriptor; typed projections are not gated beyond
    /// that (strict-only). The checks here are the per-tree invariants that
    /// keep the handshake sound afterwards: a host may hold trees from
    /// several libraries built with different versions, and a `*mut
    /// FlatTree` does not say which library minted it, so adoption re-reads
    /// the header (three scalar comparisons).
    ///
    /// # Safety
    /// `raw` must be a valid `*mut FlatTree` produced by a library whose
    /// schema lies within this host's supported ABI range.
    pub unsafe fn from_raw(raw: *mut FlatTree) -> Result<Tree, AbiError> {
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

    /// Returns the ABI version from the tree header.
    pub fn abi_version(&self) -> u32 {
        unsafe { (*self.raw).abi_version }
    }

    /// Returns the 64-bit FNV-1a schema hash from the tree header.
    pub fn schema_hash(&self) -> u64 {
        unsafe { (*self.raw).schema_hash }
    }

    fn schema(&self) -> &FlatSchema {
        unsafe { &*(*self.raw).schema }
    }

    fn types(&self) -> &[FlatTypeInfo] {
        let s = self.schema();
        unsafe { table(s.types, s.type_count) }
    }

    fn ctors(&self) -> &[FlatCtorInfo] {
        let s = self.schema();
        unsafe { table(s.ctors, s.ctor_count) }
    }

    fn type_info(&self, ty: u16) -> &FlatTypeInfo {
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

    fn str_at(&self, s: FlatStr) -> &str {
        let (base, _) = self.section(SECTION_STRINGS as i32);
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                (base + s.off as usize) as *const u8,
                s.len as usize,
            ))
        }
    }

    /// The node's constructor tag within its sum, for typed projections.
    pub fn sum_tag(&self, ty: u16, index: u32) -> Option<u16> {
        let t = self.type_info(ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Sum) || t.node_section < 0 {
            return None;
        }
        let (base, stride) = self.section(t.node_section);
        let node: FlatNode =
            unsafe { ((base + index as usize * stride) as *const FlatNode).read() };
        Some(node.tag)
    }

    /// (record pointer, section stride) of a product node, for typed
    /// projections. None when `ty` is not a product.
    pub fn product_ptr(&self, ty: u16, index: u32) -> Option<(*const u8, usize)> {
        let t = self.type_info(ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Product) || t.node_section < 0 {
            return None;
        }
        let (base, stride) = self.section(t.node_section);
        Some(((base + index as usize * stride) as *const u8, stride))
    }

    /// (payload pointer, section stride) of a sum node's payload when its
    /// tag is `tag`, for typed projections.
    pub fn payload_ptr(&self, ty: u16, index: u32, tag: u16) -> Option<(*const u8, usize)> {
        let t = self.type_info(ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Sum) || t.node_section < 0 {
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
        let node: FlatNode =
            unsafe { ((base + index as usize * stride) as *const FlatNode).read() };
        Some(((pbase + node.payload as usize * pstride) as *const u8, pstride))
    }

    /// The record base pointer of a node: `(record, fields, ctor)` — the
    /// payload address, its field table, and (for sums) the constructor.
    fn node_record(
        &self,
        ty: u16,
        index: u32,
    ) -> (*const u8, &'static [FlatFieldInfo], Option<&FlatCtorInfo>) {
        let t = self.type_info(ty);
        let (base, stride) = self.section(t.node_section);
        match FlatTypeKind::from_u8(t.kind) {
            Some(FlatTypeKind::Product) => {
                let fields = unsafe { table(t.fields, t.field_count) };
                ((base + index as usize * stride) as *const u8, fields, None)
            }
            _ => {
                let node: FlatNode =
                    unsafe { ((base + index as usize * stride) as *const FlatNode).read() };
                let ctor = &self.ctors()[t.ctor_off as usize + node.tag as usize];
                if ctor.payload_section < 0 {
                    return (std::ptr::null(), &[], Some(ctor));
                }
                let (pbase, pstride) = self.section(ctor.payload_section);
                let fields = unsafe { table(ctor.fields, ctor.field_count) };
                ((pbase + node.payload as usize * pstride) as *const u8, fields, Some(ctor))
            }
        }
    }

    fn read_field(&self, base: *const u8, f: &FlatFieldInfo) -> Value<'_> {
        let p = unsafe { base.add(f.offset as usize) };
        let enc = FlatEnc::from_u8(f.enc).expect("schema and host ABI agree (handshake)");
        match enc {
            FlatEnc::Bool => Value::Bool(unsafe { p.cast::<u8>().read() } != 0),
            FlatEnc::Byte => Value::Byte(unsafe { p.cast::<u8>().read() }),
            FlatEnc::Int => Value::Int(unsafe { p.cast::<i32>().read() }),
            FlatEnc::Long => Value::Long(unsafe { p.cast::<i64>().read() }),
            FlatEnc::SimpleSum => Value::SimpleSum(f.type_index, unsafe { p.cast::<u8>().read() }),
            FlatEnc::Str => Value::Str(self.str_at(unsafe { p.cast::<FlatStr>().read() })),
            FlatEnc::Node => Value::Node(Node {
                tree: self,
                ty: f.type_index,
                index: unsafe { p.cast::<u32>().read() },
            }),
            FlatEnc::OptBool => match unsafe { p.cast::<OptBool>().read() }.get() {
                Some(v) => Value::Bool(v),
                None => Value::None,
            },
            FlatEnc::OptByte => match unsafe { p.cast::<OptByte>().read() }.get() {
                Some(v) => Value::Byte(v),
                None => Value::None,
            },
            FlatEnc::OptInt => match unsafe { p.cast::<OptInt>().read() }.get() {
                Some(v) => Value::Int(v),
                None => Value::None,
            },
            FlatEnc::OptLong => match unsafe { p.cast::<OptLong>().read() }.get() {
                Some(v) => Value::Long(v),
                None => Value::None,
            },
            FlatEnc::OptSimpleSum => {
                let v = unsafe { p.cast::<u8>().read() };
                if v == NONE_U8 {
                    Value::None
                } else {
                    Value::SimpleSum(f.type_index, v)
                }
            }
            FlatEnc::OptStr => {
                let s = unsafe { p.cast::<FlatStr>().read() };
                if s.is_none() {
                    Value::None
                } else {
                    Value::Str(self.str_at(s))
                }
            }
            FlatEnc::OptNode => {
                let index = unsafe { p.cast::<u32>().read() };
                if index == NONE_U32 {
                    Value::None
                } else {
                    Value::Node(Node {
                        tree: self,
                        ty: f.type_index,
                        index,
                    })
                }
            }
            FlatEnc::Seq => Value::Seq(SeqView {
                tree: self,
                elem_ty: f.type_index,
                seq: unsafe { p.cast::<FlatSeq>().read() },
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
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Sum) {
            return None;
        }
        let (base, stride) = self.tree.section(t.node_section);
        let node: FlatNode =
            unsafe { ((base + self.index as usize * stride) as *const FlatNode).read() };
        Some(t.ctor_off + node.tag as u32)
    }

    /// The node's fields, as raw descriptors (name + encoding + offset).
    pub fn fields(self) -> &'t [FlatFieldInfo] {
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
    /// A boolean value.
    Bool(bool),
    /// A byte (`u8`) value.
    Byte(u8),
    /// A 32-bit signed integer (`i32`) value.
    Int(i32),
    /// A 64-bit signed integer (`i64`) value.
    Long(i64),
    /// A borrowed UTF-8 string slice from the tree's string section.
    Str(&'t str),
    /// A simple-sum value: `(type_index, tag)`. The variant name resolves
    /// through [`Tree::simple_variant_name`].
    SimpleSum(u16, u8),
    /// A child AST node cursor.
    Node(Node<'t>),
    /// A lazy view over a sequence field.
    Seq(SeqView<'t>),
}

/// A sequence field's elements, read lazily.
#[derive(Clone, Copy)]
pub struct SeqView<'t> {
    tree: &'t Tree,
    elem_ty: u16,
    seq: FlatSeq,
}

impl<'t> SeqView<'t> {
    /// Returns the number of elements in the sequence.
    pub fn len(self) -> usize {
        self.seq.len as usize
    }

    /// Returns `true` if the sequence contains no elements.
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
        let enc = FlatEnc::from_u8(t.enc).expect("schema and host ABI agree (handshake)");
        Some(match enc {
            FlatEnc::Str => Value::Str(self.tree.str_at(unsafe { p.cast::<FlatStr>().read() })),
            FlatEnc::SimpleSum => Value::SimpleSum(self.elem_ty, unsafe { p.cast::<u8>().read() }),
            FlatEnc::Int => Value::Int(unsafe { p.cast::<i32>().read() }),
            FlatEnc::Long => Value::Long(unsafe { p.cast::<i64>().read() }),
            FlatEnc::Byte | FlatEnc::Bool => Value::Byte(unsafe { p.cast::<u8>().read() }),
            FlatEnc::Node => Value::Node(Node {
                tree: self.tree,
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
    parse: fn(&str) -> Result<*mut FlatTree, String>,
    src: *const u8,
    src_len: usize,
    err_buf: *mut u8,
    err_buf_cap: usize,
    err_len: *mut usize,
) -> *mut FlatTree {
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
/// expands to a `static` [`FlatDialect`] plus
/// `pub extern "C" fn sql_dialect_snowflake() -> *const FlatDialect`. `parse`
/// is a `fn(&str) -> Result<*mut FlatTree, String>` — typically the lowering
/// plus [`TreeBuilder::finish`]. Per-dialect symbols (not a generic entry)
/// keep static linking possible. Requires the `builder` feature: the
/// descriptor references the library-side schema statics.
#[macro_export]
macro_rules! export_sql_dialect {
    ($entry:ident, $name:expr, $version:expr, $parse:expr) => {
        static DIALECT: $crate::FlatDialect = $crate::FlatDialect {
            abi_version: $crate::ABI_VERSION,
            schema_hash: SCHEMA_HASH,
            name: $crate::FlatName::from_static($name),
            dialect_version: $crate::FlatName::from_static($version),
            schema: &SCHEMA,
            parse: {
                unsafe extern "C" fn wrapped(
                    src: *const u8,
                    src_len: usize,
                    err_buf: *mut u8,
                    err_buf_cap: usize,
                    err_len: *mut usize,
                ) -> *mut $crate::FlatTree {
                    unsafe {
                        $crate::parse_entry($parse, src, src_len, err_buf, err_buf_cap, err_len)
                    }
                }
                wrapped
            },
        };

        /// The dialect descriptor entry point.
        #[unsafe(no_mangle)]
        pub extern "C" fn $entry() -> *const $crate::FlatDialect {
            &DIALECT
        }
    };
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
#[cfg(feature = "loader")]
pub mod loader {
    use std::path::Path;

    use super::{ABI_VERSION, ABI_VERSION_MIN, AbiError, FlatDialect, Tree, validate_schema};

    /// A loaded dialect library: its descriptor plus the parse entry point.
    ///
    /// The library is never unloaded (the load-once policy), so the descriptor
    /// and every tree it produces stay valid for the rest of the process.
    pub struct Dialect {
        descriptor: &'static FlatDialect,
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
                lib.get::<unsafe extern "C" fn() -> *const FlatDialect>(symbol.as_bytes())
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
        pub fn descriptor(&self) -> &FlatDialect {
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

#[cfg(feature = "loader")]
pub use loader::{Dialect, LoadError};
