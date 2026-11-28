parts = (0...200000).map { |i| "item#{i % 1000}" }
words = parts.join(",").split(",")
puts "#{words.size} #{words.select { |w| w.end_with?("7") }.sum(&:size)}"
