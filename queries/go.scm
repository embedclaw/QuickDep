; Go symbol, import, and call extraction rules for QuickDep.

(function_declaration
  name: (identifier) @name.function) @definition.function

(method_declaration
  name: (field_identifier) @name.method) @definition.method

(type_spec
  name: (type_identifier) @name.type) @definition.type

(type_alias
  name: (type_identifier) @name.type_alias) @definition.type_alias

(method_elem
  name: (field_identifier) @name.method) @definition.method

(const_spec
  name: (identifier) @name.constant) @definition.constant

(var_spec
  name: (identifier) @name.variable) @definition.variable

(import_spec) @import

(call_expression
  function: (_) @reference.call) @reference.call.expression
