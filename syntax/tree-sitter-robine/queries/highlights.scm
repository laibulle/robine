(comment) @comment
(string) @string
(integer) @number
(boolean) @boolean

[
  "module"
  "fn"
  "let"
] @keyword

(module_declaration
  name: (identifier) @namespace)

(function_declaration
  name: (identifier) @function)

(parameter
  name: (identifier) @variable.parameter)

(type_name
  (qualified_identifier
    (identifier) @type))

(effect_row
  (qualified_identifier
    (identifier) @type))

(call_expression
  function: (qualified_identifier
    (identifier) @function.call))

(identifier) @variable

[
  "("
  ")"
  "{"
  "}"
] @punctuation.bracket

[
  ","
  ":"
  "."
  ";"
] @punctuation.delimiter

[
  "!"
  "="
  "->"
] @operator
