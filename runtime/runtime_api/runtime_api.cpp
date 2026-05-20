#include "runtime_api.h"

#include <iostream>
#include <sstream>

namespace flux::runtime_api {

static bool g_trace_enabled = false;

std::vector<std::string> split_tokens(const std::string& text) {
    std::istringstream iss(text);
    std::vector<std::string> out;
    std::string token;
    while (iss >> token) {
        out.push_back(token);
    }
    return out;
}

std::string trim(const std::string& value) {
    const auto start = value.find_first_not_of(" \t");
    if (start == std::string::npos) return "";
    const auto end = value.find_last_not_of(" \t");
    return value.substr(start, end - start + 1);
}

void set_trace(bool enabled) { g_trace_enabled = enabled; }
bool trace_enabled() { return g_trace_enabled; }

void trace(const std::string& message) {
    if (!g_trace_enabled) return;
    std::cerr << "[trace] " << message << "\n";
}

} // namespace flux::runtime_api
