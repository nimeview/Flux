#include "scheduler.h"

#include <algorithm>
#include <queue>

namespace flux::scheduler {

void recompute_scheduler::recompute_batch(
    const std::vector<std::string>& changed_sources,
    flux::graph::dependency_graph& graph,
    std::unordered_map<std::string, flux::stdlib::value>& values,
    std::uint64_t tick,
    const eval_fn& eval,
    const trace_fn& trace) {
    std::unordered_set<std::string> affected;
    std::queue<std::string> q;

    for (const auto& src : changed_sources) q.push(src);

    while (!q.empty()) {
        auto cur = q.front(); q.pop();
        auto it = graph.dependents().find(cur);
        if (it == graph.dependents().end()) continue;
        for (const auto& dep : it->second) {
            if (!affected.insert(dep).second) continue;
            const auto& meta = graph.meta(dep);
            trace("event=mark_affected node=" + dep +
                  " cause=" + cur +
                  " old_version=" + std::to_string(meta.version) +
                  " new_version=" + std::to_string(meta.version) +
                  " tick=" + std::to_string(tick) +
                  " topo_level=" + std::to_string(meta.topo_level));
            q.push(dep);
        }
    }

    std::vector<std::string> order(affected.begin(), affected.end());
    std::sort(order.begin(), order.end(), [&](const std::string& a, const std::string& b) {
        const auto& ma = graph.meta(a);
        const auto& mb = graph.meta(b);
        if (ma.topo_level != mb.topo_level) return ma.topo_level < mb.topo_level;
        return a < b;
    });

    for (const auto& node : order) {
        auto expr_it = graph.reactive_expressions().find(node);
        if (expr_it == graph.reactive_expressions().end()) continue;
        auto& meta = graph.meta_mut(node);
        const auto old_version = meta.version;
        trace("event=recompute_start node=" + node +
              " cause=batch old_version=" + std::to_string(old_version) +
              " new_version=" + std::to_string(old_version + 1) +
              " tick=" + std::to_string(tick) +
              " topo_level=" + std::to_string(meta.topo_level));
        values[node] = eval(expr_it->second);
        meta.version += 1;
        meta.last_eval_tick = tick;
        trace("event=recompute_done node=" + node +
              " cause=batch old_version=" + std::to_string(old_version) +
              " new_version=" + std::to_string(meta.version) +
              " tick=" + std::to_string(tick) +
              " topo_level=" + std::to_string(meta.topo_level));
    }
}

}
