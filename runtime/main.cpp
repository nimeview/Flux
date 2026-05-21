#include "graph/graph.h"
#include "scheduler/scheduler.h"
#include "runtime_api/runtime_api.h"
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
    auto ru8=[&](){char b; in.read(&b,1); if(!in) throw std::runtime_error("bad bytecode"); return (uint8_t)b;};
    auto ru16=[&](){uint16_t v; in.read(reinterpret_cast<char*>(&v),2); if(!in) throw std::runtime_error("bad bytecode"); return v;};
    auto ru32=[&](){uint32_t v; in.read(reinterpret_cast<char*>(&v),4); if(!in) throw std::runtime_error("bad bytecode"); return v;};
    char magic[4]; in.read(magic,4); if(std::string(magic,4)!="FLXB") throw std::runtime_error("bad magic");
    uint8_t ver=ru8(); if(ver!=2) throw std::runtime_error("unsupported bytecode version");
    uint32_t count=ru32(); std::vector<instruction> out; out.reserve(count);
    for(uint32_t i=0;i<count;++i){ instruction inst{}; inst.kind=(instruction_kind)ru8(); uint16_t nl=ru16(); inst.name.resize(nl); in.read(inst.name.data(),nl); uint32_t pl=ru32(); inst.payload.resize(pl); in.read((char*)inst.payload.data(),pl); if(!in) throw std::runtime_error("bad payload"); out.push_back(std::move(inst)); }
    return out;
}

static double as_number(const value& v){ if(auto p=std::get_if<double>(&v)) return *p; throw std::runtime_error("expected number"); }

static value eval_expr(const std::vector<uint8_t>& code, const std::unordered_map<std::string, value>& values){
    std::vector<value> st; size_t ip=0;
    auto r16=[&](){ if(ip+2>code.size()) throw std::runtime_error("truncated"); uint16_t v=*reinterpret_cast<const uint16_t*>(&code[ip]); ip+=2; return v;};
    auto r64=[&](){ if(ip+8>code.size()) throw std::runtime_error("truncated"); double v=*reinterpret_cast<const double*>(&code[ip]); ip+=8; return v;};
    while(ip<code.size()){
        uint8_t op=code[ip++];
        if(op==1){ st.push_back(r64()); }
        else if(op==2){ if(ip>=code.size()) throw std::runtime_error("truncated bool"); st.push_back(code[ip++]!=0); }
        else if(op==3){ uint16_t n=r16(); if(ip+n>code.size()) throw std::runtime_error("truncated name"); std::string nm((const char*)&code[ip], n); ip+=n; auto it=values.find(nm); if(it==values.end()) throw std::runtime_error("unknown name: "+nm); st.push_back(it->second); }
        else if(op>=4 && op<=7){ if(st.size()<2) throw std::runtime_error("stack underflow"); auto b=st.back();st.pop_back(); auto a=st.back();st.pop_back(); double av=as_number(a), bv=as_number(b); if(op==4) st.push_back(av+bv); else if(op==5) st.push_back(av-bv); else if(op==6) st.push_back(av*bv); else { if(bv==0.0) throw std::runtime_error("division by zero"); st.push_back(av/bv);} }
        else if(op==8){ uint16_t n=r16(); if(ip+n>code.size()) throw std::runtime_error("truncated call"); std::string fn((const char*)&code[ip],n); ip+=n; if(ip>=code.size()) throw std::runtime_error("truncated argc"); uint8_t argc=code[ip++]; if(st.size()<argc) throw std::runtime_error("stack underflow"); std::vector<value> args(argc); for(size_t i=0;i<argc;++i){ args[argc-1-i]=st.back(); st.pop_back(); } st.push_back(flux::stdlib::call(fn,args)); }
        else throw std::runtime_error("unknown opcode");
    }
    if(st.size()!=1) throw std::runtime_error("bad expr"); return st.back();
}

static std::unordered_set<std::string> collect_deps(const std::vector<uint8_t>& code){
    std::unordered_set<std::string> out; size_t ip=0;
    auto r16=[&](){ uint16_t v=*reinterpret_cast<const uint16_t*>(&code[ip]); ip+=2; return v;};
    while(ip<code.size()){
        uint8_t op=code[ip++];
        if(op==1) ip+=8;
        else if(op==2) ip+=1;
        else if(op==3){ uint16_t n=r16(); std::string nm((const char*)&code[ip], n); ip+=n; out.insert(nm); }
        else if(op>=4 && op<=7) {}
        else if(op==8){ uint16_t n=r16(); ip+=n; ip+=1; }
    }
    return out;
}

int main(int argc, char** argv){
    if(argc < 2 || argc > 3) return 1;
    const bool trace_flag = (argc == 3 && std::string(argv[2]) == "--trace");
    flux::runtime_api::set_trace(trace_flag);
    auto program=decode_binary(argv[1]);
    flux::graph::dependency_graph graph;
    flux::scheduler::recompute_scheduler scheduler;
    std::unordered_map<std::string,value> values;
    std::uint64_t tick = 0;

    std::vector<std::string> pending_sources;
    auto flush_tick = [&]() {
        if (pending_sources.empty()) return;
        tick += 1;
        scheduler.recompute_batch(pending_sources, graph, values, tick, [&](const std::vector<uint8_t>& e){ return eval_expr(e, values); }, [&](const std::string& m){ flux::runtime_api::trace(m); });
        pending_sources.clear();
    };

    for(const auto& inst: program){
        if(inst.kind==instruction_kind::expr) {
            flush_tick();
            (void)eval_expr(inst.payload, values);
        }
        else if(inst.kind==instruction_kind::signal_decl) {
            values[inst.name]=eval_expr(inst.payload, values);
            graph.meta_mut(inst.name).version += 1;
        }
        else if(inst.kind==instruction_kind::reactive_decl){
            for(const auto& d: collect_deps(inst.payload)) graph.connect_dependency(d, inst.name);
            graph.register_reactive(inst.name, inst.payload);
            values[inst.name]=eval_expr(inst.payload, values);
            graph.meta_mut(inst.name).version += 1;
        }
        else if(inst.kind==instruction_kind::set_value){
            values[inst.name]=eval_expr(inst.payload, values);
            graph.meta_mut(inst.name).version += 1;
            pending_sources.push_back(inst.name);
        }
    }
    flush_tick();

    if (flux::runtime_api::trace_enabled()) {
        flux::runtime_api::trace("graph inspector: nodes=" + std::to_string(graph.reactive_expressions().size()));
        for (const auto& [name, _] : graph.reactive_expressions()) {
            const auto& meta = graph.meta(name);
            flux::runtime_api::trace("node=" + name + " version=" + std::to_string(meta.version) + " last_tick=" + std::to_string(meta.last_eval_tick));
        }
    }
    return 0;
}
