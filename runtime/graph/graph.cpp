#include "graph.h"

#include <algorithm>
#include <queue>
#include <stdexcept>

namespace flux::graph {

void dependency_graph::register_reactive(const std::string& node, const std::vector<std::uint8_t>& expr) {
    reactive_expr_[node] = expr;
    meta_[node];
    if (meta_[node].topo_index == 0) {
        meta_[node].topo_index = ++next_topo_index_;
    }
    recompute_topo_index();
}

void dependency_graph::connect_dependency(const std::string& dependency, const std::string& reactive_node) {
    if (dependency == reactive_node) throw std::runtime_error("cycle detected: self edge");
    if (has_path(reactive_node, dependency)) throw std::runtime_error("cycle detected: edge creates back path");
    dependents_[dependency].insert(reactive_node);
    meta_[dependency];
    meta_[reactive_node];
    const auto dependency_level = meta_[dependency].topo_level;
    if (meta_[reactive_node].topo_level < dependency_level + 1) {
        meta_[reactive_node].topo_level = dependency_level + 1;
        propagate_topo_level(reactive_node);
    }
    recompute_topo_index();
}

const std::unordered_map<std::string, std::vector<std::uint8_t>>& dependency_graph::reactive_expressions() const { return reactive_expr_; }
const std::unordered_map<std::string, std::unordered_set<std::string>>& dependency_graph::dependents() const { return dependents_; }

node_meta& dependency_graph::meta_mut(const std::string& node) { return meta_[node]; }
const node_meta& dependency_graph::meta(const std::string& node) const { return meta_.at(node); }

bool dependency_graph::has_path(const std::string& from, const std::string& to) const {
    if (from == to) return true;
    std::unordered_set<std::string> visited;
    std::queue<std::string> q;
    q.push(from);
    visited.insert(from);
    while (!q.empty()) {
        auto cur = q.front();
        q.pop();
        auto it = dependents_.find(cur);
        if (it == dependents_.end()) continue;
        for (const auto& next : it->second) {
            if (next == to) return true;
            if (visited.insert(next).second) q.push(next);
        }
    }
    return false;
}

void dependency_graph::propagate_topo_level(const std::string& start) {
    std::queue<std::string> q;
    q.push(start);
    while (!q.empty()) {
        auto cur = q.front();
        q.pop();
        auto it = dependents_.find(cur);
        if (it == dependents_.end()) continue;
        for (const auto& dep : it->second) {
            const auto candidate = meta_[cur].topo_level + 1;
            if (meta_[dep].topo_level < candidate) {
                meta_[dep].topo_level = candidate;
                q.push(dep);
            }
        }
    }
}

void dependency_graph::recompute_topo_index() {
    std::unordered_map<std::string, std::size_t> indegree;
    for (const auto& [node, _] : meta_) indegree[node] = 0;
    for (const auto& [u, deps] : dependents_) {
        indegree[u] += 0;
        for (const auto& v : deps) indegree[v] += 1;
    }
    std::vector<std::string> zeros;
    for (const auto& [node, d] : indegree) if (d == 0) zeros.push_back(node);
    std::sort(zeros.begin(), zeros.end());
    std::queue<std::string> q;
    for (const auto& n : zeros) q.push(n);
    std::uint64_t idx = 0;
    while (!q.empty()) {
        auto cur = q.front();
        q.pop();
        meta_[cur].topo_index = ++idx;
        auto it = dependents_.find(cur);
        if (it == dependents_.end()) continue;
        std::vector<std::string> outs(it->second.begin(), it->second.end());
        std::sort(outs.begin(), outs.end());
        for (const auto& nxt : outs) {
            if (--indegree[nxt] == 0) q.push(nxt);
        }
    }
}

}
