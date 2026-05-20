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
using value = flux::stdlib::value;

static std::vector<instruction> decode_binary(const std::string& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) throw std::runtime_error("cannot open bytecode file");
    auto read_u8 = [&]() -> uint8_t { char b; in.read(&b,1); if(!in) throw std::runtime_error("bad bytecode"); return (uint8_t)b; };
    auto read_u16 = [&]() -> uint16_t { uint16_t v; in.read(reinterpret_cast<char*>(&v),2); if(!in) throw std::runtime_error("bad bytecode"); return v; };
    auto read_u32 = [&]() -> uint32_t { uint32_t v; in.read(reinterpret_cast<char*>(&v),4); if(!in) throw std::runtime_error("bad bytecode"); return v; };
    char magic[4]; in.read(magic,4); if(std::string(magic,4)!="FLXB") throw std::runtime_error("bad magic");
    auto ver = read_u8(); if(ver!=1) throw std::runtime_error("unsupported version");
    auto count = read_u32();
    std::vector<instruction> out; out.reserve(count);
    for(uint32_t i=0;i<count;++i){
        instruction inst{};
        inst.kind = static_cast<instruction_kind>(read_u8());
        auto name_len = read_u16();
        inst.name.resize(name_len); in.read(inst.name.data(), name_len);
        auto payload_len = read_u32();
        inst.payload_rpn.resize(payload_len); in.read(inst.payload_rpn.data(), payload_len);
        if(!in) throw std::runtime_error("bad bytecode payload");
        out.push_back(std::move(inst));
    }
    return out;
}

static std::unordered_set<std::string> collect_deps(const std::string& rpn) {
    std::unordered_set<std::string> out;
    for (const auto& tok : flux::runtime_api::split_tokens(rpn)) {
        if (tok == "+" || tok == "-" || tok == "*" || tok == "/") continue;
        if (tok.rfind("@call:", 0) == 0) continue;
        if (tok == "true" || tok == "false") continue;
        try { (void)std::stod(tok); }
        catch (...) { out.insert(tok); }
    }
    return out;
}

static double as_number(const value& v) {
    if (auto p = std::get_if<double>(&v)) return *p;
    throw std::runtime_error("expected number");
}

static value eval_rpn(const std::string& rpn, const std::unordered_map<std::string, value>& values) {
    std::vector<value> st;
    for (const auto& tok : flux::runtime_api::split_tokens(rpn)) {
        if (tok == "+" || tok == "-" || tok == "*" || tok == "/") {
            if (st.size() < 2) throw std::runtime_error("bad expression");
            auto b = st.back(); st.pop_back();
            auto a = st.back(); st.pop_back();
            const double av = as_number(a), bv = as_number(b);
            if (tok == "+") st.push_back(av + bv);
            else if (tok == "-") st.push_back(av - bv);
            else if (tok == "*") st.push_back(av * bv);
            else {
                if (bv == 0.0) throw std::runtime_error("division by zero");
                st.push_back(av / bv);
            }
        } else if (tok.rfind("@call:", 0) == 0) {
            const auto p1 = tok.find(':', 6);
            const auto fn = tok.substr(6, p1 - 6);
            const auto argc = std::stoul(tok.substr(p1 + 1));
            if (st.size() < argc) throw std::runtime_error("bad call stack");
            std::vector<value> args(argc);
            for (size_t i = 0; i < argc; ++i) { args[argc - 1 - i] = st.back(); st.pop_back(); }
            st.push_back(flux::stdlib::call(fn, args));
        } else if (tok == "true" || tok == "false") {
            st.push_back(tok == "true");
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

    const auto program = decode_binary(argv[1]);
    flux::graph::dependency_graph graph;
    flux::scheduler::recompute_scheduler scheduler;
    std::unordered_map<std::string, value> values;

    for (const auto& inst : program) {
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
