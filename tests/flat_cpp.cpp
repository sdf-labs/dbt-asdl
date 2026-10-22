#include "flat_runtime.hpp"

#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>

namespace {

using namespace dbt_asdl;

struct CppDataTypeInt {
    uint32_t span;
    uint8_t nullable;
    OptBool unsigned_;
    OptByte width;
};

struct CppDataTypeBool {
    uint32_t span;
    uint8_t nullable;
};

struct CppWhenThen {
    uint32_t when;
    uint32_t then;
    uint8_t op;
};

static_assert(sizeof(CppDataTypeInt) == 8 && alignof(CppDataTypeInt) == 4);
static_assert(sizeof(CppDataTypeBool) == 8 && alignof(CppDataTypeBool) == 4);
static_assert(sizeof(CppWhenThen) == 12 && alignof(CppWhenThen) == 4);

std::optional<Node> child_node(Node node, std::string_view field_name) {
    auto v = node.field(field_name);
    if (!v) {
        return std::nullopt;
    }
    return v->as_node();
}

void write_out_str(const std::string& s, uint8_t* buf, size_t cap, size_t* out_len) {
    size_t n = s.size() < cap ? s.size() : cap;
    if (n > 0 && buf != nullptr) {
        std::memcpy(buf, s.data(), n);
    }
    if (out_len != nullptr) {
        *out_len = n;
    }
}

struct WalkStats {
    uint32_t node_count = 0;
    uint32_t ident_part_count = 0;
    int64_t number_sum = 0;
    uint32_t binary_op_count = 0;
    uint32_t cast_count = 0;
    uint32_t when_then_count = 0;
    uint32_t note_count = 0;
    int64_t location_index_sum = 0;
};

void walk_node_recursive(Node node, WalkStats& stats) {
    stats.node_count += 1;

    std::string_view tname = node.type_name();
    auto ctor = node.kind();

    if (tname == "when_then") {
        stats.when_then_count += 1;
    } else if (tname == "code_location") {
        if (auto idx_val = node.field("index")) {
            if (auto idx = idx_val->as_long()) {
                stats.location_index_sum += *idx;
            }
        }
    }

    if (ctor == "Identifier") {
        if (auto parts_val = node.field("parts")) {
            if (auto parts = parts_val->as_seq()) {
                stats.ident_part_count += static_cast<uint32_t>(parts->len());
            }
        }
    } else if (ctor == "Number") {
        if (auto text_val = node.field("text")) {
            if (auto text = text_val->as_str()) {
                std::string s(*text);
                stats.number_sum += std::strtoll(s.c_str(), nullptr, 10);
            }
        }
    } else if (ctor == "BinaryOp") {
        stats.binary_op_count += 1;
    } else if (ctor == "Cast") {
        stats.cast_count += 1;
        if (auto notes_val = node.field("notes")) {
            if (auto notes = notes_val->as_seq()) {
                stats.note_count += static_cast<uint32_t>(notes->len());
            }
        }
    }

    FieldSlice fields = node.fields();
    for (size_t i = 0; i < fields.len(); ++i) {
        auto val = node.field_at(i);
        if (!val) {
            continue;
        }
        if (auto child = val->as_node()) {
            walk_node_recursive(*child, stats);
        } else if (auto seq = val->as_seq()) {
            for (Value item : *seq) {
                if (auto elem_node = item.as_node()) {
                    walk_node_recursive(*elem_node, stats);
                }
            }
        }
    }
}

} // namespace

extern "C" {

struct CppTreeReport {
    uint32_t runtime_abi_version;
    uint32_t abi_version;
    uint64_t schema_hash;
    uint32_t expr_count;
    uint32_t span_count;
    uint32_t note_count;
    uint32_t ident_part_count;
    uint32_t case_count;
    int32_t number_value;
    uint8_t dt0_nullable;
    uint8_t dt0_unsigned;
    uint8_t dt0_width;
    uint8_t dt1_nullable;
    int64_t span0_end_index;
    int64_t span1_end_index;
};

struct CppWalkSummary {
    uint32_t node_count;
    uint32_t ident_part_count;
    int64_t number_sum;
    uint32_t binary_op_count;
    uint32_t cast_count;
    uint32_t when_then_count;
    uint32_t note_count;
    int64_t location_index_sum;
};

int32_t cpp_inspect_mini_sql_tree(
    FlatTree* raw,
    CppTreeReport* out,
    uint8_t* sexpr_buf,
    size_t sexpr_cap,
    size_t* sexpr_len
) {
    AbiError err{};
    auto tree_opt = Tree::from_raw(raw, &err);
    if (!tree_opt) {
        return 1;
    }
    Tree tree = std::move(*tree_opt);

    std::string schema_err;
    if (!validate_schema(*tree.raw()->schema, 1, &schema_err)) {
        return 2;
    }

    out->runtime_abi_version = tree.runtime_abi_version();
    out->abi_version = tree.abi_version();
    out->schema_hash = tree.schema_hash();

    Node root = tree.root();
    if (root.type_name() != "file" || root.kind().has_value() || root.kind_id().has_value()) {
        return 3;
    }
    if (root.field("nonexistent").has_value()) {
        return 4;
    }
    if (root.as_product<CppWhenThen>("when_then") != nullptr) {
        return 5;
    }
    if (root.as_payload<CppDataTypeInt>("data_type", 0) != nullptr) {
        return 6;
    }
    if (root.fields().len() != 2 || !root.field_at(0).has_value() ||
        root.field_at(2).has_value()) {
        return 7;
    }

    auto exprs_val = root.field("exprs");
    if (!exprs_val || !exprs_val->is_seq()) {
        return 8;
    }
    SeqView exprs = *exprs_val->as_seq();
    if (exprs.is_empty() || exprs.len() != 2 || exprs.get(2).has_value()) {
        return 9;
    }
    if (exprs.elem_type() != tree.type_by_name("expr")) {
        return 10;
    }
    out->expr_count = static_cast<uint32_t>(exprs.len());

    auto cast0_opt = exprs[0].as_node();
    if (!cast0_opt) {
        return 11;
    }
    Node cast0 = *cast0_opt;
    if (cast0.type_name() != "expr" || cast0.kind() != "Cast") {
        return 12;
    }
    if (cast0.kind_id() != tree.kind_by_name("Cast") ||
        tree.kind_by_name("NoSuchCtor").has_value()) {
        return 13;
    }

    auto notes_val = cast0.field("notes");
    if (!notes_val || !notes_val->is_seq()) {
        return 14;
    }
    SeqView notes = *notes_val->as_seq();
    std::vector<std::string_view> note_strs;
    for (Value v : notes) {
        auto s = v.as_str();
        if (!s) {
            return 15;
        }
        note_strs.push_back(*s);
    }
    if (note_strs.size() != 2 || note_strs[0] != "narrow" || note_strs[1] != "checked") {
        return 16;
    }
    out->note_count = static_cast<uint32_t>(note_strs.size());

    auto to0_opt = child_node(cast0, "to");
    if (!to0_opt) {
        return 17;
    }
    Node to0 = *to0_opt;
    if (to0.type_name() != "data_type" || to0.kind() != "Int") {
        return 18;
    }
    if (to0.field("nullable")->as_bool() != true ||
        to0.field("unsigned")->as_bool() != false ||
        to0.field("width")->as_byte() != 64) {
        return 19;
    }
    if (to0.as_payload<CppDataTypeBool>("data_type", 1) != nullptr) {
        return 20;
    }
    const CppDataTypeInt* dt0 = to0.as_payload<CppDataTypeInt>("data_type", 0);
    if (dt0 == nullptr || dt0->span == NONE_U32 || dt0->nullable != 1 ||
        dt0->unsigned_.get() != false || dt0->width.get() != 64) {
        return 21;
    }
    out->dt0_nullable = dt0->nullable;
    out->dt0_unsigned = static_cast<uint8_t>(*dt0->unsigned_.get() ? 1 : 0);
    out->dt0_width = *dt0->width.get();

    auto case_opt = child_node(cast0, "e");
    if (!case_opt || case_opt->kind() != "Case") {
        return 22;
    }
    Node case_node = *case_opt;

    auto operand_opt = child_node(case_node, "operand");
    if (!operand_opt || operand_opt->kind() != "Identifier") {
        return 23;
    }
    SeqView parts = *operand_opt->field("parts")->as_seq();
    std::vector<std::string_view> part_strs;
    for (Value v : parts) {
        part_strs.push_back(*v.as_str());
    }
    if (part_strs.size() != 3 || part_strs[0] != "db" || part_strs[1] != "users" ||
        part_strs[2] != "id") {
        return 24;
    }
    out->ident_part_count = static_cast<uint32_t>(part_strs.size());

    SeqView cases = *case_node.field("cases")->as_seq();
    if (cases.len() != 2) {
        return 25;
    }
    out->case_count = static_cast<uint32_t>(cases.len());

    Node wt0 = *cases[0].as_node();
    if (wt0.type_name() != "when_then" || wt0.kind().has_value()) {
        return 26;
    }
    auto wt0_op = wt0.field("op")->as_simple_sum();
    if (!wt0_op || tree.simple_variant_name(wt0_op->type_index, wt0_op->tag) != "Add") {
        return 27;
    }
    const CppWhenThen* wt0_proj = wt0.as_product<CppWhenThen>("when_then");
    if (wt0_proj == nullptr || wt0_proj->when == NONE_U32 || wt0_proj->then == NONE_U32 ||
        wt0_proj->op != 0) {
        return 28;
    }

    Node when0 = *child_node(wt0, "when");
    if (when0.kind() != "BinaryOp") {
        return 29;
    }
    auto bin_op = when0.field("op")->as_simple_sum();
    if (!bin_op || tree.simple_variant_name(bin_op->type_index, bin_op->tag) != "Eq") {
        return 30;
    }
    Node right0 = *child_node(when0, "right");
    Node num_lit = *child_node(right0, "value");
    if (num_lit.kind() != "Number") {
        return 31;
    }
    std::string num_text(*num_lit.field("text")->as_str());
    out->number_value = static_cast<int32_t>(std::strtol(num_text.c_str(), nullptr, 10));

    Node then0 = *child_node(wt0, "then");
    Node str_lit = *child_node(then0, "value");
    if (str_lit.kind() != "String" || str_lit.field("value")->as_str() != "ok\n\"1\"") {
        return 32;
    }

    Node wt1 = *cases[1].as_node();
    if (!wt1.field("op")->is_none()) {
        return 33;
    }
    const CppWhenThen* wt1_proj = wt1.as_product<CppWhenThen>("when_then");
    if (wt1_proj == nullptr || wt1_proj->op != NONE_U8) {
        return 34;
    }

    Node else_node = *child_node(case_node, "else");
    Node null_lit = *child_node(else_node, "value");
    if (null_lit.kind() != "Null") {
        return 35;
    }

    Node cast1 = *exprs[1].as_node();
    Node to1 = *child_node(cast1, "to");
    if (to1.as_payload<CppDataTypeInt>("data_type", 0) != nullptr) {
        return 36;
    }
    const CppDataTypeBool* dt1 = to1.as_payload<CppDataTypeBool>("data_type", 1);
    if (dt1 == nullptr || dt1->nullable != 0) {
        return 37;
    }
    out->dt1_nullable = dt1->nullable;

    SeqView spans = *root.field("spans")->as_seq();
    if (spans.len() != 2) {
        return 38;
    }
    out->span_count = static_cast<uint32_t>(spans.len());

    Node span0 = *spans[0].as_node();
    Node start0 = *child_node(span0, "start");
    Node end0 = *child_node(span0, "end");
    if (start0.field("lineno")->as_int() != 1 || start0.field("col_offset")->as_int() != 0 ||
        start0.field("index")->as_long() != 0) {
        return 39;
    }
    out->span0_end_index = *end0.field("index")->as_long();

    Node span1 = *spans[1].as_node();
    Node start1 = *child_node(span1, "start");
    Node end1 = *child_node(span1, "end");
    if (!start1.field("lineno")->is_none() || !start1.field("col_offset")->is_none() ||
        !start1.field("index")->is_none()) {
        return 40;
    }
    out->span1_end_index = *end1.field("index")->as_long();

    std::string sexpr = tree.to_sexpr();
    if (sexpr != root.to_sexpr()) {
        return 41;
    }
    write_out_str(sexpr, sexpr_buf, sexpr_cap, sexpr_len);

    return 0;
}

int32_t cpp_walk_and_summarize(
    FlatTree* raw,
    CppWalkSummary* out,
    uint8_t* sexpr_buf,
    size_t sexpr_cap,
    size_t* sexpr_len
) {
    auto tree_opt = Tree::from_raw(raw);
    if (!tree_opt) {
        return 1;
    }
    Tree tree = std::move(*tree_opt);

    if (!validate_schema(*tree.raw()->schema, tree.abi_version())) {
        return 2;
    }

    WalkStats stats{};
    walk_node_recursive(tree.root(), stats);

    out->node_count = stats.node_count;
    out->ident_part_count = stats.ident_part_count;
    out->number_sum = stats.number_sum;
    out->binary_op_count = stats.binary_op_count;
    out->cast_count = stats.cast_count;
    out->when_then_count = stats.when_then_count;
    out->note_count = stats.note_count;
    out->location_index_sum = stats.location_index_sum;

    std::string sexpr = tree.to_sexpr();
    write_out_str(sexpr, sexpr_buf, sexpr_cap, sexpr_len);

    return 0;
}

int32_t cpp_test_abi_and_primitives() {
    if (!OptBool::NONE.is_none() || OptBool::NONE.get().has_value()) {
        return 1;
    }
    if (OptBool::from(true).get() != true || OptBool::from(false).get() != false) {
        return 2;
    }
    if (!OptByte::NONE.is_none() || OptByte::from(uint8_t{255}).get() != 255) {
        return 3;
    }
    if (!OptInt::NONE.is_none() || OptInt::from(42).get() != 42) {
        return 4;
    }
    if (!OptLong::NONE.is_none() || OptLong::from(int64_t{-99}).get() != -99) {
        return 5;
    }

    auto [tag, payload] = unpack_sum_ref(pack_sum_ref(7, 12345));
    if (tag != 7 || payload != 12345) {
        return 6;
    }

    AbiError err{};
    if (Tree::from_raw(nullptr, &err).has_value() || err != AbiError::null_tree() ||
        err.message() != "null tree") {
        return 7;
    }

    FlatTree null_schema_tree{};
    null_schema_tree.schema = nullptr;
    if (Tree::from_raw(&null_schema_tree, &err).has_value() || err != AbiError::null_schema() ||
        err.message() != "null schema") {
        return 8;
    }

    FlatSchema bad_runtime_schema{};
    bad_runtime_schema.runtime_abi_version = 999;
    FlatTree bad_runtime_tree{};
    bad_runtime_tree.schema = &bad_runtime_schema;
    if (Tree::from_raw(&bad_runtime_tree, &err).has_value() ||
        err != AbiError::runtime_abi_version(999, RUNTIME_ABI_VERSION)) {
        return 9;
    }

    std::string msg;
    if (validate_schema(bad_runtime_schema, 1, &msg)) {
        return 10;
    }

    FlatSchema bad_abi_schema{};
    bad_abi_schema.runtime_abi_version = RUNTIME_ABI_VERSION;
    bad_abi_schema.abi_version = 2;
    if (validate_schema(bad_abi_schema, 1, &msg)) {
        return 11;
    }

    return 0;
}

} // extern "C"
