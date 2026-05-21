#include "../graph/graph.h"
#include "../scheduler/scheduler.h"

#include <cassert>
#include <random>
#include <sstream>
#include <string>
#include <unordered_map>

using flux::graph::dependency_graph;
using flux::scheduler::recompute_scheduler;
using flux::stdlib::value;

static std::vector<std::uint8_t> expr_stub(std::uint8_t seed) { return {seed}; }

int main() {
    {
        dependency_graph g;
        g.register_reactive("a", expr_stub(1));
        g.register_reactive("b", expr_stub(2));
        g.connect_dependency("a", "b");
        bool cycle_thrown = false;
        try { g.connect_dependency("b", "a"); } catch (...) { cycle_thrown = true; }
        assert(cycle_thrown);
    }

    {
        dependency_graph g;
        recompute_scheduler s;
        std::unordered_map<std::string, value> values;
        g.register_reactive("mid", expr_stub(1));
        g.register_reactive("leaf", expr_stub(2));
        g.connect_dependency("src", "mid");
        g.connect_dependency("mid", "leaf");
        std::vector<std::string> trace_log;
        s.recompute_batch({"src"}, g, values, 1, [](const auto&) { return value(1.0); }, [&](const std::string& m) { trace_log.push_back(m); });
        assert(trace_log.size() >= 4);
    }

    {
        // Property-like determinism check over repeated random input streams.
        std::mt19937 rng(42);
        std::uniform_int_distribution<int> dist(0, 9);
        auto run_once = [&](unsigned seed) {
            std::mt19937 local_rng(seed);
            dependency_graph g;
            recompute_scheduler s;
            std::unordered_map<std::string, value> values;
            g.register_reactive("n1", expr_stub(1));
            g.register_reactive("n2", expr_stub(2));
            g.register_reactive("n3", expr_stub(3));
            g.connect_dependency("src0", "n1");
            g.connect_dependency("src1", "n2");
            g.connect_dependency("n1", "n3");
            g.connect_dependency("n2", "n3");
            std::ostringstream out;
            for (int t = 1; t <= 50; ++t) {
                std::vector<std::string> changed = {"src" + std::to_string(dist(local_rng) % 2)};
                s.recompute_batch(changed, g, values, t, [](const auto&) { return value(1.0); }, [&](const std::string& m) { out << m << "\n"; });
            }
            return out.str();
        };
        auto trace1 = run_once(7);
        auto trace2 = run_once(7);
        assert(trace1 == trace2);
    }

    {
        // Load-ish scenario: 1000+ nodes, fan-out/fan-in.
        dependency_graph g;
        recompute_scheduler s;
        std::unordered_map<std::string, value> values;
        for (int i = 0; i < 1200; ++i) g.register_reactive("n" + std::to_string(i), expr_stub(static_cast<std::uint8_t>(i % 255)));
        for (int i = 0; i < 400; ++i) g.connect_dependency("src", "n" + std::to_string(i));
        for (int i = 400; i < 1200; ++i) g.connect_dependency("n" + std::to_string(i % 400), "n" + std::to_string(i));
        std::size_t traces = 0;
        s.recompute_batch({"src"}, g, values, 1, [](const auto&) { return value(1.0); }, [&](const std::string&) { traces += 1; });
        assert(!values.empty());
        assert(traces > 1000);
    }
}
