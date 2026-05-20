#ifndef FLUX_RUNTIME_GRAPH_H
#define FLUX_RUNTIME_GRAPH_H

#include <string>
#include <unordered_map>
#include <unordered_set>

namespace flux::graph {

class dependency_graph {
public:
    void register_reactive(const std::string& node, const std::string& rpn_expr);
    void connect_dependency(const std::string& dependency, const std::string& reactive_node);

    const std::unordered_map<std::string, std::string>& reactive_expressions() const;
    const std::unordered_map<std::string, std::unordered_set<std::string>>& dependents() const;

private:
    std::unordered_map<std::string, std::string> reactive_rpn_;
    std::unordered_map<std::string, std::unordered_set<std::string>> dependents_;
};

} // namespace flux::graph

#endif // FLUX_RUNTIME_GRAPH_H
