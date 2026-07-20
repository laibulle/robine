/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  equality: 1,
  comparison: 2,
  additive: 3,
  multiplicative: 4,
};

module.exports = grammar({
  name: "robine",

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) =>
      seq(
        $.module_declaration,
        repeat($.import_declaration),
        repeat($.function_declaration),
      ),

    module_declaration: ($) =>
      seq("module", field("name", $.qualified_identifier)),

    import_declaration: ($) =>
      seq("import", field("module", $.qualified_identifier)),

    function_declaration: ($) =>
      seq(
        optional(field("visibility", "pub")),
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
        $.if_expression,
        $.binary_expression,
        $.call_expression,
        $.parenthesized_expression,
        $.identifier,
        $.string,
        $.integer,
        $.boolean,
      ),

    if_expression: ($) =>
      seq(
        "if",
        field("condition", $.expression),
        field("consequence", $.expression_block),
        "else",
        field("alternative", $.expression_block),
      ),

    expression_block: ($) =>
      seq("{", field("result", $.expression), "}"),

    binary_expression: ($) =>
      choice(
        binary($, "==", PREC.equality),
        binary($, "<", PREC.comparison),
        binary($, "<=", PREC.comparison),
        binary($, "+", PREC.additive),
        binary($, "-", PREC.additive),
        binary($, "*", PREC.multiplicative),
      ),

    parenthesized_expression: ($) =>
      seq("(", field("value", $.expression), ")"),

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

function binary($, operator, precedence) {
  return prec.left(
    precedence,
    seq(
      field("left", $.expression),
      field("operator", operator),
      field("right", $.expression),
    ),
  );
}
