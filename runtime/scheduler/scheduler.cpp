#include "scheduler.h"

#include <queue>

namespace flux::scheduler {

void recompute_scheduler::recompute_from(
    const std::string& source,
    const std::unordered_map<std::string, std::unordered_set<std::string>>& dependents,
    const std::unordered_map<std::string, std::string>& reactive_expr,
    std::unordered_map<std::string, flux::stdlib::value>& values,
    const eval_fn& eval) {
    std::queue<std::string> q;
    std::unordered_set<std::string> seen;
    q.push(source);

    while (!q.empty()) {
        const auto current = q.front();
        q.pop();

        auto it = dependents.find(current);
        if (it == dependents.end()) continue;

        for (const auto& dep : it->second) {
            if (!seen.insert(dep).second) continue;
            values[dep] = eval(reactive_expr.at(dep));
            q.push(dep);
        }
    }
}

} // namespace flux::scheduler
