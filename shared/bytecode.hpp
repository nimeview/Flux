#ifndef FLUX_SHARED_BYTECODE_HPP
#define FLUX_SHARED_BYTECODE_HPP

#include <cstdint>
#include <string>

namespace flux::shared {

enum class instruction_kind : std::uint8_t {
    signal_decl = 0,
    reactive_decl = 1,
    set_value = 2,
    expr = 3,
};

struct instruction {
    instruction_kind kind;
    std::string name;
    std::string payload_rpn;
};

} // namespace flux::shared

#endif // FLUX_SHARED_BYTECODE_HPP
