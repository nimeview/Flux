#ifndef FLUX_SHARED_BYTECODE_HPP
#define FLUX_SHARED_BYTECODE_HPP

#include <string>

namespace flux::shared {

enum class instruction_kind {
    signal_decl,
    reactive_decl,
    set_value,
    expr,
};

struct instruction {
    instruction_kind kind;
    std::string name;
    std::string payload_rpn;
};

} // namespace flux::shared

#endif // FLUX_SHARED_BYTECODE_HPP
