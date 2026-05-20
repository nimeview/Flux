#ifndef FLUX_STDLIB_STD_HPP
#define FLUX_STDLIB_STD_HPP

#include <cstddef>
#include <string>
#include <vector>

namespace flux::stdlib {

bool is_supported(const std::string& fn);
double call(const std::string& fn, const std::vector<double>& args);
std::size_t arity(const std::string& fn);

} // namespace flux::stdlib

#endif // FLUX_STDLIB_STD_HPP
