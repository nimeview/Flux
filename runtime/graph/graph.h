#ifndef FLUX_RUNTIME_GRAPH_H
#define FLUX_RUNTIME_GRAPH_H

#include <cstdint>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace flux::graph {

class dependency_graph {
public:
    void register_reactive(const std::string& node, const std::vector<std::uint8_t>& expr);
    void connect_dependency(const std::string& dependency, const std::string& reactive_node);

    const std::unordered_map<std::string, std::vector<std::uint8_t>>& reactive_expressions() const;
    const std::unordered_map<std::string, std::unordered_set<std::string>>& dependents() const;

private:
    std::unordered_map<std::string, std::vector<std::uint8_t>> reactive_expr_;
    std::unordered_map<std::string, std::unordered_set<std::string>> dependents_;
};

} // namespace flux::graph

#endif
