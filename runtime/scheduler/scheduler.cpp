#include "scheduler.h"

#include <algorithm>
#include <queue>

namespace flux::scheduler {

void recompute_scheduler::recompute_batch(
    const std::vector<std::string>& changed_sources,
    flux::graph::dependency_graph& graph,
    std::unordered_map<std::string, flux::stdlib::value>& values,
    std::uint64_t tick,
    const eval_fn& eval) {
    std::unordered_set<std::string> affected;
    std::queue<std::string> q;

    for (const auto& src : changed_sources) q.push(src);

    while (!q.empty()) {
        auto cur = q.front(); q.pop();
        auto it = graph.dependents().find(cur);
        if (it == graph.dependents().end()) continue;
        for (const auto& dep : it->second) {
            if (!affected.insert(dep).second) continue;
            q.push(dep);
        }
    }

    std::vector<std::string> order(affected.begin(), affected.end());
    std::sort(order.begin(), order.end()); // deterministic

    for (const auto& node : order) {
        auto expr_it = graph.reactive_expressions().find(node);
        if (expr_it == graph.reactive_expressions().end()) continue;
        values[node] = eval(expr_it->second);
        auto& meta = graph.meta_mut(node);
        meta.version += 1;
        meta.last_eval_tick = tick;
    }
}

}
