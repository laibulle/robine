/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
  name: "robine",

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) => seq($.module_declaration, repeat($.function_declaration)),

    module_declaration: ($) =>
      seq("module", field("name", $.identifier)),

    function_declaration: ($) =>
      seq(
        "fn",
        field("name", $.identifier),
        "(",
        optional(commaSep1($.parameter)),
        ")",
        "->",
        field("return_type", $.type_name),
        optional($.effect_row),
        field("body", $.block),
      ),

    parameter: ($) =>
      seq(
        field("name", $.identifier),
        ":",
        field("type", $.type_name),
      ),

    type_name: ($) => $.qualified_identifier,

    effect_row: ($) =>
      seq("!", "{", optional(commaSep1($.qualified_identifier)), "}"),

    block: ($) =>
      seq(
        "{",
        repeat(choice($.let_statement, $.expression_statement)),
        optional(field("result", $.expression)),
        "}",
      ),

    let_statement: ($) =>
      seq(
        "let",
        field("name", $.identifier),
        optional(seq(":", field("type", $.type_name))),
        "=",
        field("value", $.expression),
        ";",
      ),

    expression_statement: ($) => seq($.expression, ";"),

    expression: ($) =>
      choice(
        $.call_expression,
        $.identifier,
        $.string,
        $.integer,
        $.boolean,
      ),

    call_expression: ($) =>
      seq(
        field("function", $.qualified_identifier),
        "(",
        optional(commaSep1(field("argument", $.expression))),
        ")",
      ),

    qualified_identifier: ($) =>
      seq($.identifier, repeat(seq(".", $.identifier))),

    identifier: () => /[\p{XID_Start}_][\p{XID_Continue}]*/u,
    integer: () => /\d+/,
    boolean: () => choice("true", "false"),
    string: () =>
      seq(
        '"',
        repeat(choice(/[^"\\\n]+/, /\\["\\nrt]/)),
        '"',
      ),
    comment: () => token(seq("//", /.*/)),
  },
});

function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}
