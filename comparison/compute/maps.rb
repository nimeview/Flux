counts = Hash.new(0)
200000.times { |i| counts["k#{i % 5000}"] += i % 7 }
puts "#{counts.size} #{counts.values.sum}"
