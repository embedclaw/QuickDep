; TypeScript symbol, import, and call extraction rules for QuickDep.

(function_declaration
  name: (identifier) @name.function) @definition.function

(class_declaration
  name: (type_identifier) @name.class) @definition.class

(abstract_class_declaration
  name: (type_identifier) @name.class) @definition.class

(interface_declaration
  name: (type_identifier) @name.interface) @definition.interface

(method_definition
  name: (_) @name.method) @definition.method

(method_signature
  name: (_) @name.method) @definition.method

(abstract_method_signature
  name: (_) @name.method) @definition.method

(public_field_definition
  name: (_) @name.property) @definition.property

(type_alias_declaration
  name: (type_identifier) @name.type_alias) @definition.type_alias

(variable_declarator
  name: (identifier) @name.variable
  value: [
    (arrow_function)
    (function_expression)
  ]) @definition.function.binding

(import_statement) @import

(call_expression
  function: (_) @reference.call) @reference.call.expression

(new_expression
  constructor: (_) @reference.call) @reference.call.expression
