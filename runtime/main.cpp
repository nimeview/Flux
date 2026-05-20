#include "graph/graph.h"
#include "runtime_api/runtime_api.h"
#include "scheduler/scheduler.h"
#include "../shared/bytecode.hpp"
#include "../stdlib/std.hpp"

#include <fstream>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

using flux::shared::instruction;
using flux::shared::instruction_kind;

static instruction parse_instruction(const std::string& line) {
    const auto trimmed = flux::runtime_api::trim(line);
    if (trimmed.rfind("expr ", 0) == 0) {
        return {instruction_kind::expr, "", flux::runtime_api::trim(trimmed.substr(5))};
    }

    const auto eq = trimmed.find('=');
    const auto lhs = flux::runtime_api::trim(trimmed.substr(0, eq));
    const auto rhs = flux::runtime_api::trim(trimmed.substr(eq + 1));
    const auto sp = lhs.find(' ');
    const auto kind = lhs.substr(0, sp);
    const auto name = flux::runtime_api::trim(lhs.substr(sp + 1));

    if (kind == "signal") return {instruction_kind::signal_decl, name, rhs};
    if (kind == "reactive") return {instruction_kind::reactive_decl, name, rhs};
    if (kind == "set") return {instruction_kind::set_value, name, rhs};
    throw std::runtime_error("unknown instruction kind: " + kind);
}

static std::unordered_set<std::string> collect_deps(const std::string& rpn) {
    std::unordered_set<std::string> out;
    for (const auto& tok : flux::runtime_api::split_tokens(rpn)) {
        if (tok == "+" || tok == "-" || tok == "*" || tok == "/") continue;
        if (tok.rfind("@call:", 0) == 0) continue;
        try { (void)std::stod(tok); }
        catch (...) { out.insert(tok); }
    }
    return out;
}

static double eval_rpn(const std::string& rpn, const std::unordered_map<std::string, double>& values) {
    std::vector<double> st;
    for (const auto& tok : flux::runtime_api::split_tokens(rpn)) {
        if (tok == "+" || tok == "-" || tok == "*" || tok == "/") {
            if (st.size() < 2) throw std::runtime_error("bad expression");
            double b = st.back(); st.pop_back();
            double a = st.back(); st.pop_back();
            if (tok == "+") st.push_back(a + b);
            else if (tok == "-") st.push_back(a - b);
            else if (tok == "*") st.push_back(a * b);
            else {
                if (b == 0.0) throw std::runtime_error("division by zero");
                st.push_back(a / b);
            }
        } else if (tok.rfind("@call:", 0) == 0) {
            const auto p1 = tok.find(':', 6);
            const auto fn = tok.substr(6, p1 - 6);
            const auto argc = std::stoul(tok.substr(p1 + 1));
            if (st.size() < argc) throw std::runtime_error("bad call stack");
            std::vector<double> args(argc);
            for (size_t i = 0; i < argc; ++i) { args[argc - 1 - i] = st.back(); st.pop_back(); }
            st.push_back(flux::stdlib::call(fn, args));
        } else {
            try { st.push_back(std::stod(tok)); }
            catch (...) {
                auto it = values.find(tok);
                if (it == values.end()) throw std::runtime_error("unknown name: " + tok);
                st.push_back(it->second);
            }
        }
    }
    if (st.size() != 1) throw std::runtime_error("bad expression");
    return st.back();
}

int main(int argc, char** argv) {
    if (argc != 2) return 1;

    std::ifstream in(argv[1]);
    if (!in) return 1;

    flux::graph::dependency_graph graph;
    flux::scheduler::recompute_scheduler scheduler;
    std::unordered_map<std::string, double> values;

    std::string line;
    while (std::getline(in, line)) {
        const auto trimmed = flux::runtime_api::trim(line);
        if (trimmed.empty()) continue;

        const auto inst = parse_instruction(trimmed);
        if (inst.kind == instruction_kind::expr) {
            (void)eval_rpn(inst.payload_rpn, values);
        } else if (inst.kind == instruction_kind::signal_decl) {
            values[inst.name] = eval_rpn(inst.payload_rpn, values);
        } else if (inst.kind == instruction_kind::reactive_decl) {
            for (const auto& dep : collect_deps(inst.payload_rpn)) graph.connect_dependency(dep, inst.name);
            graph.register_reactive(inst.name, inst.payload_rpn);
            values[inst.name] = eval_rpn(inst.payload_rpn, values);
        } else if (inst.kind == instruction_kind::set_value) {
            values[inst.name] = eval_rpn(inst.payload_rpn, values);
            scheduler.recompute_from(inst.name, graph.dependents(), graph.reactive_expressions(), values,
                [&](const std::string& expr) { return eval_rpn(expr, values); });
        }
    }

    return 0;
}
