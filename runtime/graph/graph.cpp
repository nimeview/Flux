#include "graph.h"
namespace flux::graph {
void dependency_graph::register_reactive(const std::string& node, const std::vector<std::uint8_t>& expr){ reactive_expr_[node]=expr; }
void dependency_graph::connect_dependency(const std::string& dependency, const std::string& reactive_node){ dependents_[dependency].insert(reactive_node); }
const std::unordered_map<std::string, std::vector<std::uint8_t>>& dependency_graph::reactive_expressions() const { return reactive_expr_; }
const std::unordered_map<std::string, std::unordered_set<std::string>>& dependency_graph::dependents() const { return dependents_; }
}
