; Python symbol, import, and call extraction rules for QuickDep.

(function_definition
  name: (identifier) @name.function) @definition.function

(class_definition
  name: (identifier) @name.class) @definition.class

(import_statement) @import

(import_from_statement) @import

(call
  function: (_) @reference.call) @reference.call.expression
