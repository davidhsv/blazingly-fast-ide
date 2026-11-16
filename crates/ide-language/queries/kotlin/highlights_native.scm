; The token colouring Kotlin's declaration-role query (highlights.scm) is joined after, for a UI
; that has no token lexer of its own: every name a variable, keywords (soft ones too, wherever
; they stand), the literal atoms, strings, numbers, comments, operators, annotations and the
; delimiters of string templates.

(identifier) @variable

((identifier) @type
  (#any-of? @type
    "Boolean" "Byte" "Character" "CharSequence" "Class" "ClassLoader" "Cloneable" "Comparable"
    "Compiler" "Double" "Exception" "Float" "Integer" "Long" "Math" "Number" "Object" "Package"
    "Pair" "Process" "Runtime" "Runnable" "SecurityManager" "Short" "StackTraceElement"
    "StrictMath" "String" "StringBuffer" "System" "Thread" "ThreadGroup" "ThreadLocal" "Throwable"
    "Triple" "Void" "Annotation" "Any" "BooleanArray" "ByteArray" "Char" "CharArray"
    "DeprecationLevel" "DoubleArray" "Enum" "FloatArray" "Function" "Int" "IntArray" "Lazy"
    "LazyThreadSafetyMode" "LongArray" "Nothing" "ShortArray" "Unit"))

((identifier) @constant.builtin
  (#any-of? @constant.builtin "true" "false" "null"))

((identifier) @keyword
  (#any-of? @keyword
    "This" "break" "continue" "file" "import" "where" "by" "get" "set" "abstract" "enum" "open"
    "inner" "override" "private" "public" "internal" "protected" "catch" "finally" "out" "final"
    "vararg" "reified" "dynamic" "companion" "constructor" "init" "sealed" "field" "property"
    "receiver" "param" "sparam" "lateinit" "data" "inline" "noinline" "tailrec" "external"
    "annotation" "crossinline" "const" "operator" "infix" "suspend" "actual" "expect" "setparam"))

[
  "package" "import" "as" "as?" "typealias" "class" "interface" "object" "fun" "val" "var"
  "this" "this@" "super" "super@" "for" "is" "!is" "in" "!in" "throw" "return" "return@" "if"
  "else" "while" "do" "try" "catch" "finally" "when" "where" "by" "get" "set" "abstract" "enum"
  "open" "inner" "override" "private" "public" "internal" "protected" "out" "final" "vararg"
  "dynamic" "companion" "constructor" "init" "sealed" "field" "property" "receiver" "param"
  "setparam" "file" "lateinit" "data" "inline" "noinline" "tailrec" "external" "annotation"
  "crossinline" "const" "operator" "infix" "suspend" "actual" "expect"
] @keyword

(reification_modifier) @keyword

[
  "!" "!!" "!=" "!==" "%" "%=" "&" "&&" "*" "*=" "+" "++" "+=" "-" "--" "-=" "->" "/" "/=" "<"
  "<=" "=" "==" "===" ">" ">=" "?" "?:" "||"
] @operator

(string_literal) @string
(multiline_string_literal) @string
(character_literal) @string
(number_literal) @number
(float_literal) @number
(line_comment) @comment
(block_comment) @comment

; The code of a template is coloured as code; its delimiters take the annotation colour. The
; grammar reads a `$name` template of a one-line string as two pieces of string content.
(interpolation) @embedded
(interpolation ["$" "${" "}"] @attribute)
((string_content) @attribute . (string_content) @_name
  (#eq? @attribute "$")
  (#match? @_name "^[A-Za-z_]"))

; An annotation's `@` and its use-site target (`@file`, `@get`) are one annotation token.
(annotation "@" @attribute)
(file_annotation ["@" "file"] @attribute)
(use_site_target ["field" "property" "get" "set" "receiver" "param" "setparam" "delegate"] @attribute)
