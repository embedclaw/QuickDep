; Rust symbol, import, and call extraction rules for QuickDep.

(function_item
  (identifier) @name.function) @definition.function

(struct_item
  (type_identifier) @name.struct) @definition.struct

(enum_item
  (type_identifier) @name.enum) @definition.enum

(enum_variant
  (identifier) @name.enum_variant) @definition.enum_variant

(trait_item
  (type_identifier) @name.trait) @definition.trait

(type_item
  (type_identifier) @name.type_alias) @definition.type_alias

(mod_item
  (identifier) @name.module) @definition.module

(const_item
  (identifier) @name.constant) @definition.constant

(static_item
  (identifier) @name.variable) @definition.variable

(macro_definition
  (identifier) @name.macro) @definition.macro

(use_declaration) @import

(call_expression
  function: (_) @reference.call) @reference.call.expression

(macro_invocation
  macro: (_) @reference.call) @reference.call.expression
