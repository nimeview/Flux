#ifndef FLUX_RUNTIME_SCHEDULER_H
#define FLUX_RUNTIME_SCHEDULER_H

#include "../../stdlib/std.hpp"
#include "../graph/graph.h"

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
    using trace_fn = std::function<void(const std::string&)>;

    void recompute_batch(
        const std::vector<std::string>& changed_sources,
        flux::graph::dependency_graph& graph,
        std::unordered_map<std::string, flux::stdlib::value>& values,
        std::uint64_t tick,
        const eval_fn& eval,
        const trace_fn& trace);
};

} // namespace flux::scheduler

#endif
