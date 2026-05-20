#ifndef FLUX_RUNTIME_SCHEDULER_H
#define FLUX_RUNTIME_SCHEDULER_H

#include <functional>
#include <string>
#include <unordered_map>
#include <unordered_set>

namespace flux::scheduler {

class recompute_scheduler {
public:
    using eval_fn = std::function<double(const std::string&)>;

    void recompute_from(
        const std::string& source,
        const std::unordered_map<std::string, std::unordered_set<std::string>>& dependents,
        const std::unordered_map<std::string, std::string>& reactive_expr,
        std::unordered_map<std::string, double>& values,
        const eval_fn& eval);
};

} // namespace flux::scheduler

#endif // FLUX_RUNTIME_SCHEDULER_H
