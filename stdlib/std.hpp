#ifndef FLUX_STDLIB_STD_HPP
#define FLUX_STDLIB_STD_HPP

#include <string>
#include <variant>
#include <vector>

namespace flux::stdlib {

using value = std::variant<double, bool, std::vector<double>>;

value call(const std::string& fn, const std::vector<value>& args);

} // namespace flux::stdlib

#endif // FLUX_STDLIB_STD_HPP
