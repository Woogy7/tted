# frozen_string_literal: true

class Greeting
  def initialize(name)
    @name = name
  end

  def call(times: 1)
    Array.new(times) { |index| "#{index + 1}. Hello, #{@name}!" }
  end
end

puts Greeting.new("TTED").call(times: 3)
