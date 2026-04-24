require_relative "shared/helper"

module Acme
  module Shared
    module Formatter
      def format(name)
        name.strip
      end
    end
  end
end

class BaseService
end

class UserService < BaseService
  include Acme::Shared::Formatter

  def greet(name)
    helper = Helper.new
    helper.decorate(format(name))
  end
end
