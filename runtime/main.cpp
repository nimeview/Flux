#include <fstream>
#include <iostream>
#include <queue>
#include <sstream>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

struct Runtime {
    std::unordered_map<std::string, double> values;
    std::unordered_map<std::string, std::string> reactive_rpn;
    std::unordered_map<std::string, std::unordered_set<std::string>> deps;

    double eval_rpn(const std::string& rpn) {
        std::vector<double> st;
        std::istringstream iss(rpn);
        std::string tok;
        while (iss >> tok) {
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
                auto p1 = tok.find(':', 6);
                auto fn = tok.substr(6, p1 - 6);
                auto argc = std::stoul(tok.substr(p1 + 1));
                if (st.size() < argc) throw std::runtime_error("bad call stack");

                std::vector<double> args(argc);
                for (size_t i = 0; i < argc; ++i) {
                    args[argc - 1 - i] = st.back();
                    st.pop_back();
                }

                if (fn == "print") {
                    if (argc != 1) throw std::runtime_error("print expects 1 argument");
                    std::cout << args[0] << "\n";
                    st.push_back(args[0]);
                } else {
                    throw std::runtime_error("unknown stdlib function: " + fn);
                }
            } else {
                try { st.push_back(std::stod(tok)); }
                catch (...) {
                    if (!values.contains(tok)) throw std::runtime_error("unknown name: " + tok);
                    st.push_back(values[tok]);
                }
            }
        }
        if (st.size() != 1) throw std::runtime_error("bad expression");
        return st.back();
    }

    void recompute(const std::string& src) {
        std::queue<std::string> q;
        std::unordered_set<std::string> seen;
        q.push(src);
        while (!q.empty()) {
            auto n = q.front(); q.pop();
            if (!deps.contains(n)) continue;
            for (const auto& dep : deps[n]) {
                if (!seen.insert(dep).second) continue;
                values[dep] = eval_rpn(reactive_rpn[dep]);
                q.push(dep);
            }
        }
    }
};

static std::unordered_set<std::string> collect_deps(const std::string& rpn) {
    std::unordered_set<std::string> out;
    std::istringstream iss(rpn);
    std::string tok;
    while (iss >> tok) {
        if (tok == "+" || tok == "-" || tok == "*" || tok == "/") continue;
        if (tok.rfind("@call:", 0) == 0) continue;
        try { (void)std::stod(tok); }
        catch (...) { out.insert(tok); }
    }
    return out;
}

int main(int argc, char** argv) {
    if (argc != 2) { std::cerr << "Usage: flux_runtime <program.fxbc>\n"; return 1; }
    std::ifstream in(argv[1]);
    if (!in) { std::cerr << "failed to open " << argv[1] << "\n"; return 1; }

    Runtime rt;
    auto trim=[](std::string v){ auto b=v.find_first_not_of(" \t"); auto e=v.find_last_not_of(" \t"); return b==std::string::npos?std::string():v.substr(b,e-b+1); };
    std::string line;
    while (std::getline(in, line)) {
        line = trim(line);
        if (line.empty()) continue;

        if (line.rfind("expr ", 0) == 0) {
            (void)rt.eval_rpn(trim(line.substr(5)));
            continue;
        }

        auto eq = line.find('=');
        auto lhs = trim(line.substr(0, eq));
        auto rhs = trim(line.substr(eq + 1));
        auto sp = lhs.find(' ');
        auto kind = lhs.substr(0, sp);
        auto name = trim(lhs.substr(sp + 1));

        if (kind == "signal") {
            rt.values[name] = rt.eval_rpn(rhs);
        } else if (kind == "reactive") {
            for (const auto& dep : collect_deps(rhs)) rt.deps[dep].insert(name);
            rt.reactive_rpn[name] = rhs;
            rt.values[name] = rt.eval_rpn(rhs);
        } else if (kind == "set") {
            rt.values[name] = rt.eval_rpn(rhs);
            rt.recompute(name);
        }
    }
    return 0;
}
