#ifndef DBT_ASDL_FLAT_RUNTIME_HPP
#define DBT_ASDL_FLAT_RUNTIME_HPP

#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iterator>
#include <limits>
#include <optional>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

namespace dbt_asdl {

inline constexpr uint32_t RUNTIME_ABI_VERSION = 2;

inline constexpr uint32_t NONE_U32 = std::numeric_limits<uint32_t>::max();
inline constexpr uint16_t NONE_U16 = std::numeric_limits<uint16_t>::max();
inline constexpr uint8_t NONE_U8 = std::numeric_limits<uint8_t>::max();
inline constexpr int32_t NONE_I32 = std::numeric_limits<int32_t>::min();
inline constexpr int64_t NONE_I64 = std::numeric_limits<int64_t>::min();

inline constexpr uint16_t SECTION_STRINGS = 0;

struct FlatStr {
    uint32_t off;
    uint32_t len;

    static const FlatStr NONE;

    constexpr bool is_none() const noexcept { return off == NONE_U32; }

    constexpr bool operator==(const FlatStr& other) const noexcept {
        return off == other.off && len == other.len;
    }
    constexpr bool operator!=(const FlatStr& other) const noexcept { return !(*this == other); }
};

inline constexpr FlatStr FlatStr::NONE{NONE_U32, 0};

struct FlatSeq {
    uint32_t off;
    uint32_t len;

    constexpr bool operator==(const FlatSeq& other) const noexcept {
        return off == other.off && len == other.len;
    }
    constexpr bool operator!=(const FlatSeq& other) const noexcept { return !(*this == other); }
};

struct OptBool {
    uint8_t value;

    static const OptBool NONE;

    static constexpr OptBool from(std::optional<bool> v) noexcept {
        return v.has_value() ? OptBool{static_cast<uint8_t>(*v ? 1 : 0)} : OptBool{NONE_U8};
    }

    constexpr bool is_none() const noexcept { return value == NONE_U8; }

    constexpr std::optional<bool> get() const noexcept {
        if (value == NONE_U8) {
            return std::nullopt;
        }
        return value != 0;
    }

    constexpr bool operator==(const OptBool& other) const noexcept { return value == other.value; }
    constexpr bool operator!=(const OptBool& other) const noexcept { return value != other.value; }
};

inline constexpr OptBool OptBool::NONE{NONE_U8};

struct OptByte {
    uint8_t is_some;
    uint8_t value;

    static const OptByte NONE;

    static constexpr OptByte from(std::optional<uint8_t> v) noexcept {
        return v.has_value() ? OptByte{1, *v} : OptByte{0, 0};
    }

    constexpr bool is_none() const noexcept { return is_some == 0; }

    constexpr std::optional<uint8_t> get() const noexcept {
        if (is_some == 0) {
            return std::nullopt;
        }
        return value;
    }

    constexpr bool operator==(const OptByte& other) const noexcept {
        return is_some == other.is_some && value == other.value;
    }
    constexpr bool operator!=(const OptByte& other) const noexcept { return !(*this == other); }
};

inline constexpr OptByte OptByte::NONE{0, 0};

struct OptInt {
    int32_t value;

    static const OptInt NONE;

    static constexpr OptInt from(std::optional<int32_t> v) noexcept {
        if (v.has_value()) {
            assert(*v != NONE_I32 && "value collides with NONE sentinel");
            return OptInt{*v};
        }
        return OptInt{NONE_I32};
    }

    constexpr bool is_none() const noexcept { return value == NONE_I32; }

    constexpr std::optional<int32_t> get() const noexcept {
        if (value == NONE_I32) {
            return std::nullopt;
        }
        return value;
    }

    constexpr bool operator==(const OptInt& other) const noexcept { return value == other.value; }
    constexpr bool operator!=(const OptInt& other) const noexcept { return value != other.value; }
};

inline constexpr OptInt OptInt::NONE{NONE_I32};

struct OptLong {
    int64_t value;

    static const OptLong NONE;

    static constexpr OptLong from(std::optional<int64_t> v) noexcept {
        if (v.has_value()) {
            assert(*v != NONE_I64 && "value collides with NONE sentinel");
            return OptLong{*v};
        }
        return OptLong{NONE_I64};
    }

    constexpr bool is_none() const noexcept { return value == NONE_I64; }

    constexpr std::optional<int64_t> get() const noexcept {
        if (value == NONE_I64) {
            return std::nullopt;
        }
        return value;
    }

    constexpr bool operator==(const OptLong& other) const noexcept { return value == other.value; }
    constexpr bool operator!=(const OptLong& other) const noexcept { return value != other.value; }
};

inline constexpr OptLong OptLong::NONE{NONE_I64};

inline constexpr uint32_t SUM_PAYLOAD_BITS = 24;
inline constexpr uint32_t SUM_PAYLOAD_MASK = (1u << SUM_PAYLOAD_BITS) - 1u;

constexpr uint32_t pack_sum_ref(uint16_t tag, uint32_t payload) noexcept {
    assert(tag <= 0xFFu && "sum constructor tag exceeds 8 bits");
    assert(payload <= SUM_PAYLOAD_MASK && "sum payload index exceeds 24 bits");
    uint32_t packed =
        (static_cast<uint32_t>(tag) << SUM_PAYLOAD_BITS) | (payload & SUM_PAYLOAD_MASK);
    assert(packed != NONE_U32 && "packed sum reference collides with NONE_U32");
    return packed;
}

constexpr std::pair<uint16_t, uint32_t> unpack_sum_ref(uint32_t index) noexcept {
    uint16_t tag = static_cast<uint16_t>(index >> SUM_PAYLOAD_BITS);
    uint32_t payload = index & SUM_PAYLOAD_MASK;
    return {tag, payload};
}

struct FlatName {
    uint16_t off;
    uint16_t len;

    constexpr bool operator==(const FlatName& other) const noexcept {
        return off == other.off && len == other.len;
    }
    constexpr bool operator!=(const FlatName& other) const noexcept { return !(*this == other); }
};

enum class FlatEnc : uint8_t {
    Bool = 0,
    Byte = 1,
    Int = 2,
    Long = 3,
    SimpleSum = 4,
    Str = 5,
    Node = 6,
    OptBool = 7,
    OptByte = 8,
    OptInt = 9,
    OptLong = 10,
    OptSimpleSum = 11,
    OptStr = 12,
    OptNode = 13,
    Seq = 14,
};

constexpr std::optional<FlatEnc> flat_enc_from_u8(uint8_t v) noexcept {
    if (v <= static_cast<uint8_t>(FlatEnc::Seq)) {
        return static_cast<FlatEnc>(v);
    }
    return std::nullopt;
}

enum class FlatTypeKind : uint8_t {
    Builtin = 0,
    SimpleSum = 1,
    Product = 2,
    Sum = 3,
};

constexpr std::optional<FlatTypeKind> flat_type_kind_from_u8(uint8_t v) noexcept {
    if (v <= static_cast<uint8_t>(FlatTypeKind::Sum)) {
        return static_cast<FlatTypeKind>(v);
    }
    return std::nullopt;
}

struct FlatFieldInfo {
    FlatName name;
    uint16_t offset;
    uint16_t type_index;
    uint8_t enc;
};

struct FlatCtorInfo {
    FlatName name;
    uint16_t tag;
    uint16_t type_index;
    uint16_t payload_section;
    uint16_t field_off;
    uint16_t field_count;
};

struct FlatTypeInfo {
    FlatName name;
    uint8_t kind;
    uint8_t enc;
    uint16_t node_section;
    uint16_t seq_section;
    uint16_t ctor_off;
    uint16_t ctor_count;
    uint16_t field_off;
    uint16_t field_count;
};

struct FlatSchema {
    uint32_t runtime_abi_version;
    uint32_t abi_version;
    uint64_t schema_hash;
    uint32_t strings_len;
    uint16_t type_count;
    uint16_t ctor_count;
    uint16_t field_count;
    uint16_t root_type;
    const uint8_t* strings;
    const FlatTypeInfo* types;
    const FlatCtorInfo* ctors;
    const FlatFieldInfo* fields;

    std::string_view resolve_name(FlatName name) const noexcept {
        if (name.len == 0) {
            return std::string_view();
        }
        return std::string_view(reinterpret_cast<const char*>(strings + name.off), name.len);
    }
};

struct FlatSection {
    uint8_t* ptr;
    uint32_t len;
    uint32_t stride;
};

struct FlatTree {
    uint32_t section_count;
    uint32_t root_index;
    const FlatSchema* schema;
    FlatSection* sections;
    void (*release)(FlatTree*);
};

using FlatParseFn = FlatTree* (*)(
    const uint8_t* src,
    size_t src_len,
    uint8_t* err_buf,
    size_t err_buf_cap,
    size_t* err_len
);

struct FlatDialect {
    const uint8_t* name_ptr;
    const uint8_t* dialect_version_ptr;
    uint32_t name_len;
    uint32_t dialect_version_len;
    const FlatSchema* schema;
    FlatParseFn parse;

    std::string_view name() const noexcept {
        if (name_len == 0) {
            return std::string_view();
        }
        return std::string_view(reinterpret_cast<const char*>(name_ptr), name_len);
    }

    std::string_view dialect_version() const noexcept {
        if (dialect_version_len == 0) {
            return std::string_view();
        }
        return std::string_view(
            reinterpret_cast<const char*>(dialect_version_ptr),
            dialect_version_len
        );
    }
};

enum class AbiErrorKind : uint8_t {
    RuntimeAbiVersion,
    AbiVersion,
    NullTree,
    NullSchema,
};

struct AbiError {
    AbiErrorKind kind;
    uint32_t found = 0;
    uint32_t expected = 0;

    static constexpr AbiError runtime_abi_version(uint32_t found, uint32_t expected) noexcept {
        return AbiError{AbiErrorKind::RuntimeAbiVersion, found, expected};
    }

    static constexpr AbiError abi_version(uint32_t found, uint32_t expected) noexcept {
        return AbiError{AbiErrorKind::AbiVersion, found, expected};
    }

    static constexpr AbiError null_tree() noexcept {
        return AbiError{AbiErrorKind::NullTree, 0, 0};
    }

    static constexpr AbiError null_schema() noexcept {
        return AbiError{AbiErrorKind::NullSchema, 0, 0};
    }

    constexpr bool operator==(const AbiError& other) const noexcept {
        return kind == other.kind && found == other.found && expected == other.expected;
    }
    constexpr bool operator!=(const AbiError& other) const noexcept { return !(*this == other); }

    std::string message() const {
        switch (kind) {
            case AbiErrorKind::RuntimeAbiVersion:
                return "unsupported dbt-asdl runtime ABI version " + std::to_string(found) +
                       " (this host expects " + std::to_string(expected) + ")";
            case AbiErrorKind::AbiVersion:
                return "unsupported schema ABI version " + std::to_string(found) +
                       " (this host expects " + std::to_string(expected) + ")";
            case AbiErrorKind::NullTree:
                return "null tree";
            case AbiErrorKind::NullSchema:
                return "null schema";
        }
        return "unknown ABI error";
    }
};

class FieldSlice {
public:
    constexpr FieldSlice() noexcept : ptr_(nullptr), len_(0) {}
    constexpr FieldSlice(const FlatFieldInfo* ptr, size_t len) noexcept : ptr_(ptr), len_(len) {}

    constexpr const FlatFieldInfo* data() const noexcept { return ptr_; }
    constexpr size_t len() const noexcept { return len_; }
    constexpr size_t size() const noexcept { return len_; }
    constexpr bool is_empty() const noexcept { return len_ == 0; }
    constexpr bool empty() const noexcept { return len_ == 0; }

    constexpr const FlatFieldInfo& operator[](size_t i) const noexcept {
        assert(i < len_);
        return ptr_[i];
    }

    constexpr const FlatFieldInfo* begin() const noexcept { return ptr_; }
    constexpr const FlatFieldInfo* end() const noexcept { return ptr_ ? ptr_ + len_ : nullptr; }

private:
    const FlatFieldInfo* ptr_;
    size_t len_;
};

namespace detail {

template <typename T>
inline T read_unaligned(const uint8_t* p) noexcept {
    static_assert(std::is_trivially_copyable_v<T>);
    T val;
    std::memcpy(&val, p, sizeof(T));
    return val;
}

inline bool is_valid_utf8(const uint8_t* bytes, size_t len) noexcept {
    size_t i = 0;
    while (i < len) {
        uint8_t b0 = bytes[i];
        if (b0 < 0x80) {
            i += 1;
        } else if ((b0 >> 5) == 0x6) {
            if (i + 1 >= len) {
                return false;
            }
            uint8_t b1 = bytes[i + 1];
            if ((b1 & 0xC0) != 0x80 || (b0 & 0x1E) == 0) {
                return false;
            }
            i += 2;
        } else if ((b0 >> 4) == 0xE) {
            if (i + 2 >= len) {
                return false;
            }
            uint8_t b1 = bytes[i + 1];
            uint8_t b2 = bytes[i + 2];
            if ((b1 & 0xC0) != 0x80 || (b2 & 0xC0) != 0x80) {
                return false;
            }
            uint32_t cp = ((static_cast<uint32_t>(b0 & 0x0F)) << 12) |
                          ((static_cast<uint32_t>(b1 & 0x3F)) << 6) |
                          static_cast<uint32_t>(b2 & 0x3F);
            if (cp < 0x800 || (cp >= 0xD800 && cp <= 0xDFFF)) {
                return false;
            }
            i += 3;
        } else if ((b0 >> 3) == 0x1E) {
            if (i + 3 >= len) {
                return false;
            }
            uint8_t b1 = bytes[i + 1];
            uint8_t b2 = bytes[i + 2];
            uint8_t b3 = bytes[i + 3];
            if ((b1 & 0xC0) != 0x80 || (b2 & 0xC0) != 0x80 || (b3 & 0xC0) != 0x80) {
                return false;
            }
            uint32_t cp = ((static_cast<uint32_t>(b0 & 0x07)) << 18) |
                          ((static_cast<uint32_t>(b1 & 0x3F)) << 12) |
                          ((static_cast<uint32_t>(b2 & 0x3F)) << 6) |
                          static_cast<uint32_t>(b3 & 0x3F);
            if (cp < 0x10000 || cp > 0x10FFFF) {
                return false;
            }
            i += 4;
        } else {
            return false;
        }
    }
    return true;
}

inline std::optional<std::string_view> validate_name(
    const uint8_t* strings,
    uint32_t strings_len,
    FlatName name,
    std::string_view what,
    std::string* err_out
) {
    size_t start = name.off;
    size_t end = start + name.len;
    if (end > strings_len) {
        if (err_out) {
            *err_out = std::string(what) + ": name range " + std::to_string(start) + ".." +
                       std::to_string(end) + " out of bounds";
        }
        return std::nullopt;
    }
    if (name.len == 0) {
        return std::string_view();
    }
    if (!is_valid_utf8(strings + start, name.len)) {
        if (err_out) {
            *err_out = std::string(what) + ": name is not valid UTF-8";
        }
        return std::nullopt;
    }
    return std::string_view(reinterpret_cast<const char*>(strings + start), name.len);
}

inline bool validate_field_range(
    std::string_view owner,
    const uint8_t* strings,
    uint32_t strings_len,
    const FlatFieldInfo* fields,
    uint16_t total_fields,
    uint16_t field_off,
    uint16_t field_count,
    uint16_t type_count,
    std::string* err_out
) {
    size_t start = field_off;
    size_t end = start + field_count;
    if (end > total_fields) {
        if (err_out) {
            *err_out = std::string(owner) + ": field range " + std::to_string(start) + ".." +
                       std::to_string(end) + " out of bounds";
        }
        return false;
    }
    for (size_t i = start; i < end; ++i) {
        const FlatFieldInfo& f = fields[i];
        auto fname = validate_name(strings, strings_len, f.name, owner, err_out);
        if (!fname) {
            return false;
        }
        auto enc = flat_enc_from_u8(f.enc);
        if (!enc) {
            if (err_out) {
                *err_out = std::string(owner) + "." + std::string(*fname) +
                           ": unknown encoding " + std::to_string(f.enc);
            }
            return false;
        }
        bool references_type = (*enc == FlatEnc::SimpleSum || *enc == FlatEnc::Node ||
                                *enc == FlatEnc::OptSimpleSum || *enc == FlatEnc::OptNode ||
                                *enc == FlatEnc::Seq);
        if (references_type && f.type_index >= type_count) {
            if (err_out) {
                *err_out = std::string(owner) + "." + std::string(*fname) + ": type index " +
                           std::to_string(f.type_index) + " out of range";
            }
            return false;
        }
    }
    return true;
}

inline constexpr uint32_t SEQ_INDEX_BITS = 24;
inline constexpr uint64_t SEQ_INDEX_MASK = (uint64_t{1} << SEQ_INDEX_BITS) - 1;

constexpr uint64_t pack_seq_view(uint16_t elem_ty, FlatSeq seq) noexcept {
    assert(static_cast<uint64_t>(seq.off) <= SEQ_INDEX_MASK && "sequence offset exceeds 24 bits");
    assert(static_cast<uint64_t>(seq.len) <= SEQ_INDEX_MASK && "sequence length exceeds 24 bits");
    return (static_cast<uint64_t>(seq.off) & SEQ_INDEX_MASK) |
           ((static_cast<uint64_t>(seq.len) & SEQ_INDEX_MASK) << SEQ_INDEX_BITS) |
           (static_cast<uint64_t>(elem_ty) << (2 * SEQ_INDEX_BITS));
}

constexpr std::pair<uint16_t, FlatSeq> unpack_seq_view(uint64_t packed) noexcept {
    uint32_t off = static_cast<uint32_t>(packed & SEQ_INDEX_MASK);
    uint32_t len = static_cast<uint32_t>((packed >> SEQ_INDEX_BITS) & SEQ_INDEX_MASK);
    uint16_t elem_ty = static_cast<uint16_t>(packed >> (2 * SEQ_INDEX_BITS));
    return {elem_ty, FlatSeq{off, len}};
}

struct NodeRecord {
    const uint8_t* base;
    FieldSlice fields;
    const FlatCtorInfo* ctor;
};

inline NodeRecord node_record(const FlatTree* flat, uint16_t ty, uint32_t index) noexcept {
    const FlatSchema* s = flat->schema;
    const FlatTypeInfo& t = s->types[ty];
    const FlatFieldInfo* all_fields = s->fields;
    if (flat_type_kind_from_u8(t.kind) == FlatTypeKind::Product) {
        const FlatSection& sec = flat->sections[t.node_section];
        FieldSlice fields(
            t.field_count ? all_fields + t.field_off : nullptr,
            t.field_count
        );
        const uint8_t* ptr = sec.ptr ? sec.ptr + static_cast<size_t>(index) * sec.stride : nullptr;
        return NodeRecord{ptr, fields, nullptr};
    }
    auto [tag, payload] = unpack_sum_ref(index);
    const FlatCtorInfo& ctor = s->ctors[static_cast<size_t>(t.ctor_off) + tag];
    if (ctor.payload_section == NONE_U16) {
        return NodeRecord{nullptr, FieldSlice(), &ctor};
    }
    const FlatSection& psec = flat->sections[ctor.payload_section];
    FieldSlice fields(
        ctor.field_count ? all_fields + ctor.field_off : nullptr,
        ctor.field_count
    );
    const uint8_t* ptr =
        psec.ptr ? psec.ptr + static_cast<size_t>(payload) * psec.stride : nullptr;
    return NodeRecord{ptr, fields, &ctor};
}

inline std::string_view str_at(const FlatTree* flat, FlatStr s) noexcept {
    if (s.len == 0) {
        return std::string_view();
    }
    const FlatSection& sec = flat->sections[SECTION_STRINGS];
    return std::string_view(reinterpret_cast<const char*>(sec.ptr + s.off), s.len);
}

inline std::optional<std::string_view> simple_variant_name(
    const FlatTree* flat,
    uint16_t ty,
    uint8_t tag
) noexcept {
    const FlatSchema* s = flat->schema;
    if (ty >= s->type_count) {
        return std::nullopt;
    }
    const FlatTypeInfo& t = s->types[ty];
    if (static_cast<uint16_t>(tag) >= t.ctor_count) {
        return std::nullopt;
    }
    size_t idx = static_cast<size_t>(t.ctor_off) + tag;
    if (idx >= s->ctor_count) {
        return std::nullopt;
    }
    return s->resolve_name(s->ctors[idx].name);
}

} // namespace detail

inline bool validate_schema(
    const FlatSchema& schema,
    uint32_t abi_version,
    std::string* err_out = nullptr
) {
    if (schema.runtime_abi_version != RUNTIME_ABI_VERSION) {
        if (err_out) {
            *err_out = "schema runtime_abi_version " +
                       std::to_string(schema.runtime_abi_version) +
                       " does not match host RUNTIME_ABI_VERSION " +
                       std::to_string(RUNTIME_ABI_VERSION);
        }
        return false;
    }
    if (schema.abi_version != abi_version) {
        if (err_out) {
            *err_out = "schema abi_version " + std::to_string(schema.abi_version) +
                       " does not match host abi_version " + std::to_string(abi_version);
        }
        return false;
    }
    if (schema.strings_len > 0 && schema.strings == nullptr) {
        if (err_out) {
            *err_out = "null string table";
        }
        return false;
    }
    if (schema.type_count > 0 && schema.types == nullptr) {
        if (err_out) {
            *err_out = "null type table";
        }
        return false;
    }
    if (schema.ctor_count > 0 && schema.ctors == nullptr) {
        if (err_out) {
            *err_out = "null constructor table";
        }
        return false;
    }
    if (schema.field_count > 0 && schema.fields == nullptr) {
        if (err_out) {
            *err_out = "null field table";
        }
        return false;
    }
    if (schema.root_type >= schema.type_count) {
        if (err_out) {
            *err_out = "root type " + std::to_string(schema.root_type) + " out of range";
        }
        return false;
    }

    for (uint16_t i = 0; i < schema.type_count; ++i) {
        const FlatTypeInfo& t = schema.types[i];
        std::string what = "type[" + std::to_string(i) + "]";
        auto name = detail::validate_name(schema.strings, schema.strings_len, t.name, what, err_out);
        if (!name) {
            return false;
        }
        auto kind = flat_type_kind_from_u8(t.kind);
        if (!kind) {
            if (err_out) {
                *err_out = "type " + std::string(*name) + ": unknown kind " +
                           std::to_string(t.kind);
            }
            return false;
        }
        auto enc = flat_enc_from_u8(t.enc);
        if (!enc) {
            if (err_out) {
                *err_out = "type " + std::string(*name) + ": unknown encoding " +
                           std::to_string(t.enc);
            }
            return false;
        }
        bool enc_ok = false;
        if ((*kind == FlatTypeKind::Product || *kind == FlatTypeKind::Sum) &&
            *enc == FlatEnc::Node) {
            enc_ok = true;
        } else if (*kind == FlatTypeKind::SimpleSum && *enc == FlatEnc::SimpleSum) {
            enc_ok = true;
        } else if (*kind == FlatTypeKind::Builtin &&
                   (*enc == FlatEnc::Bool || *enc == FlatEnc::Byte || *enc == FlatEnc::Int ||
                    *enc == FlatEnc::Long || *enc == FlatEnc::Str)) {
            enc_ok = true;
        }
        if (!enc_ok) {
            if (err_out) {
                *err_out = "type " + std::string(*name) + ": incompatible kind and encoding";
            }
            return false;
        }
        if (*kind == FlatTypeKind::Product) {
            if (t.node_section == NONE_U16) {
                if (err_out) {
                    *err_out = "type " + std::string(*name) + ": product has no node section";
                }
                return false;
            }
            if (!detail::validate_field_range(
                    *name,
                    schema.strings,
                    schema.strings_len,
                    schema.fields,
                    schema.field_count,
                    t.field_off,
                    t.field_count,
                    schema.type_count,
                    err_out
                )) {
                return false;
            }
        }
        if (*kind == FlatTypeKind::SimpleSum || *kind == FlatTypeKind::Sum) {
            if (static_cast<uint32_t>(t.ctor_off) + static_cast<uint32_t>(t.ctor_count) >
                static_cast<uint32_t>(schema.ctor_count)) {
                if (err_out) {
                    *err_out =
                        "type " + std::string(*name) + ": constructor range out of bounds";
                }
                return false;
            }
            for (uint16_t j = 0; j < t.ctor_count; ++j) {
                const FlatCtorInfo& c = schema.ctors[static_cast<size_t>(t.ctor_off) + j];
                std::string cwhat =
                    "type " + std::string(*name) + " ctor[" + std::to_string(j) + "]";
                auto cname = detail::validate_name(
                    schema.strings,
                    schema.strings_len,
                    c.name,
                    cwhat,
                    err_out
                );
                if (!cname) {
                    return false;
                }
                if (c.type_index != i) {
                    if (err_out) {
                        *err_out = "constructor " + std::string(*cname) +
                                   ": lists owning type " + std::to_string(c.type_index);
                    }
                    return false;
                }
                if (c.tag != j) {
                    if (err_out) {
                        *err_out = "constructor " + std::string(*cname) + ": tag " +
                                   std::to_string(c.tag) + " != its position " +
                                   std::to_string(j);
                    }
                    return false;
                }
                if (!detail::validate_field_range(
                        *cname,
                        schema.strings,
                        schema.strings_len,
                        schema.fields,
                        schema.field_count,
                        c.field_off,
                        c.field_count,
                        schema.type_count,
                        err_out
                    )) {
                    return false;
                }
            }
        }
    }
    return true;
}

class TreeView;
class Tree;
class Node;
class SeqView;
class Value;

struct SimpleSumVal {
    uint16_t type_index;
    uint8_t tag;

    constexpr bool operator==(const SimpleSumVal& other) const noexcept {
        return type_index == other.type_index && tag == other.tag;
    }
    constexpr bool operator!=(const SimpleSumVal& other) const noexcept {
        return !(*this == other);
    }
};

class TreeView {
public:
    constexpr explicit TreeView(FlatTree* raw) noexcept : raw_(raw) {}

    constexpr FlatTree* raw() const noexcept { return raw_; }

    uint32_t runtime_abi_version() const noexcept { return raw_->schema->runtime_abi_version; }

    uint32_t abi_version() const noexcept { return raw_->schema->abi_version; }

    uint64_t schema_hash() const noexcept { return raw_->schema->schema_hash; }

    std::string_view resolve_name(FlatName name) const noexcept {
        return raw_->schema->resolve_name(name);
    }

    inline Node root() const noexcept;

    std::optional<uint16_t> type_by_name(std::string_view name) const noexcept {
        const FlatSchema* s = raw_->schema;
        for (uint16_t i = 0; i < s->type_count; ++i) {
            if (s->resolve_name(s->types[i].name) == name) {
                return i;
            }
        }
        return std::nullopt;
    }

    std::optional<uint32_t> kind_by_name(std::string_view ctor_name) const noexcept {
        const FlatSchema* s = raw_->schema;
        for (uint16_t i = 0; i < s->ctor_count; ++i) {
            if (s->resolve_name(s->ctors[i].name) == ctor_name) {
                return static_cast<uint32_t>(i);
            }
        }
        return std::nullopt;
    }

    std::optional<std::string_view> simple_variant_name(uint16_t ty, uint8_t tag) const noexcept {
        return detail::simple_variant_name(raw_, ty, tag);
    }

    std::optional<uint16_t> sum_tag(uint16_t ty, uint32_t index) const noexcept {
        const FlatTypeInfo& t = raw_->schema->types[ty];
        if (flat_type_kind_from_u8(t.kind) != FlatTypeKind::Sum) {
            return std::nullopt;
        }
        auto [tag, _] = unpack_sum_ref(index);
        return tag;
    }

    std::optional<std::pair<const uint8_t*, size_t>> product_ptr(
        uint16_t ty,
        uint32_t index
    ) const noexcept {
        const FlatTypeInfo& t = raw_->schema->types[ty];
        if (flat_type_kind_from_u8(t.kind) != FlatTypeKind::Product ||
            t.node_section == NONE_U16) {
            return std::nullopt;
        }
        const FlatSection& sec = raw_->sections[t.node_section];
        const uint8_t* ptr =
            sec.ptr ? sec.ptr + static_cast<size_t>(index) * sec.stride : nullptr;
        return std::pair<const uint8_t*, size_t>{ptr, static_cast<size_t>(sec.stride)};
    }

    std::optional<std::pair<const uint8_t*, size_t>> payload_ptr(
        uint16_t ty,
        uint32_t index,
        uint16_t tag
    ) const noexcept {
        const FlatSchema* s = raw_->schema;
        const FlatTypeInfo& t = s->types[ty];
        if (flat_type_kind_from_u8(t.kind) != FlatTypeKind::Sum) {
            return std::nullopt;
        }
        auto [node_tag, payload] = unpack_sum_ref(index);
        if (node_tag != tag) {
            return std::nullopt;
        }
        const FlatCtorInfo& ctor = s->ctors[static_cast<size_t>(t.ctor_off) + tag];
        if (ctor.payload_section == NONE_U16) {
            return std::nullopt;
        }
        const FlatSection& psec = raw_->sections[ctor.payload_section];
        const uint8_t* ptr =
            psec.ptr ? psec.ptr + static_cast<size_t>(payload) * psec.stride : nullptr;
        return std::pair<const uint8_t*, size_t>{ptr, static_cast<size_t>(psec.stride)};
    }

    template <typename T>
    const T* product(uint16_t ty, uint32_t index) const noexcept {
        auto res = product_ptr(ty, index);
        if (!res || res->second != sizeof(T)) {
            return nullptr;
        }
        return reinterpret_cast<const T*>(res->first);
    }

    template <typename T>
    const T* payload(uint16_t ty, uint32_t index, uint16_t tag) const noexcept {
        auto res = payload_ptr(ty, index, tag);
        if (!res || res->second != sizeof(T)) {
            return nullptr;
        }
        return reinterpret_cast<const T*>(res->first);
    }

    inline std::string to_sexpr() const;

protected:
    FlatTree* raw_;
};

class Tree final : public TreeView {
public:
    static std::optional<Tree> from_raw(FlatTree* raw, AbiError* err_out = nullptr) noexcept {
        if (raw == nullptr) {
            if (err_out) {
                *err_out = AbiError::null_tree();
            }
            return std::nullopt;
        }
        const FlatSchema* schema = raw->schema;
        if (schema == nullptr) {
            if (err_out) {
                *err_out = AbiError::null_schema();
            }
            return std::nullopt;
        }
        if (schema->runtime_abi_version != RUNTIME_ABI_VERSION) {
            if (err_out) {
                *err_out = AbiError::runtime_abi_version(
                    schema->runtime_abi_version,
                    RUNTIME_ABI_VERSION
                );
            }
            return std::nullopt;
        }
        return Tree(raw);
    }

    Tree(const Tree&) = delete;
    Tree& operator=(const Tree&) = delete;

    Tree(Tree&& other) noexcept : TreeView(other.raw_) { other.raw_ = nullptr; }

    Tree& operator=(Tree&& other) noexcept {
        if (this != &other) {
            if (raw_ != nullptr && raw_->release != nullptr) {
                raw_->release(raw_);
            }
            raw_ = other.raw_;
            other.raw_ = nullptr;
        }
        return *this;
    }

    ~Tree() {
        if (raw_ != nullptr && raw_->release != nullptr) {
            raw_->release(raw_);
        }
    }

    FlatTree* into_raw() noexcept {
        FlatTree* ptr = raw_;
        raw_ = nullptr;
        return ptr;
    }

private:
    explicit constexpr Tree(FlatTree* raw) noexcept : TreeView(raw) {}
};

class Node {
public:
    constexpr Node(FlatTree* raw, uint16_t ty, uint32_t index) noexcept
        : raw_(raw), ty_(ty), index_(index) {}

    constexpr TreeView tree() const noexcept { return TreeView(raw_); }

    constexpr uint16_t type_index() const noexcept { return ty_; }

    constexpr uint32_t index() const noexcept { return index_; }

    std::string_view type_name() const noexcept {
        return raw_->schema->resolve_name(raw_->schema->types[ty_].name);
    }

    std::optional<std::string_view> kind() const noexcept {
        auto rec = detail::node_record(raw_, ty_, index_);
        if (rec.ctor == nullptr) {
            return std::nullopt;
        }
        return raw_->schema->resolve_name(rec.ctor->name);
    }

    std::optional<uint32_t> kind_id() const noexcept {
        const FlatTypeInfo& t = raw_->schema->types[ty_];
        if (flat_type_kind_from_u8(t.kind) != FlatTypeKind::Sum) {
            return std::nullopt;
        }
        auto [tag, _] = unpack_sum_ref(index_);
        return static_cast<uint32_t>(t.ctor_off) + static_cast<uint32_t>(tag);
    }

    FieldSlice fields() const noexcept {
        return detail::node_record(raw_, ty_, index_).fields;
    }

    inline std::optional<Value> field(std::string_view name) const noexcept;

    inline std::optional<Value> field_at(size_t i) const noexcept;

    template <typename T>
    const T* as_product(std::string_view expected_type_name) const noexcept {
        auto ty = tree().type_by_name(expected_type_name);
        if (!ty || *ty != ty_) {
            return nullptr;
        }
        return tree().template product<T>(*ty, index_);
    }

    template <typename T>
    const T* as_payload(std::string_view expected_type_name, uint16_t tag) const noexcept {
        auto ty = tree().type_by_name(expected_type_name);
        if (!ty || *ty != ty_) {
            return nullptr;
        }
        return tree().template payload<T>(*ty, index_, tag);
    }

    inline std::string to_sexpr() const;

private:
    FlatTree* raw_;
    uint16_t ty_;
    uint32_t index_;
};

class SeqView {
public:
    class Iterator;

    constexpr SeqView(FlatTree* raw, uint16_t elem_ty, FlatSeq seq) noexcept
        : raw_(raw), packed_(detail::pack_seq_view(elem_ty, seq)) {}

    constexpr size_t len() const noexcept {
        return detail::unpack_seq_view(packed_).second.len;
    }

    constexpr size_t size() const noexcept { return len(); }

    constexpr bool is_empty() const noexcept { return len() == 0; }

    constexpr bool empty() const noexcept { return is_empty(); }

    constexpr uint16_t elem_type() const noexcept {
        return detail::unpack_seq_view(packed_).first;
    }

    inline std::optional<Value> get(size_t i) const noexcept;

    inline Value operator[](size_t i) const noexcept;

    inline Iterator begin() const noexcept;
    inline Iterator end() const noexcept;

private:
    constexpr size_t off() const noexcept {
        return detail::unpack_seq_view(packed_).second.off;
    }

    FlatTree* raw_;
    uint64_t packed_;
};

enum class ValueKind : uint8_t {
    None,
    Bool,
    Byte,
    Int,
    Long,
    Str,
    SimpleSum,
    Node,
    Seq,
};

class Value {
public:
    static constexpr Value none() noexcept { return Value(); }

    static constexpr Value from_bool(bool v) noexcept {
        Value out;
        out.kind_ = ValueKind::Bool;
        out.payload_.bool_val = v;
        return out;
    }

    static constexpr Value from_byte(uint8_t v) noexcept {
        Value out;
        out.kind_ = ValueKind::Byte;
        out.payload_.byte_val = v;
        return out;
    }

    static constexpr Value from_int(int32_t v) noexcept {
        Value out;
        out.kind_ = ValueKind::Int;
        out.payload_.int_val = v;
        return out;
    }

    static constexpr Value from_long(int64_t v) noexcept {
        Value out;
        out.kind_ = ValueKind::Long;
        out.payload_.long_val = v;
        return out;
    }

    static constexpr Value from_str(std::string_view v) noexcept {
        Value out;
        out.kind_ = ValueKind::Str;
        out.payload_.str_val = v;
        return out;
    }

    static constexpr Value from_simple_sum(uint16_t ty, uint8_t tag) noexcept {
        Value out;
        out.kind_ = ValueKind::SimpleSum;
        out.payload_.simple_sum_val = SimpleSumVal{ty, tag};
        return out;
    }

    static constexpr Value from_node(Node v) noexcept {
        Value out;
        out.kind_ = ValueKind::Node;
        out.payload_.node_val = v;
        return out;
    }

    static constexpr Value from_seq(SeqView v) noexcept {
        Value out;
        out.kind_ = ValueKind::Seq;
        out.payload_.seq_val = v;
        return out;
    }

    constexpr ValueKind kind() const noexcept { return kind_; }

    constexpr bool is_none() const noexcept { return kind_ == ValueKind::None; }
    constexpr bool is_bool() const noexcept { return kind_ == ValueKind::Bool; }
    constexpr bool is_byte() const noexcept { return kind_ == ValueKind::Byte; }
    constexpr bool is_int() const noexcept { return kind_ == ValueKind::Int; }
    constexpr bool is_long() const noexcept { return kind_ == ValueKind::Long; }
    constexpr bool is_str() const noexcept { return kind_ == ValueKind::Str; }
    constexpr bool is_simple_sum() const noexcept { return kind_ == ValueKind::SimpleSum; }
    constexpr bool is_node() const noexcept { return kind_ == ValueKind::Node; }
    constexpr bool is_seq() const noexcept { return kind_ == ValueKind::Seq; }

    constexpr std::optional<bool> as_bool() const noexcept {
        if (kind_ != ValueKind::Bool) {
            return std::nullopt;
        }
        return payload_.bool_val;
    }

    constexpr std::optional<uint8_t> as_byte() const noexcept {
        if (kind_ != ValueKind::Byte) {
            return std::nullopt;
        }
        return payload_.byte_val;
    }

    constexpr std::optional<int32_t> as_int() const noexcept {
        if (kind_ != ValueKind::Int) {
            return std::nullopt;
        }
        return payload_.int_val;
    }

    constexpr std::optional<int64_t> as_long() const noexcept {
        if (kind_ != ValueKind::Long) {
            return std::nullopt;
        }
        return payload_.long_val;
    }

    constexpr std::optional<std::string_view> as_str() const noexcept {
        if (kind_ != ValueKind::Str) {
            return std::nullopt;
        }
        return payload_.str_val;
    }

    constexpr std::optional<SimpleSumVal> as_simple_sum() const noexcept {
        if (kind_ != ValueKind::SimpleSum) {
            return std::nullopt;
        }
        return payload_.simple_sum_val;
    }

    constexpr std::optional<Node> as_node() const noexcept {
        if (kind_ != ValueKind::Node) {
            return std::nullopt;
        }
        return payload_.node_val;
    }

    constexpr std::optional<SeqView> as_seq() const noexcept {
        if (kind_ != ValueKind::Seq) {
            return std::nullopt;
        }
        return payload_.seq_val;
    }

private:
    constexpr Value() noexcept : kind_(ValueKind::None), payload_() {}

    ValueKind kind_;
    union Payload {
        bool bool_val;
        uint8_t byte_val;
        int32_t int_val;
        int64_t long_val;
        std::string_view str_val;
        SimpleSumVal simple_sum_val;
        Node node_val;
        SeqView seq_val;

        constexpr Payload() noexcept : long_val(0) {}
    } payload_;
};

namespace detail {

inline Value read_field(
    FlatTree* flat,
    const uint8_t* base,
    const FlatFieldInfo& f
) noexcept {
    const uint8_t* p = base + f.offset;
    auto enc = flat_enc_from_u8(f.enc);
    assert(enc.has_value() && "schema and host ABI agree (handshake)");
    switch (*enc) {
        case FlatEnc::Bool:
            return Value::from_bool(read_unaligned<uint8_t>(p) != 0);
        case FlatEnc::Byte:
            return Value::from_byte(read_unaligned<uint8_t>(p));
        case FlatEnc::Int:
            return Value::from_int(read_unaligned<int32_t>(p));
        case FlatEnc::Long:
            return Value::from_long(read_unaligned<int64_t>(p));
        case FlatEnc::SimpleSum:
            return Value::from_simple_sum(f.type_index, read_unaligned<uint8_t>(p));
        case FlatEnc::Str:
            return Value::from_str(str_at(flat, read_unaligned<FlatStr>(p)));
        case FlatEnc::Node:
            return Value::from_node(Node(flat, f.type_index, read_unaligned<uint32_t>(p)));
        case FlatEnc::OptBool: {
            auto v = read_unaligned<OptBool>(p).get();
            return v.has_value() ? Value::from_bool(*v) : Value::none();
        }
        case FlatEnc::OptByte: {
            auto v = read_unaligned<OptByte>(p).get();
            return v.has_value() ? Value::from_byte(*v) : Value::none();
        }
        case FlatEnc::OptInt: {
            auto v = read_unaligned<OptInt>(p).get();
            return v.has_value() ? Value::from_int(*v) : Value::none();
        }
        case FlatEnc::OptLong: {
            auto v = read_unaligned<OptLong>(p).get();
            return v.has_value() ? Value::from_long(*v) : Value::none();
        }
        case FlatEnc::OptSimpleSum: {
            uint8_t v = read_unaligned<uint8_t>(p);
            return v == NONE_U8 ? Value::none() : Value::from_simple_sum(f.type_index, v);
        }
        case FlatEnc::OptStr: {
            FlatStr s = read_unaligned<FlatStr>(p);
            return s.is_none() ? Value::none() : Value::from_str(str_at(flat, s));
        }
        case FlatEnc::OptNode: {
            uint32_t idx = read_unaligned<uint32_t>(p);
            return idx == NONE_U32 ? Value::none() : Value::from_node(Node(flat, f.type_index, idx));
        }
        case FlatEnc::Seq:
            return Value::from_seq(SeqView(flat, f.type_index, read_unaligned<FlatSeq>(p)));
    }
    return Value::none();
}

} // namespace detail

inline Node TreeView::root() const noexcept {
    return Node(raw_, raw_->schema->root_type, raw_->root_index);
}

inline std::optional<Value> Node::field(std::string_view name) const noexcept {
    auto rec = detail::node_record(raw_, ty_, index_);
    const FlatSchema* s = raw_->schema;
    for (const FlatFieldInfo& f : rec.fields) {
        if (s->resolve_name(f.name) == name) {
            return detail::read_field(raw_, rec.base, f);
        }
    }
    return std::nullopt;
}

inline std::optional<Value> Node::field_at(size_t i) const noexcept {
    auto rec = detail::node_record(raw_, ty_, index_);
    if (i >= rec.fields.len()) {
        return std::nullopt;
    }
    return detail::read_field(raw_, rec.base, rec.fields[i]);
}

inline std::optional<Value> SeqView::get(size_t i) const noexcept {
    if (i >= len()) {
        return std::nullopt;
    }
    uint16_t elem_ty = elem_type();
    const FlatTypeInfo& t = raw_->schema->types[elem_ty];
    const FlatSection& sec = raw_->sections[t.seq_section];
    const uint8_t* p = sec.ptr + (off() + i) * sec.stride;
    auto enc = flat_enc_from_u8(t.enc);
    assert(enc.has_value() && "schema and host ABI agree (handshake)");
    switch (*enc) {
        case FlatEnc::Str:
            return Value::from_str(detail::str_at(raw_, detail::read_unaligned<FlatStr>(p)));
        case FlatEnc::SimpleSum:
            return Value::from_simple_sum(elem_ty, detail::read_unaligned<uint8_t>(p));
        case FlatEnc::Int:
            return Value::from_int(detail::read_unaligned<int32_t>(p));
        case FlatEnc::Long:
            return Value::from_long(detail::read_unaligned<int64_t>(p));
        case FlatEnc::Bool:
            return Value::from_bool(detail::read_unaligned<uint8_t>(p) != 0);
        case FlatEnc::Byte:
            return Value::from_byte(detail::read_unaligned<uint8_t>(p));
        case FlatEnc::Node:
            return Value::from_node(Node(raw_, elem_ty, detail::read_unaligned<uint32_t>(p)));
        default:
            assert(false && "element encodings are never optional");
            return std::nullopt;
    }
}

inline Value SeqView::operator[](size_t i) const noexcept {
    auto v = get(i);
    assert(v.has_value());
    return *v;
}

class SeqView::Iterator {
public:
    using iterator_category = std::input_iterator_tag;
    using value_type = Value;
    using difference_type = std::ptrdiff_t;
    using pointer = void;
    using reference = Value;

    constexpr Iterator(SeqView seq, size_t index) noexcept : seq_(seq), index_(index) {}

    Value operator*() const noexcept { return seq_[index_]; }

    Iterator& operator++() noexcept {
        ++index_;
        return *this;
    }

    Iterator operator++(int) noexcept {
        Iterator tmp = *this;
        ++(*this);
        return tmp;
    }

    constexpr bool operator==(const Iterator& other) const noexcept {
        return index_ == other.index_;
    }
    constexpr bool operator!=(const Iterator& other) const noexcept {
        return index_ != other.index_;
    }

private:
    SeqView seq_;
    size_t index_;
};

inline SeqView::Iterator SeqView::begin() const noexcept { return Iterator(*this, 0); }

inline SeqView::Iterator SeqView::end() const noexcept { return Iterator(*this, len()); }

namespace detail {

inline std::string camel_sexpr(std::string_view name) {
    std::string out;
    out.reserve(name.size());
    bool cap_next = true;
    for (char c : name) {
        if (c == '_') {
            cap_next = true;
        } else if (cap_next) {
            if (c >= 'a' && c <= 'z') {
                out.push_back(static_cast<char>(c - 'a' + 'A'));
            } else {
                out.push_back(c);
            }
            cap_next = false;
        } else {
            out.push_back(c);
        }
    }
    return out;
}

inline void write_escaped(std::string_view s, std::string& out) {
    out.push_back('"');
    for (unsigned char c : s) {
        switch (c) {
            case '"':
                out.append("\\\"");
                break;
            case '\\':
                out.append("\\\\");
                break;
            case '\n':
                out.append("\\n");
                break;
            case '\r':
                out.append("\\r");
                break;
            case '\t':
                out.append("\\t");
                break;
            default:
                if (c < 0x20) {
                    char buf[16];
                    std::snprintf(buf, sizeof(buf), "\\u{%x}", static_cast<unsigned int>(c));
                    out.append(buf);
                } else {
                    out.push_back(static_cast<char>(c));
                }
                break;
        }
    }
    out.push_back('"');
}

inline void write_value(const FlatTree* flat, Value value, std::string& out);

inline void write_node(Node node, std::string& out) {
    auto ctor = node.kind();
    bool is_sum = ctor.has_value();
    std::string name_owned;
    std::string_view name;
    if (ctor.has_value()) {
        name = *ctor;
    } else {
        name_owned = camel_sexpr(node.type_name());
        name = name_owned;
    }

    FieldSlice fields = node.fields();
    FlatTree* flat = node.tree().raw();
    const FlatSchema* s = flat->schema;

    std::vector<std::pair<size_t, Value>> rendered;
    for (size_t i = 0; i < fields.len(); ++i) {
        std::string_view fname = s->resolve_name(fields[i].name);
        if (fname.substr(0, 4) == "span") {
            continue;
        }
        Value val = *node.field_at(i);
        bool elided = false;
        if (val.is_none()) {
            elided = true;
        } else if (val.is_bool() && !*val.as_bool() &&
                   fields[i].enc == static_cast<uint8_t>(FlatEnc::Bool)) {
            elided = true;
        } else if (val.is_seq() && val.as_seq()->is_empty()) {
            elided = true;
        }
        if (!elided) {
            rendered.emplace_back(i, val);
        }
    }

    if (is_sum && rendered.empty()) {
        out.append(name);
        return;
    }

    out.push_back('(');
    out.append(name);
    for (const auto& [i, val] : rendered) {
        out.append(" (");
        out.append(s->resolve_name(fields[i].name));
        out.push_back(' ');
        write_value(flat, val, out);
        out.push_back(')');
    }
    out.push_back(')');
}

inline void write_value(const FlatTree* flat, Value value, std::string& out) {
    switch (value.kind()) {
        case ValueKind::None:
            out.append("None");
            break;
        case ValueKind::Bool:
            out.append(*value.as_bool() ? "true" : "false");
            break;
        case ValueKind::Byte:
            out.append(std::to_string(static_cast<int64_t>(*value.as_byte())));
            break;
        case ValueKind::Int:
            out.append(std::to_string(*value.as_int()));
            break;
        case ValueKind::Long:
            out.append(std::to_string(*value.as_long()));
            break;
        case ValueKind::Str:
            write_escaped(*value.as_str(), out);
            break;
        case ValueKind::SimpleSum: {
            auto ss = *value.as_simple_sum();
            auto vname = simple_variant_name(flat, ss.type_index, ss.tag);
            assert(vname.has_value());
            out.append(*vname);
            break;
        }
        case ValueKind::Node:
            write_node(*value.as_node(), out);
            break;
        case ValueKind::Seq: {
            SeqView seq = *value.as_seq();
            out.push_back('[');
            for (size_t i = 0; i < seq.len(); ++i) {
                if (i > 0) {
                    out.push_back(' ');
                }
                write_value(flat, seq[i], out);
            }
            out.push_back(']');
            break;
        }
    }
}

} // namespace detail

inline std::string TreeView::to_sexpr() const {
    std::string out;
    detail::write_node(root(), out);
    return out;
}

inline std::string Node::to_sexpr() const {
    std::string out;
    detail::write_node(*this, out);
    return out;
}

static_assert(sizeof(OptBool) == 1 && alignof(OptBool) == 1);
static_assert(sizeof(OptByte) == 2 && alignof(OptByte) == 1);
static_assert(sizeof(OptInt) == 4 && alignof(OptInt) == 4);
static_assert(sizeof(OptLong) == 8 && alignof(OptLong) == alignof(int64_t));
static_assert(sizeof(FlatStr) == 8 && alignof(FlatStr) == 4);
static_assert(sizeof(FlatSeq) == 8 && alignof(FlatSeq) == 4);
static_assert(sizeof(FlatName) == 4 && alignof(FlatName) == 2);
static_assert(sizeof(FlatFieldInfo) == 10 && alignof(FlatFieldInfo) == 2);
static_assert(sizeof(FlatCtorInfo) == 14 && alignof(FlatCtorInfo) == 2);
static_assert(sizeof(FlatTypeInfo) == 18 && alignof(FlatTypeInfo) == 2);

static_assert(sizeof(void*) != 8 || (sizeof(FlatSection) == 16 && alignof(FlatSection) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(FlatTree) == 32 && alignof(FlatTree) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(FlatDialect) == 40 && alignof(FlatDialect) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(FlatSchema) == 64 && alignof(FlatSchema) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(TreeView) == 8 && alignof(TreeView) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(Tree) == 8 && alignof(Tree) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(Node) == 16 && alignof(Node) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(SeqView) == 16 && alignof(SeqView) == 8));
static_assert(sizeof(void*) != 8 || (sizeof(Value) == 24 && alignof(Value) == 8));

} // namespace dbt_asdl

#endif // DBT_ASDL_FLAT_RUNTIME_HPP
