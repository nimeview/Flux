#include "graph.h"

namespace flux::graph {

void dependency_graph::register_reactive(const std::string& node, const std::string& rpn_expr) {
    reactive_rpn_[node] = rpn_expr;
}

void dependency_graph::connect_dependency(const std::string& dependency, const std::string& reactive_node) {
    dependents_[dependency].insert(reactive_node);
}

const std::unordered_map<std::string, std::string>& dependency_graph::reactive_expressions() const {
    return reactive_rpn_;
}

const std::unordered_map<std::string, std::unordered_set<std::string>>& dependency_graph::dependents() const {
    return dependents_;
}

} // namespace flux::graph
