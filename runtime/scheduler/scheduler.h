#ifndef FLUX_RUNTIME_SCHEDULER_H
#define FLUX_RUNTIME_SCHEDULER_H

#include "../../stdlib/std.hpp"
#include <cstdint>
#include <functional>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace flux::scheduler {
class recompute_scheduler {
public:
    using eval_fn = std::function<flux::stdlib::value(const std::vector<std::uint8_t>&)>;
    void recompute_from(const std::string& source,
        const std::unordered_map<std::string, std::unordered_set<std::string>>& dependents,
        const std::unordered_map<std::string, std::vector<std::uint8_t>>& reactive_expr,
        std::unordered_map<std::string, flux::stdlib::value>& values,
        const eval_fn& eval);
};
}
#endif
