#ifndef FLUX_RUNTIME_API_H
#define FLUX_RUNTIME_API_H

#include <string>
#include <vector>

namespace flux::runtime_api {

std::vector<std::string> split_tokens(const std::string& text);
std::string trim(const std::string& value);

} // namespace flux::runtime_api

#endif // FLUX_RUNTIME_API_H
