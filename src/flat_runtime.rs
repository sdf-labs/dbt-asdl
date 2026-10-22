use std::fmt::Write as _;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::sexpr::write_escaped;

/// The C++17 header source for the host-side [`Tree`] wrapper of [`FlatTree`].
pub const FLAT_RUNTIME_HPP: &str = include_str!("../include/dbt_asdl/flat_runtime.hpp");

/// Writes `<include_dir>/dbt_asdl/flat_runtime.hpp` and returns the file path.
///
/// Pass `include_dir` to the C++ compiler include path so that C++ sources
/// can use `#include <dbt_asdl/flat_runtime.hpp>`.
///
/// # Examples
///
/// ```
/// let include_dir = std::env::temp_dir().join(format!(
///     "dbt_asdl_cpp_doc_{}",
///     std::process::id()
/// ));
/// let header = dbt_asdl::codegen::cpp::write_runtime_header(&include_dir)?;
/// assert_eq!(header, include_dir.join("dbt_asdl/flat_runtime.hpp"));
/// assert_eq!(std::fs::read_to_string(&header)?, dbt_asdl::FLAT_RUNTIME_HPP);
/// std::fs::remove_dir_all(&include_dir)?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn write_runtime_header(
    include_dir: impl AsRef<std::path::Path>,
) -> std::io::Result<std::path::PathBuf> {
    let sub_dir = include_dir.as_ref().join("dbt_asdl");
    std::fs::create_dir_all(&sub_dir)?;
    let header_path = sub_dir.join("flat_runtime.hpp");
    std::fs::write(&header_path, FLAT_RUNTIME_HPP)?;
    Ok(header_path)
}

/// Layout version of the `dbt-asdl` boundary descriptor and tree-record structs
/// (`FlatDialect`, `FlatSchema`, `FlatTree`, `FlatSection`, `FlatStr`,
/// `FlatSeq`, and `Opt*`). Checked at load time before any schema table or
/// schema-specific `abi_version` is read.
pub const RUNTIME_ABI_VERSION: u32 = 2;

/// The `None` sentinel for optional node references and optional string
/// offsets.
pub const NONE_U32: u32 = u32::MAX;
/// The `None` sentinel for 16-bit schema table and section indices.
pub const NONE_U16: u16 = u16::MAX;
/// The `None` sentinel for [`OptBool`] and optional simple-sum values.
pub const NONE_U8: u8 = u8::MAX;
/// The `None` sentinel for [`OptInt`].
pub const NONE_I32: i32 = i32::MIN;
/// The `None` sentinel for [`OptLong`].
pub const NONE_I64: i64 = i64::MIN;

/// By convention, section 0 of every tree is the UTF-8 string blob.
pub const SECTION_STRINGS: u16 = 0;

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

/// An optional boolean value in the flat C ABI layout.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptBool {
    /// The encoded boolean value (`0` for `false`, `1` for `true`, or [`NONE_U8`] for `None`).
    pub value: u8,
}

impl OptBool {
    /// The `None` sentinel for an optional boolean field.
    pub const NONE: Self = OptBool { value: NONE_U8 };

    /// Returns `true` if this value holds the `None` sentinel.
    pub fn is_none(self) -> bool {
        self.value == NONE_U8
    }

    /// Returns `Some(bool)` if a value is present, or `None` if this value is `None`.
    pub fn get(self) -> Option<bool> {
        (self.value != NONE_U8).then_some(self.value != 0)
    }
}

impl From<Option<bool>> for OptBool {
    fn from(v: Option<bool>) -> Self {
        match v {
            Some(value) => OptBool { value: value as u8 },
            None => OptBool::NONE,
        }
    }
}

/// An optional byte (`u8`) value in the flat C ABI layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptByte {
    /// A non-zero value indicates that a byte value is present. Zero indicates `None`.
    pub is_some: u8,
    /// The byte payload when `is_some` is not zero.
    pub value: u8,
}

impl OptByte {
    /// The `None` value for an optional byte field.
    pub const NONE: Self = OptByte {
        is_some: 0,
        value: 0,
    };

    /// Returns `true` if this value is `None`.
    pub fn is_none(self) -> bool {
        self.is_some == 0
    }

    /// Returns `Some(value)` if `is_some` is not zero, or `None` if `is_some` is zero.
    pub fn get(self) -> Option<u8> {
        (self.is_some != 0).then_some(self.value)
    }
}

impl From<Option<u8>> for OptByte {
    fn from(v: Option<u8>) -> Self {
        match v {
            Some(value) => OptByte { is_some: 1, value },
            None => OptByte::NONE,
        }
    }
}

/// An optional 32-bit signed integer (`i32`) value in the flat C ABI layout.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptInt {
    /// The integer payload, or [`NONE_I32`] for `None`.
    pub value: i32,
}

/// An optional 64-bit signed integer (`i64`) value in the flat C ABI layout.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptLong {
    /// The 64-bit integer payload, or [`NONE_I64`] for `None`.
    pub value: i64,
}

macro_rules! opt_scalar {
    ($t:ident, $v:ty, $none:ident) => {
        impl $t {
            /// The `None` sentinel for this optional scalar type.
            pub const NONE: Self = $t { value: $none };

            /// Returns `true` if this value holds the `None` sentinel.
            pub fn is_none(self) -> bool {
                self.value == $none
            }

            /// Returns `Some(value)` if a value is present, or `None` if this value is `None`.
            pub fn get(self) -> Option<$v> {
                (self.value != $none).then_some(self.value)
            }
        }

        impl From<Option<$v>> for $t {
            fn from(v: Option<$v>) -> Self {
                match v {
                    Some(value) => {
                        debug_assert_ne!(value, $none, "value collides with NONE sentinel");
                        $t { value }
                    }
                    None => $t::NONE,
                }
            }
        }
    };
}

opt_scalar!(OptInt, i32, NONE_I32);
opt_scalar!(OptLong, i64, NONE_I64);

/// The number of low bits that store the payload index in a packed sum reference.
pub const SUM_PAYLOAD_BITS: u32 = 24;
/// The bitmask for the payload index in a packed sum reference.
pub const SUM_PAYLOAD_MASK: u32 = (1 << SUM_PAYLOAD_BITS) - 1;

/// Packs a constructor tag and payload index into one `u32` sum reference.
pub const fn pack_sum_ref(tag: u16, payload: u32) -> u32 {
    debug_assert!(tag <= 0xFF, "sum constructor tag exceeds 8 bits");
    debug_assert!(payload <= SUM_PAYLOAD_MASK, "sum payload index exceeds 24 bits");
    let packed = ((tag as u32) << SUM_PAYLOAD_BITS) | (payload & SUM_PAYLOAD_MASK);
    debug_assert!(packed != NONE_U32, "packed sum reference collides with NONE_U32");
    packed
}

/// Unpacks a `u32` sum reference into its `(tag, payload)` pair.
pub const fn unpack_sum_ref(index: u32) -> (u16, u32) {
    let tag = (index >> SUM_PAYLOAD_BITS) as u16;
    let payload = index & SUM_PAYLOAD_MASK;
    (tag, payload)
}

// ---------------------------------------------------------------------------
// The descriptor types: the dialect library's metadata and the schema-as-data
// the host's introspection reads.
// ---------------------------------------------------------------------------

/// A string slice in the schema string table (`FlatSchema::strings`), stored as `{off, len}`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatName {
    /// Byte offset into `FlatSchema::strings`.
    pub off: u16,
    /// Byte length of the UTF-8 string.
    pub len: u16,
}

/// Encoding descriptor for the built-in types.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatFieldInfo {
    /// Field name in the ASDL specification.
    pub name: FlatName,
    /// Byte offset of the field within its record.
    pub offset: u16,
    /// The referenced/element type, for `SimpleSum`/`Node`/`OptNode`/`Seq`/
    /// `OptSimpleSum` encodings; [`NONE_U16`] otherwise.
    pub type_index: u16,
    /// A [`FlatEnc`] value.
    pub enc: u8,
}

/// One constructor of a sum (compound or simple).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatCtorInfo {
    /// Constructor name in the ASDL specification.
    pub name: FlatName,
    /// The tag value within its sum.
    pub tag: u16,
    /// The owning sum's index in the type table.
    pub type_index: u16,
    /// The section holding this constructor's payload records, or [`NONE_U16`].
    pub payload_section: u16,
    /// Start index in `FlatSchema::fields` for this constructor's fields.
    pub field_off: u16,
    /// Number of fields for this constructor.
    pub field_count: u16,
}

/// One type of the schema.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatTypeInfo {
    /// Type name in the ASDL specification.
    pub name: FlatName,
    /// A [`FlatTypeKind`] value.
    pub kind: u8,
    /// The [`FlatEnc`] of a value of this type when it appears inline or as a
    /// sequence element (`Node` for products and compound sums, `Str` for
    /// `identifier`/`string`, the scalar encodings for the rest).
    pub enc: u8,
    /// The section that holds this product type's records, or [`NONE_U16`] for non-product types.
    pub node_section: u16,
    /// The section holding this type's sequence pool, or [`NONE_U16`] if the type
    /// never appears as `T*`.
    pub seq_section: u16,
    /// For sums: the constructors are `ctors[ctor_off .. ctor_off +
    /// ctor_count]`, tags indexing within.
    pub ctor_off: u16,
    /// Number of constructors for sum types, or `0` for products and builtins.
    pub ctor_count: u16,
    /// For products: start index in `FlatSchema::fields` for this product's fields.
    pub field_off: u16,
    /// Number of fields for product types, or `0` otherwise.
    pub field_count: u16,
}

/// The schema of the exported ASDL tree.
///
/// Allows runtime introspection of library-constructed trees from the host
/// binary, using the [`Tree`] API. Embedded as static data on the library side.
#[repr(C)]
pub struct FlatSchema {
    /// Layout version of the `dbt-asdl` boundary descriptors (`RUNTIME_ABI_VERSION`).
    pub runtime_abi_version: u32,
    /// Schema ABI version from the `.asdl` specification (`ABI_VERSION`).
    pub abi_version: u32,
    /// 64-bit FNV-1a hash of the source `.asdl` bytes.
    pub schema_hash: u64,
    /// Byte length of `strings`.
    pub strings_len: u32,
    /// Number of entries in `types`.
    pub type_count: u16,
    /// Number of entries in `ctors`.
    pub ctor_count: u16,
    /// Number of entries in `fields`.
    pub field_count: u16,
    /// The type index of the root (`file`).
    pub root_type: u16,
    /// Pointer to the schema's static UTF-8 string pool.
    pub strings: *const u8,
    /// Pointer to the schema's type descriptor table.
    pub types: *const FlatTypeInfo,
    /// Pointer to the schema's constructor descriptor table.
    pub ctors: *const FlatCtorInfo,
    /// Pointer to the schema's field descriptor table.
    pub fields: *const FlatFieldInfo,
}

impl FlatSchema {
    /// Resolves `name` to a string slice in this schema's string pool.
    ///
    /// # Safety
    /// This schema must have passed [`validate_schema`], and `name` must be in bounds of
    /// `self.strings`.
    pub unsafe fn resolve_name(&self, name: FlatName) -> &str {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.strings.add(name.off as usize),
                name.len as usize,
            ))
        }
    }
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
    /// Number of sections in `sections`.
    pub section_count: u32,
    /// The root node's index in the schema's `root_type` section.
    pub root_index: u32,
    /// Pointer to the static schema descriptor of the library.
    pub schema: *const FlatSchema,
    /// Pointer to the section array of length `section_count`.
    pub sections: *mut FlatSection,
    /// Library-side destructor: frees the tree and every section.
    pub release: unsafe extern "C" fn(*mut FlatTree),
}

/// A loaded dialect library's descriptor: metadata + entry points.
#[repr(C)]
pub struct FlatDialect {
    /// Pointer to the static UTF-8 bytes of the dialect identifier name.
    pub name_ptr: *const u8,
    /// Pointer to the static UTF-8 bytes of the dialect library version string.
    pub dialect_version_ptr: *const u8,
    /// Byte length of the dialect identifier name.
    pub name_len: u32,
    /// Byte length of the dialect library version string.
    pub dialect_version_len: u32,
    /// Pointer to the static schema descriptor of the library.
    pub schema: *const FlatSchema,
    /// Synchronous parse entry point of the dialect library.
    pub parse: FlatParseFn,
}

impl FlatDialect {
    /// Returns the dialect identifier name.
    pub fn name(&self) -> &str {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.name_ptr,
                self.name_len as usize,
            ))
        }
    }

    /// Returns the dialect library version string.
    pub fn dialect_version(&self) -> &str {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                self.dialect_version_ptr,
                self.dialect_version_len as usize,
            ))
        }
    }
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
unsafe impl Sync for FlatSchema {}
unsafe impl Sync for FlatDialect {}

// ---------------------------------------------------------------------------
// Host-side introspection
// ---------------------------------------------------------------------------

/// Why a tree or descriptor was refused at the boundary.
#[derive(Debug, PartialEq, Eq)]
pub enum AbiError {
    /// `runtime_abi_version` does not match [`RUNTIME_ABI_VERSION`].
    RuntimeAbiVersion {
        /// Runtime ABI version reported by the library or tree.
        found: u32,
        /// Runtime ABI version expected by this `dbt-asdl` build.
        expected: u32,
    },
    /// Schema `abi_version` does not match the host's expected `ABI_VERSION`.
    AbiVersion {
        /// Schema ABI version reported by the library or tree.
        found: u32,
        /// Schema ABI version expected by the host.
        expected: u32,
    },
    /// The `*mut FlatTree` pointer was null.
    NullTree,
    /// The `*const FlatSchema` pointer was null.
    NullSchema,
}

impl std::fmt::Display for AbiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbiError::RuntimeAbiVersion { found, expected } => write!(
                f,
                "unsupported dbt-asdl runtime ABI version {found} (this host expects {expected})"
            ),
            AbiError::AbiVersion { found, expected } => {
                write!(f, "unsupported schema ABI version {found} (this host expects {expected})")
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
/// second half (the first being the `abi_version` check).
///
/// # Safety
/// `schema` must be the descriptor of a dialect library whose
/// `runtime_abi_version` matches [`RUNTIME_ABI_VERSION`]; the tables are read
/// in place from the library's memory.
pub unsafe fn validate_schema(schema: &FlatSchema, abi_version: u32) -> Result<(), String> {
    if schema.runtime_abi_version != RUNTIME_ABI_VERSION {
        return Err(format!(
            "schema runtime_abi_version {} does not match host RUNTIME_ABI_VERSION {RUNTIME_ABI_VERSION}",
            schema.runtime_abi_version
        ));
    }
    if schema.abi_version != abi_version {
        return Err(format!(
            "schema abi_version {} does not match host abi_version {abi_version}",
            schema.abi_version
        ));
    }
    if schema.strings_len > 0 && schema.strings.is_null() {
        return Err("null string table".to_string());
    }
    if schema.type_count > 0 && schema.types.is_null() {
        return Err("null type table".to_string());
    }
    if schema.ctor_count > 0 && schema.ctors.is_null() {
        return Err("null constructor table".to_string());
    }
    if schema.field_count > 0 && schema.fields.is_null() {
        return Err("null field table".to_string());
    }
    if schema.root_type >= schema.type_count {
        return Err(format!("root type {} out of range", schema.root_type));
    }
    let strings = unsafe { table(schema.strings, schema.strings_len) };
    let types = unsafe { table(schema.types, schema.type_count as u32) };
    let ctors = unsafe { table(schema.ctors, schema.ctor_count as u32) };
    let fields = unsafe { table(schema.fields, schema.field_count as u32) };
    for (i, t) in types.iter().enumerate() {
        let name = validate_name(strings, t.name, &format!("type[{i}]"))?;
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
        if kind == FlatTypeKind::Product {
            if t.node_section == NONE_U16 {
                return Err(format!("type {name}: product has no node section"));
            }
            validate_field_range(
                name,
                strings,
                fields,
                t.field_off,
                t.field_count,
                schema.type_count,
            )?;
        }
        if kind == FlatTypeKind::SimpleSum || kind == FlatTypeKind::Sum {
            if (t.ctor_off as u32) + (t.ctor_count as u32) > (schema.ctor_count as u32) {
                return Err(format!("type {name}: constructor range out of bounds"));
            }
            for j in 0..t.ctor_count {
                let c = &ctors[(t.ctor_off + j) as usize];
                let cname = validate_name(strings, c.name, &format!("type {name} ctor[{j}]"))?;
                if c.type_index != i as u16 {
                    return Err(format!("constructor {cname}: lists owning type {}", c.type_index));
                }
                if c.tag != j {
                    return Err(format!("constructor {cname}: tag {} != its position {j}", c.tag));
                }
                validate_field_range(
                    cname,
                    strings,
                    fields,
                    c.field_off,
                    c.field_count,
                    schema.type_count,
                )?;
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

fn validate_name<'a>(strings: &'a [u8], name: FlatName, what: &str) -> Result<&'a str, String> {
    let start = name.off as usize;
    let end = start + name.len as usize;
    let bytes = strings
        .get(start..end)
        .ok_or_else(|| format!("{what}: name range {start}..{end} out of bounds"))?;
    std::str::from_utf8(bytes).map_err(|e| format!("{what}: name is not valid UTF-8: {e}"))
}

/// Validates one contiguous slice of `fields` belonging to a product or constructor.
fn validate_field_range(
    owner: &str,
    strings: &[u8],
    fields: &[FlatFieldInfo],
    field_off: u16,
    field_count: u16,
    type_count: u16,
) -> Result<(), String> {
    let start = field_off as usize;
    let end = start + field_count as usize;
    let owner_fields = fields
        .get(start..end)
        .ok_or_else(|| format!("{owner}: field range {start}..{end} out of bounds"))?;
    for f in owner_fields {
        let fname = validate_name(strings, f.name, owner)?;
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
        if references_type && f.type_index >= type_count {
            return Err(format!("{owner}.{fname}: type index {} out of range", f.type_index));
        }
    }
    Ok(())
}

impl FlatTree {
    fn schema(&self) -> &FlatSchema {
        unsafe { &*self.schema }
    }

    fn types(&self) -> &[FlatTypeInfo] {
        let s = self.schema();
        unsafe { table(s.types, s.type_count as u32) }
    }

    fn ctors(&self) -> &[FlatCtorInfo] {
        let s = self.schema();
        unsafe { table(s.ctors, s.ctor_count as u32) }
    }

    fn fields(&self) -> &[FlatFieldInfo] {
        let s = self.schema();
        unsafe { table(s.fields, s.field_count as u32) }
    }

    fn type_info(&self, ty: u16) -> &FlatTypeInfo {
        &self.types()[ty as usize]
    }

    fn section(&self, i: u16) -> (usize, usize) {
        let s = unsafe { &*self.sections.add(i as usize) };
        (s.ptr as usize, s.stride as usize)
    }

    fn resolve_name(&self, name: FlatName) -> &str {
        unsafe { self.schema().resolve_name(name) }
    }

    fn str_at(&self, s: FlatStr) -> &str {
        let (base, _) = self.section(SECTION_STRINGS);
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                (base + s.off as usize) as *const u8,
                s.len as usize,
            ))
        }
    }

    fn simple_variant_name(&self, ty: u16, tag: u8) -> Option<&str> {
        let t = self.type_info(ty);
        let ctor = self.ctors().get(t.ctor_off as usize + tag as usize)?;
        (tag as u16)
            .lt(&t.ctor_count)
            .then(|| self.resolve_name(ctor.name))
    }

    /// The record base pointer of a node: `(record, fields, ctor)` — the
    /// payload address, its field table, and (for sums) the constructor.
    fn node_record(
        &self,
        ty: u16,
        index: u32,
    ) -> (*const u8, &[FlatFieldInfo], Option<&FlatCtorInfo>) {
        let t = self.type_info(ty);
        let all_fields = self.fields();
        match FlatTypeKind::from_u8(t.kind) {
            Some(FlatTypeKind::Product) => {
                let (base, stride) = self.section(t.node_section);
                let start = t.field_off as usize;
                let fields = &all_fields[start..start + t.field_count as usize];
                ((base + index as usize * stride) as *const u8, fields, None)
            }
            _ => {
                let (tag, payload) = unpack_sum_ref(index);
                let ctor = &self.ctors()[t.ctor_off as usize + tag as usize];
                if ctor.payload_section == NONE_U16 {
                    return (std::ptr::null(), &[], Some(ctor));
                }
                let (pbase, pstride) = self.section(ctor.payload_section);
                let start = ctor.field_off as usize;
                let fields = &all_fields[start..start + ctor.field_count as usize];
                ((pbase + payload as usize * pstride) as *const u8, fields, Some(ctor))
            }
        }
    }

    fn read_field(&self, base: *const u8, f: &FlatFieldInfo) -> Value<'_> {
        let raw = NonNull::from(self);
        let p = unsafe { base.add(f.offset as usize) };
        let enc = FlatEnc::from_u8(f.enc).expect("schema and host ABI agree (handshake)");
        match enc {
            FlatEnc::Bool => Value::Bool(unsafe { p.cast::<u8>().read() } != 0),
            FlatEnc::Byte => Value::Byte(unsafe { p.cast::<u8>().read() }),
            FlatEnc::Int => Value::Int(unsafe { p.cast::<i32>().read() }),
            FlatEnc::Long => Value::Long(unsafe { p.cast::<i64>().read() }),
            FlatEnc::SimpleSum => Value::SimpleSum(f.type_index, unsafe { p.cast::<u8>().read() }),
            FlatEnc::Str => Value::Str(self.str_at(unsafe { p.cast::<FlatStr>().read() })),
            FlatEnc::Node => {
                Value::Node(Node::new(raw, f.type_index, unsafe { p.cast::<u32>().read() }))
            }
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
                    Value::Node(Node::new(raw, f.type_index, index))
                }
            }
            FlatEnc::Seq => {
                Value::Seq(SeqView::new(raw, f.type_index, unsafe { p.cast::<FlatSeq>().read() }))
            }
        }
    }
}

/// A library-owned flat tree under host management.
///
/// This wrapper reads the tree in place and frees it through the header's
/// `release` function on drop.
#[repr(transparent)]
pub struct Tree {
    raw: NonNull<FlatTree>,
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
    /// the schema header.
    ///
    /// # Safety
    /// `raw` must be a valid `*mut FlatTree` produced by a library whose
    /// schema lies within this host's supported ABI range.
    pub unsafe fn from_raw(raw: *mut FlatTree) -> Result<Tree, AbiError> {
        let Some(raw) = NonNull::new(raw) else {
            return Err(AbiError::NullTree);
        };
        let schema = unsafe { (*raw.as_ptr()).schema };
        if schema.is_null() {
            return Err(AbiError::NullSchema);
        }
        let runtime_abi = unsafe { (*schema).runtime_abi_version };
        if runtime_abi != RUNTIME_ABI_VERSION {
            return Err(AbiError::RuntimeAbiVersion {
                found: runtime_abi,
                expected: RUNTIME_ABI_VERSION,
            });
        }
        Ok(Tree { raw })
    }

    fn flat(&self) -> &FlatTree {
        unsafe { self.raw.as_ref() }
    }

    /// Returns the `dbt-asdl` runtime ABI version from the tree's schema header.
    pub fn runtime_abi_version(&self) -> u32 {
        self.flat().schema().runtime_abi_version
    }

    /// Returns the schema ABI version from the tree's schema header.
    pub fn abi_version(&self) -> u32 {
        self.flat().schema().abi_version
    }

    /// Returns the 64-bit FNV-1a schema hash from the tree's schema header.
    pub fn schema_hash(&self) -> u64 {
        self.flat().schema().schema_hash
    }

    /// Resolves a [`FlatName`] in this tree's schema string pool.
    pub fn resolve_name(&self, name: FlatName) -> &str {
        self.flat().resolve_name(name)
    }

    /// The root node (the `file`).
    pub fn root(&self) -> Node<'_> {
        let flat = self.flat();
        Node::new(self.raw, flat.schema().root_type, flat.root_index)
    }

    /// A type's index in the schema, by name. Linear scan over the type
    /// table: resolve once, then keep the index.
    pub fn type_by_name(&self, name: &str) -> Option<u16> {
        let flat = self.flat();
        flat.types()
            .iter()
            .position(|t| flat.resolve_name(t.name) == name)
            .map(|i| i as u16)
    }

    /// A constructor's global index in the schema, by name. Constructor
    /// names are unique across the schema (the ASDL check enforces it), so
    /// one name identifies one (sum, tag) pair. Resolve once, then keep
    /// the index.
    pub fn kind_by_name(&self, ctor_name: &str) -> Option<u32> {
        let flat = self.flat();
        flat.ctors()
            .iter()
            .position(|c| flat.resolve_name(c.name) == ctor_name)
            .map(|i| i as u32)
    }

    /// The variant name of a simple-sum value, for renderings.
    pub fn simple_variant_name(&self, ty: u16, tag: u8) -> Option<&str> {
        self.flat().simple_variant_name(ty, tag)
    }

    /// The node's constructor tag within its sum, for typed projections.
    pub fn sum_tag(&self, ty: u16, index: u32) -> Option<u16> {
        let t = self.flat().type_info(ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Sum) {
            return None;
        }
        let (tag, _) = unpack_sum_ref(index);
        Some(tag)
    }

    /// (record pointer, section stride) of a product node, for typed
    /// projections. None when `ty` is not a product.
    pub fn product_ptr(&self, ty: u16, index: u32) -> Option<(*const u8, usize)> {
        let flat = self.flat();
        let t = flat.type_info(ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Product)
            || t.node_section == NONE_U16
        {
            return None;
        }
        let (base, stride) = flat.section(t.node_section);
        Some(((base + index as usize * stride) as *const u8, stride))
    }

    /// (payload pointer, section stride) of a sum node's payload when its
    /// tag is `tag`, for typed projections.
    pub fn payload_ptr(&self, ty: u16, index: u32, tag: u16) -> Option<(*const u8, usize)> {
        let flat = self.flat();
        let t = flat.type_info(ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Sum) {
            return None;
        }
        let (node_tag, payload) = unpack_sum_ref(index);
        if node_tag != tag {
            return None;
        }
        let ctor = &flat.ctors()[t.ctor_off as usize + tag as usize];
        if ctor.payload_section == NONE_U16 {
            return None;
        }
        let (pbase, pstride) = flat.section(ctor.payload_section);
        Some(((pbase + payload as usize * pstride) as *const u8, pstride))
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
        let raw = self.raw.as_ptr();
        unsafe { ((*raw).release)(raw) }
    }
}

/// A by-value cursor into a library-owned tree.
///
/// This cursor stores the direct `FlatTree` pointer, the schema type index,
/// and the node index. It is `Copy` and stays valid for the lifetime of the
/// tree.
#[derive(Clone, Copy)]
pub struct Node<'t> {
    raw: NonNull<FlatTree>,
    ty: u16,
    index: u32,
    _marker: PhantomData<&'t Tree>,
}

impl<'t> Node<'t> {
    fn new(raw: NonNull<FlatTree>, ty: u16, index: u32) -> Self {
        Node {
            raw,
            ty,
            index,
            _marker: PhantomData,
        }
    }

    fn flat(&self) -> &'t FlatTree {
        unsafe { self.raw.as_ref() }
    }

    /// Returns the tree that this cursor reads.
    pub fn tree(&self) -> &'t Tree {
        unsafe { &*(&raw const self.raw).cast::<Tree>() }
    }

    /// The node's type index in the schema.
    pub fn type_index(&self) -> u16 {
        self.ty
    }

    /// The node's index in its type's node section.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The node's type name (`expr`, `struct_field`, ...).
    pub fn type_name(&self) -> &'t str {
        let flat = self.flat();
        flat.resolve_name(flat.type_info(self.ty).name)
    }

    /// The node's constructor name, for sum nodes; None for products.
    pub fn kind(&self) -> Option<&'t str> {
        let flat = self.flat();
        let (_, _, ctor) = flat.node_record(self.ty, self.index);
        ctor.map(|c| flat.resolve_name(c.name))
    }

    /// The node's constructor as a global schema index (compare against
    /// [`Tree::kind_by_name`]); None for products.
    pub fn kind_id(&self) -> Option<u32> {
        let t = self.flat().type_info(self.ty);
        if FlatTypeKind::from_u8(t.kind) != Some(FlatTypeKind::Sum) {
            return None;
        }
        let (tag, _) = unpack_sum_ref(self.index);
        Some(t.ctor_off as u32 + tag as u32)
    }

    /// The node's fields, as raw descriptors (name + encoding + offset).
    pub fn fields(&self) -> &'t [FlatFieldInfo] {
        let (_, fields, _) = self.flat().node_record(self.ty, self.index);
        fields
    }

    /// The field named `name`, or None when the node's constructor has no
    /// such field (skew included).
    pub fn field(&self, name: &str) -> Option<Value<'t>> {
        let flat = self.flat();
        let (base, fields, _) = flat.node_record(self.ty, self.index);
        let f = fields.iter().find(|f| flat.resolve_name(f.name) == name)?;
        Some(flat.read_field(base, f))
    }

    /// The `i`th field, or None when out of range.
    pub fn field_at(&self, i: usize) -> Option<Value<'t>> {
        let flat = self.flat();
        let (base, fields, _) = flat.node_record(self.ty, self.index);
        let f = fields.get(i)?;
        Some(flat.read_field(base, f))
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

const SEQ_INDEX_BITS: u32 = 24;
const SEQ_INDEX_MASK: u64 = (1 << SEQ_INDEX_BITS) - 1;

const fn pack_seq_view(elem_ty: u16, seq: FlatSeq) -> u64 {
    debug_assert!((seq.off as u64) <= SEQ_INDEX_MASK, "sequence offset exceeds 24 bits");
    debug_assert!((seq.len as u64) <= SEQ_INDEX_MASK, "sequence length exceeds 24 bits");
    (seq.off as u64 & SEQ_INDEX_MASK)
        | ((seq.len as u64 & SEQ_INDEX_MASK) << SEQ_INDEX_BITS)
        | ((elem_ty as u64) << (2 * SEQ_INDEX_BITS))
}

const fn unpack_seq_view(packed: u64) -> (u16, FlatSeq) {
    let off = (packed & SEQ_INDEX_MASK) as u32;
    let len = ((packed >> SEQ_INDEX_BITS) & SEQ_INDEX_MASK) as u32;
    let elem_ty = (packed >> (2 * SEQ_INDEX_BITS)) as u16;
    (elem_ty, FlatSeq { off, len })
}

/// A lazy view over the elements of a sequence field.
///
/// This view packs the 24-bit pool offset, the 24-bit element count, and the
/// 16-bit element type index into one `u64` word. It also stores the direct
/// `FlatTree` pointer.
#[derive(Clone, Copy)]
pub struct SeqView<'t> {
    raw: NonNull<FlatTree>,
    packed: u64,
    _marker: PhantomData<&'t Tree>,
}

impl<'t> SeqView<'t> {
    fn new(raw: NonNull<FlatTree>, elem_ty: u16, seq: FlatSeq) -> Self {
        SeqView {
            raw,
            packed: pack_seq_view(elem_ty, seq),
            _marker: PhantomData,
        }
    }

    fn flat(&self) -> &'t FlatTree {
        unsafe { self.raw.as_ref() }
    }

    fn off(&self) -> usize {
        let (_, seq) = unpack_seq_view(self.packed);
        seq.off as usize
    }

    /// Returns the number of elements in the sequence.
    pub fn len(&self) -> usize {
        let (_, seq) = unpack_seq_view(self.packed);
        seq.len as usize
    }

    /// Returns `true` if the sequence contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element type's index in the schema.
    pub fn elem_type(&self) -> u16 {
        let (elem_ty, _) = unpack_seq_view(self.packed);
        elem_ty
    }

    /// The `i`th element, or None when out of bounds.
    pub fn get(&self, i: usize) -> Option<Value<'t>> {
        if i >= self.len() {
            return None;
        }
        let flat = self.flat();
        let elem_ty = self.elem_type();
        let t = flat.type_info(elem_ty);
        let (base, stride) = flat.section(t.seq_section);
        let p = (base + (self.off() + i) * stride) as *const u8;
        let enc = FlatEnc::from_u8(t.enc).expect("schema and host ABI agree (handshake)");
        Some(match enc {
            FlatEnc::Str => Value::Str(flat.str_at(unsafe { p.cast::<FlatStr>().read() })),
            FlatEnc::SimpleSum => Value::SimpleSum(elem_ty, unsafe { p.cast::<u8>().read() }),
            FlatEnc::Int => Value::Int(unsafe { p.cast::<i32>().read() }),
            FlatEnc::Long => Value::Long(unsafe { p.cast::<i64>().read() }),
            FlatEnc::Bool => Value::Bool(unsafe { p.cast::<u8>().read() } != 0),
            FlatEnc::Byte => Value::Byte(unsafe { p.cast::<u8>().read() }),
            FlatEnc::Node => {
                Value::Node(Node::new(self.raw, elem_ty, unsafe { p.cast::<u32>().read() }))
            }
            _ => unreachable!("element encodings are never optional"),
        })
    }

    /// Iterate the elements.
    pub fn iter(&self) -> impl Iterator<Item = Value<'t>> {
        let len = self.len();
        (0..len).map(move |i| self.get(i).expect("in bounds"))
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
            name_ptr: ($name).as_ptr(),
            dialect_version_ptr: ($version).as_ptr(),
            name_len: ($name).len() as u32,
            dialect_version_len: ($version).len() as u32,
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
    let flat = node.flat();

    let printable: Vec<usize> = (0..fields.len())
        .filter(|&i| !flat.resolve_name(fields[i].name).starts_with("span"))
        .collect();

    let mut rendered: Vec<(usize, Value)> = Vec::new();
    for &i in &printable {
        let value = node.field_at(i).expect("index in range");
        let elided = match &value {
            Value::None => true,
            Value::Bool(b) => !b && fields[i].enc == FlatEnc::Bool as u8,
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
    for (i, value) in rendered {
        out.push_str(" (");
        out.push_str(flat.resolve_name(fields[i].name));
        out.push(' ');
        write_value(flat, value, out);
        out.push(')');
    }
    out.push(')');
}

fn write_value(flat: &FlatTree, value: Value, out: &mut String) {
    match value {
        Value::None => out.push_str("None"),
        Value::Bool(b) => out.push_str(if b { "true" } else { "false" }),
        Value::Byte(v) => push_int(v as i64, out),
        Value::Int(v) => push_int(v as i64, out),
        Value::Long(v) => push_int(v, out),
        Value::Str(s) => write_escaped(s, out),
        Value::SimpleSum(ty, tag) => {
            let name = flat.simple_variant_name(ty, tag).expect("tag in range");
            out.push_str(name);
        }
        Value::Node(n) => write_node(n, out),
        Value::Seq(s) => {
            out.push('[');
            for (i, item) in s.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(flat, item, out);
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

    use super::{AbiError, FlatDialect, FlatSchema, RUNTIME_ABI_VERSION, Tree, validate_schema};
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
        pub unsafe fn load(
            path: &Path,
            name: &str,
            abi_version: u32,
        ) -> Result<Dialect, LoadError> {
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
            if descriptor.schema.is_null() {
                return Err(LoadError::Abi(AbiError::NullSchema));
            }
            let schema = unsafe { &*descriptor.schema };
            if schema.runtime_abi_version != RUNTIME_ABI_VERSION {
                return Err(LoadError::Abi(AbiError::RuntimeAbiVersion {
                    found: schema.runtime_abi_version,
                    expected: RUNTIME_ABI_VERSION,
                }));
            }
            if schema.abi_version != abi_version {
                return Err(LoadError::Abi(AbiError::AbiVersion {
                    found: schema.abi_version,
                    expected: abi_version,
                }));
            }
            unsafe { validate_schema(schema, abi_version) }.map_err(LoadError::BadSchema)?;
            std::mem::forget(lib);
            Ok(Dialect { descriptor })
        }

        /// The library's descriptor.
        pub fn descriptor(&self) -> &FlatDialect {
            self.descriptor
        }

        /// The library's static schema descriptor.
        pub fn schema(&self) -> &FlatSchema {
            unsafe { &*self.descriptor.schema }
        }

        /// Returns the `dbt-asdl` runtime ABI version of the loaded library.
        pub fn runtime_abi_version(&self) -> u32 {
            self.schema().runtime_abi_version
        }

        /// Returns the schema ABI version of the loaded library.
        pub fn abi_version(&self) -> u32 {
            self.schema().abi_version
        }

        /// The dialect's name.
        pub fn name(&self) -> &str {
            self.descriptor.name()
        }

        /// The dialect library's own version string.
        pub fn dialect_version(&self) -> &str {
            self.descriptor.dialect_version()
        }

        /// The hash of the `sql.asdl` the library was generated from.
        pub fn schema_hash(&self) -> u64 {
            self.schema().schema_hash
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
            let tree =
                unsafe { Tree::from_raw(raw) }.expect("the library passed the load-time handshake");
            assert_eq!(tree.abi_version(), self.abi_version());
            Ok(tree)
        }
    }
}

#[cfg(feature = "loader")]
pub use loader::{Dialect, LoadError};

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::{
        FlatCtorInfo, FlatDialect, FlatFieldInfo, FlatName, FlatSchema, FlatSection, FlatSeq,
        FlatTree, FlatTypeInfo, NONE_I32, NONE_I64, NONE_U8, Node, OptBool, OptByte, OptInt,
        OptLong, SEQ_INDEX_MASK, SUM_PAYLOAD_MASK, SeqView, Tree, Value, pack_seq_view,
        pack_sum_ref, unpack_seq_view, unpack_sum_ref,
    };

    #[test]
    fn opt_scalar_layouts_and_roundtrips() {
        assert_eq!(size_of::<OptBool>(), 1);
        assert_eq!(align_of::<OptBool>(), 1);
        assert_eq!(OptBool::NONE.value, NONE_U8);
        assert!(OptBool::NONE.is_none());
        assert_eq!(OptBool::NONE.get(), None);
        assert_eq!(OptBool::from(None), OptBool::NONE);
        assert_eq!(OptBool::from(Some(false)).get(), Some(false));
        assert_eq!(OptBool::from(Some(true)).get(), Some(true));
        assert!(!OptBool::from(Some(false)).is_none());
        assert!(!OptBool::from(Some(true)).is_none());

        assert_eq!(size_of::<OptByte>(), 2);
        assert_eq!(align_of::<OptByte>(), 1);
        assert!(OptByte::NONE.is_none());
        assert_eq!(OptByte::NONE.get(), None);
        assert_eq!(OptByte::from(None), OptByte::NONE);
        for b in [0u8, 1, 64, 127, 254, u8::MAX] {
            let opt = OptByte::from(Some(b));
            assert!(!opt.is_none());
            assert_eq!(opt.get(), Some(b));
        }

        assert_eq!(size_of::<OptInt>(), 4);
        assert_eq!(align_of::<OptInt>(), 4);
        assert_eq!(OptInt::NONE.value, NONE_I32);
        assert!(OptInt::NONE.is_none());
        assert_eq!(OptInt::NONE.get(), None);
        assert_eq!(OptInt::from(None), OptInt::NONE);
        for v in [i32::MIN + 1, -1, 0, 1, 42, i32::MAX] {
            let opt = OptInt::from(Some(v));
            assert!(!opt.is_none());
            assert_eq!(opt.get(), Some(v));
        }

        assert_eq!(size_of::<OptLong>(), 8);
        assert_eq!(align_of::<OptLong>(), align_of::<i64>());
        assert_eq!(OptLong::NONE.value, NONE_I64);
        assert!(OptLong::NONE.is_none());
        assert_eq!(OptLong::NONE.get(), None);
        assert_eq!(OptLong::from(None), OptLong::NONE);
        for v in [i64::MIN + 1, -1, 0, 1, 42, i64::MAX] {
            let opt = OptLong::from(Some(v));
            assert!(!opt.is_none());
            assert_eq!(opt.get(), Some(v));
        }
    }

    #[test]
    fn sum_ref_pack_and_unpack() {
        for (tag, payload) in [
            (0u16, 0u32),
            (0, 1),
            (1, 42),
            (7, SUM_PAYLOAD_MASK),
            (254, SUM_PAYLOAD_MASK),
            (255, 0),
            (255, SUM_PAYLOAD_MASK - 1),
        ] {
            let packed = pack_sum_ref(tag, payload);
            assert_eq!(unpack_sum_ref(packed), (tag, payload));
        }
    }

    #[test]
    fn seq_view_pack_and_unpack() {
        let max_idx = SEQ_INDEX_MASK as u32;
        for (elem_ty, off, len) in [
            (0u16, 0u32, 0u32),
            (1, 42, 7),
            (12, max_idx, 0),
            (255, 0, max_idx),
            (u16::MAX, max_idx, max_idx),
        ] {
            let seq = FlatSeq { off, len };
            let packed = pack_seq_view(elem_ty, seq);
            assert_eq!(unpack_seq_view(packed), (elem_ty, seq));
        }
    }

    #[test]
    fn descriptor_and_header_layouts() {
        assert_eq!(size_of::<FlatName>(), 4);
        assert_eq!(align_of::<FlatName>(), 2);

        assert_eq!(size_of::<FlatFieldInfo>(), 10);
        assert_eq!(align_of::<FlatFieldInfo>(), 2);

        assert_eq!(size_of::<FlatCtorInfo>(), 14);
        assert_eq!(align_of::<FlatCtorInfo>(), 2);

        assert_eq!(size_of::<FlatTypeInfo>(), 18);
        assert_eq!(align_of::<FlatTypeInfo>(), 2);

        if size_of::<*const u8>() == 8 {
            assert_eq!(size_of::<FlatSection>(), 16);
            assert_eq!(align_of::<FlatSection>(), 8);

            assert_eq!(size_of::<FlatTree>(), 32);
            assert_eq!(align_of::<FlatTree>(), 8);

            assert_eq!(size_of::<FlatDialect>(), 40);
            assert_eq!(align_of::<FlatDialect>(), 8);

            assert_eq!(size_of::<FlatSchema>(), 64);
            assert_eq!(align_of::<FlatSchema>(), 8);

            assert_eq!(size_of::<Tree>(), 8);
            assert_eq!(align_of::<Tree>(), 8);
            assert_eq!(size_of::<Option<Tree>>(), 8);

            assert_eq!(size_of::<Node<'_>>(), 16);
            assert_eq!(align_of::<Node<'_>>(), 8);
            assert_eq!(size_of::<Option<Node<'_>>>(), 16);

            assert_eq!(size_of::<SeqView<'_>>(), 16);
            assert_eq!(align_of::<SeqView<'_>>(), 8);
            assert_eq!(size_of::<Option<SeqView<'_>>>(), 16);

            assert_eq!(size_of::<Value<'_>>(), 24);
            assert_eq!(align_of::<Value<'_>>(), 8);
        }
    }
}
