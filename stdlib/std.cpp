#include "std.hpp"

#include <chrono>
#include <cmath>
#include <iostream>
#include <stdexcept>

namespace flux::stdlib {

static double as_number(const value& v) {
    if (auto p = std::get_if<double>(&v)) return *p;
    throw std::runtime_error("expected number");
}

value call(const std::string& fn, const std::vector<value>& args) {
    if (fn == "print") {
        if (args.size() != 1) throw std::runtime_error("print expects 1 argument");
        if (auto n = std::get_if<double>(&args[0])) std::cout << *n << "\n";
        else if (auto b = std::get_if<bool>(&args[0])) std::cout << (*b ? "true" : "false") << "\n";
        else std::cout << "[vec len=" << std::get<std::vector<double>>(args[0]).size() << "]\n";
        return args[0];
    }
    if (fn == "sqrt") {
        if (args.size() != 1) throw std::runtime_error("sqrt expects 1 argument");
        return std::sqrt(as_number(args[0]));
    }
    if (fn == "min") {
        if (args.size() != 2) throw std::runtime_error("min expects 2 arguments");
        return std::min(as_number(args[0]), as_number(args[1]));
    }
    if (fn == "max") {
        if (args.size() != 2) throw std::runtime_error("max expects 2 arguments");
        return std::max(as_number(args[0]), as_number(args[1]));
    }
    if (fn == "now_ms") {
        if (!args.empty()) throw std::runtime_error("now_ms expects 0 arguments");
        auto now = std::chrono::duration_cast<std::chrono::milliseconds>(
            std::chrono::system_clock::now().time_since_epoch()).count();
        return static_cast<double>(now);
    }
    if (fn == "vec") {
        std::vector<double> out;
        out.reserve(args.size());
        for (const auto& a : args) out.push_back(as_number(a));
        return out;
    }
    if (fn == "len") {
        if (args.size() != 1) throw std::runtime_error("len expects 1 argument");
        if (auto arr = std::get_if<std::vector<double>>(&args[0])) return static_cast<double>(arr->size());
        throw std::runtime_error("len expects vec");
    }
    throw std::runtime_error("unknown stdlib function: " + fn);
}

} // namespace flux::stdlib
