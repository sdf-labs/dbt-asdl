#include <dbt_asdl/flat_runtime.hpp>
#include "gen_flat.hpp"

#include <cstdlib>
#include <cstring>
#include <string>
#include <string_view>
#include <vector>

namespace {

using namespace dbt_asdl;
using namespace mini_sql;

static_assert(sizeof(CodeLocation) == 16 && alignof(CodeLocation) == alignof(int64_t));
static_assert(sizeof(Span) == 8 && alignof(Span) == 4);
static_assert(sizeof(data_type::Int) == 8 && alignof(data_type::Int) == 4);
static_assert(sizeof(data_type::Bool) == 8 && alignof(data_type::Bool) == 4);
static_assert(sizeof(literal::Number) == 12 && alignof(literal::Number) == 4);
static_assert(sizeof(literal::String) == 12 && alignof(literal::String) == 4);
static_assert(sizeof(literal::Null) == 4 && alignof(literal::Null) == 4);
static_assert(sizeof(WhenThen) == 12 && alignof(WhenThen) == 4);
static_assert(sizeof(expr::Identifier) == 12 && alignof(expr::Identifier) == 4);
static_assert(sizeof(expr::Literal) == 8 && alignof(expr::Literal) == 4);
static_assert(sizeof(expr::BinaryOp) == 16 && alignof(expr::BinaryOp) == 4);
static_assert(sizeof(expr::Case) == 20 && alignof(expr::Case) == 4);
static_assert(sizeof(expr::Cast) == 20 && alignof(expr::Cast) == 4);
static_assert(sizeof(File) == 16 && alignof(File) == 4);

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


void walk_code_location_typed(const CodeLocation* loc, WalkStats& stats) {
    stats.node_count += 1;
    if (auto idx = loc->index.get()) {
        stats.location_index_sum += *idx;
    }
}

void walk_span_typed(TreeView tree, const Span* sp, WalkStats& stats) {
    stats.node_count += 1;
    walk_code_location_typed(as_code_location(tree, sp->start), stats);
    walk_code_location_typed(as_code_location(tree, sp->end), stats);
}

void walk_data_type_typed(TreeView tree, DataTypeView dt, WalkStats& stats) {
    stats.node_count += 1;
    switch (*dt.tag()) {
        case DataTypeTag::Int:
            walk_span_typed(tree, as_span(tree, dt.as_int()->span), stats);
            break;
        case DataTypeTag::Bool:
            walk_span_typed(tree, as_span(tree, dt.as_bool()->span), stats);
            break;
    }
}

void walk_literal_typed(TreeView tree, LiteralView lit, WalkStats& stats) {
    stats.node_count += 1;
    switch (*lit.tag()) {
        case LiteralTag::Number: {
            const literal::Number* r = lit.as_number();
            walk_span_typed(tree, as_span(tree, r->span), stats);
            std::string s(*tree.str(r->text));
            stats.number_sum += std::strtoll(s.c_str(), nullptr, 10);
            break;
        }
        case LiteralTag::String:
            walk_span_typed(tree, as_span(tree, lit.as_string()->span), stats);
            break;
        case LiteralTag::Null:
            walk_span_typed(tree, as_span(tree, lit.as_null()->span), stats);
            break;
    }
}

void walk_expr_typed(TreeView tree, ExprView e, WalkStats& stats);

void walk_when_then_typed(TreeView tree, const WhenThen* wt, WalkStats& stats) {
    stats.node_count += 1;
    stats.when_then_count += 1;
    walk_expr_typed(tree, *as_expr(tree, wt->when), stats);
    walk_expr_typed(tree, *as_expr(tree, wt->then), stats);
}

void walk_expr_typed(TreeView tree, ExprView e, WalkStats& stats) {
    stats.node_count += 1;
    switch (*e.tag()) {
        case ExprTag::Identifier: {
            const expr::Identifier* r = e.as_identifier();
            walk_span_typed(tree, as_span(tree, r->span), stats);
            stats.ident_part_count += static_cast<uint32_t>(tree.seq("identifier", r->parts)->len());
            break;
        }
        case ExprTag::Literal: {
            const expr::Literal* r = e.as_literal();
            walk_span_typed(tree, as_span(tree, r->span), stats);
            walk_literal_typed(tree, *as_literal(tree, r->value), stats);
            break;
        }
        case ExprTag::BinaryOp: {
            stats.binary_op_count += 1;
            const expr::BinaryOp* r = e.as_binary_op();
            walk_span_typed(tree, as_span(tree, r->span), stats);
            walk_expr_typed(tree, *as_expr(tree, r->left), stats);
            walk_expr_typed(tree, *as_expr(tree, r->right), stats);
            break;
        }
        case ExprTag::Case: {
            const expr::Case* r = e.as_case();
            walk_span_typed(tree, as_span(tree, r->span), stats);
            if (auto op = as_expr(tree, r->operand)) {
                walk_expr_typed(tree, *op, stats);
            }
            for (Value item : *tree.seq("when_then", r->cases)) {
                walk_when_then_typed(tree, as_when_then(*item.as_node()), stats);
            }
            if (auto el = as_expr(tree, r->else_)) {
                walk_expr_typed(tree, *el, stats);
            }
            break;
        }
        case ExprTag::Cast: {
            stats.cast_count += 1;
            const expr::Cast* r = e.as_cast();
            walk_span_typed(tree, as_span(tree, r->span), stats);
            walk_expr_typed(tree, *as_expr(tree, r->e), stats);
            walk_data_type_typed(tree, *as_data_type(tree, r->to), stats);
            stats.note_count += static_cast<uint32_t>(tree.seq("string", r->notes)->len());
            break;
        }
    }
}

void walk_file_typed(TreeView tree, const File* f, WalkStats& stats) {
    stats.node_count += 1;
    for (Value item : *tree.seq("expr", f->exprs)) {
        walk_expr_typed(tree, *as_expr(*item.as_node()), stats);
    }
    for (Value item : *tree.seq("span", f->spans)) {
        walk_span_typed(tree, as_span(*item.as_node()), stats);
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
    if (!validate_schema(*tree.raw()->schema, ABI_VERSION, &schema_err) ||
        tree.schema_hash() != SCHEMA_HASH) {
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
    if (as_when_then(root) != nullptr) {
        return 5;
    }
    if (as_data_type(root).has_value()) {
        return 6;
    }
    const File* file_rec = as_file(root);
    if (file_rec == nullptr || root.fields().len() != 2 || !root.field_at(0).has_value() ||
        root.field_at(2).has_value()) {
        return 7;
    }

    auto exprs_val = root.field("exprs");
    if (!exprs_val || !exprs_val->is_seq()) {
        return 8;
    }
    SeqView exprs = *exprs_val->as_seq();
    if (exprs.is_empty() || exprs.len() != 2 || file_rec->exprs.len != 2 ||
        exprs.get(2).has_value()) {
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
    auto cast0_view = as_expr(cast0);
    if (!cast0_view || cast0_view->tag() != ExprTag::Cast || cast0.kind() != "Cast") {
        return 12;
    }
    const expr::Cast* cast0_rec = cast0_view->as_cast();
    if (cast0_rec == nullptr || cast0.kind_id() != tree.kind_by_name("Cast") ||
        tree.kind_by_name("NoSuchCtor").has_value()) {
        return 13;
    }

    auto notes_seq = tree.seq("string", cast0_rec->notes);
    if (!notes_seq) {
        return 14;
    }
    std::vector<std::string_view> note_strs;
    for (Value v : *notes_seq) {
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
    auto dt0_view = as_data_type(tree, cast0_rec->to);
    if (!dt0_view || dt0_view->tag() != DataTypeTag::Int || to0.kind() != "Int") {
        return 18;
    }
    if (to0.field("nullable")->as_bool() != true ||
        to0.field("unsigned")->as_bool() != false ||
        to0.field("width")->as_byte() != 64) {
        return 19;
    }
    if (dt0_view->as_bool() != nullptr) {
        return 20;
    }
    const data_type::Int* dt0 = dt0_view->as_int();
    if (dt0 == nullptr || dt0->span.is_none() || dt0->nullable != 1 ||
        dt0->unsigned_.get() != false || dt0->width.get() != 64) {
        return 21;
    }
    out->dt0_nullable = dt0->nullable;
    out->dt0_unsigned = static_cast<uint8_t>(*dt0->unsigned_.get() ? 1 : 0);
    out->dt0_width = *dt0->width.get();

    auto case_view = as_expr(tree, cast0_rec->e);
    if (!case_view || case_view->tag() != ExprTag::Case) {
        return 22;
    }
    const expr::Case* case_rec = case_view->as_case();
    Node case_node = case_view->node();

    auto operand_view = as_expr(tree, case_rec->operand);
    if (!operand_view || operand_view->tag() != ExprTag::Identifier) {
        return 23;
    }
    const expr::Identifier* ident_rec = operand_view->as_identifier();
    SeqView parts = *tree.seq("identifier", ident_rec->parts);
    std::vector<std::string_view> part_strs;
    for (Value v : parts) {
        part_strs.push_back(*v.as_str());
    }
    if (part_strs.size() != 3 || part_strs[0] != "db" || part_strs[1] != "users" ||
        part_strs[2] != "id") {
        return 24;
    }
    out->ident_part_count = static_cast<uint32_t>(part_strs.size());

    SeqView cases = *tree.seq("when_then", case_rec->cases);
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
    const WhenThen* wt0_proj = as_when_then(wt0);
    if (wt0_proj == nullptr || wt0_proj->when.is_none() || wt0_proj->then.is_none() ||
        bin_op_from_u8(wt0_proj->op) != BinOp::Add) {
        return 28;
    }

    auto when0_view = as_expr(tree, wt0_proj->when);
    if (!when0_view || when0_view->tag() != ExprTag::BinaryOp) {
        return 29;
    }
    const expr::BinaryOp* bin_rec = when0_view->as_binary_op();
    if (bin_rec == nullptr || bin_op_from_u8(bin_rec->op) != BinOp::Eq) {
        return 30;
    }
    auto right0_lit_expr = as_expr(tree, bin_rec->right)->as_literal();
    auto num_lit_view = as_literal(tree, right0_lit_expr->value);
    if (!num_lit_view || num_lit_view->tag() != LiteralTag::Number) {
        return 31;
    }
    const literal::Number* num_rec = num_lit_view->as_number();
    std::string num_text(*tree.str(num_rec->text));
    out->number_value = static_cast<int32_t>(std::strtol(num_text.c_str(), nullptr, 10));

    auto then0_lit_expr = as_expr(tree, wt0_proj->then)->as_literal();
    auto str_lit_view = as_literal(tree, then0_lit_expr->value);
    const literal::String* str_rec = str_lit_view->as_string();
    if (str_rec == nullptr || tree.str(str_rec->value) != "ok\n\"1\"") {
        return 32;
    }

    Node wt1 = *cases[1].as_node();
    if (!wt1.field("op")->is_none()) {
        return 33;
    }
    const WhenThen* wt1_proj = as_when_then(wt1);
    if (wt1_proj == nullptr || wt1_proj->op != NONE_U8 ||
        bin_op_from_u8(wt1_proj->op).has_value()) {
        return 34;
    }

    auto else_lit_expr = as_expr(tree, case_rec->else_)->as_literal();
    auto null_lit_view = as_literal(tree, else_lit_expr->value);
    if (!null_lit_view || null_lit_view->tag() != LiteralTag::Null ||
        null_lit_view->as_null() == nullptr) {
        return 35;
    }

    Node cast1 = *exprs[1].as_node();
    Node to1 = *child_node(cast1, "to");
    auto dt1_view = as_data_type(to1);
    if (!dt1_view || dt1_view->as_int() != nullptr) {
        return 36;
    }
    const data_type::Bool* dt1 = dt1_view->as_bool();
    if (dt1 == nullptr || dt1->nullable != 0) {
        return 37;
    }
    out->dt1_nullable = dt1->nullable;

    SeqView spans = *tree.seq("span", file_rec->spans);
    if (spans.len() != 2) {
        return 38;
    }
    out->span_count = static_cast<uint32_t>(spans.len());

    const Span* span0 = as_span(*spans[0].as_node());
    const CodeLocation* start0 = as_code_location(tree, span0->start);
    const CodeLocation* end0 = as_code_location(tree, span0->end);
    if (start0 == nullptr || end0 == nullptr || start0->lineno.get() != 1 ||
        start0->col_offset.get() != 0 || start0->index.get() != 0) {
        return 39;
    }
    out->span0_end_index = *end0->index.get();

    const Span* span1 = as_span(*spans[1].as_node());
    const CodeLocation* start1 = as_code_location(tree, span1->start);
    const CodeLocation* end1 = as_code_location(tree, span1->end);
    if (start1 == nullptr || end1 == nullptr || !start1->lineno.is_none() ||
        !start1->col_offset.is_none() || !start1->index.is_none()) {
        return 40;
    }
    out->span1_end_index = *end1->index.get();

    std::string sexpr = tree.to_sexpr();
    if (sexpr != root.to_sexpr() || case_node.kind() != "Case") {
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

    WalkStats typed_stats{};
    walk_file_typed(tree, as_file(tree.root()), typed_stats);
    if (typed_stats.node_count != stats.node_count ||
        typed_stats.ident_part_count != stats.ident_part_count ||
        typed_stats.number_sum != stats.number_sum ||
        typed_stats.binary_op_count != stats.binary_op_count ||
        typed_stats.cast_count != stats.cast_count ||
        typed_stats.when_then_count != stats.when_then_count ||
        typed_stats.note_count != stats.note_count ||
        typed_stats.location_index_sum != stats.location_index_sum) {
        return 3;
    }

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
