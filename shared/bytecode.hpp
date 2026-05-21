#ifndef FLUX_SHARED_BYTECODE_HPP
#define FLUX_SHARED_BYTECODE_HPP

#include <cstdint>
#include <string>
#include <vector>

namespace flux::shared {

enum class instruction_kind : std::uint8_t {
    signal_decl = 0,
    reactive_decl = 1,
    set_value = 2,
    expr = 3,
};

enum class expr_opcode : std::uint8_t {
    push_number = 1,
    push_bool = 2,
    load_name = 3,
    add = 4,
    sub = 5,
    mul = 6,
    div = 7,
    call = 8,
    push_string = 9,
    make_tuple = 10,
    jump = 11,
    branch_if_false = 12,
    dup = 13,
    pop = 14,
    eq_bool = 15,
    eq_number = 16,
};

struct instruction {
    instruction_kind kind;
    std::string name;
    std::vector<std::uint8_t> payload;
};

} // namespace flux::shared

#endif // FLUX_SHARED_BYTECODE_HPP
