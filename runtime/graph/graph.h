#ifndef FLUX_RUNTIME_GRAPH_H
#define FLUX_RUNTIME_GRAPH_H

#include <cstdint>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace flux::graph {

struct node_meta {
    std::uint64_t version = 0;
    std::uint64_t last_eval_tick = 0;
    std::uint64_t topo_index = 0;
    std::uint64_t topo_level = 0;
};

class dependency_graph {
public:
    void register_reactive(const std::string& node, const std::vector<std::uint8_t>& expr);
    void connect_dependency(const std::string& dependency, const std::string& reactive_node);

    const std::unordered_map<std::string, std::vector<std::uint8_t>>& reactive_expressions() const;
    const std::unordered_map<std::string, std::unordered_set<std::string>>& dependents() const;

    node_meta& meta_mut(const std::string& node);
    const node_meta& meta(const std::string& node) const;

private:
    bool has_path(const std::string& from, const std::string& to) const;
    void recompute_topo_index();
    void propagate_topo_level(const std::string& start);

    std::unordered_map<std::string, std::vector<std::uint8_t>> reactive_expr_;
    std::unordered_map<std::string, std::unordered_set<std::string>> dependents_;
    std::unordered_map<std::string, node_meta> meta_;
    std::uint64_t next_topo_index_ = 0;
};

} // namespace flux::graph

#endif
