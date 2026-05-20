#include "../../stdlib/std.hpp"

#include <iostream>
#include <stdexcept>

namespace flux::stdlib {

bool is_supported(const std::string& fn) {
    return fn == "print";
}

std::size_t arity(const std::string& fn) {
    if (fn == "print") return 1;
    throw std::runtime_error("unknown stdlib function: " + fn);
}

double call(const std::string& fn, const std::vector<double>& args) {
    if (fn == "print") {
        if (args.size() != 1) throw std::runtime_error("print expects 1 argument");
        std::cout << args[0] << "\n";
        return args[0];
    }
    throw std::runtime_error("unknown stdlib function: " + fn);
}

} // namespace flux::stdlib
