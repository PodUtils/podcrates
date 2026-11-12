# pod-actions-expr

The expression language of GitHub Actions (`${{ }}`), with no I/O: a
lexer and a parser of the runner's grammar, and an evaluator with the
rules of GitHub. podrunner asked for it for its local mode
(PodUtils/podrunner@T-046): the expressions of the job level (the `if` of a
job, `strategy.matrix` with `fromJSON`, `runs-on`, `needs.<id>.outputs` and
`.result`). The official runner evaluates the steps. `pod-actions-workflow`
uses it. The crate has no dependency, reads no environment and does no
I/O. T-251 made it, as new code from the documents of GitHub ("Evaluate
expressions in workflows and actions", "Contexts reference") and from the
behaviour of the runner (`actions/runner`, `src/Sdk/DTExpressions2`).

## Use

```rust
use pod_actions_expr::{Condition, Contexts, Expr, Status, Template, Value};

let mut contexts = Contexts::new();
contexts.insert("github", github_stand_in);          // a Value::Object
contexts.insert("needs", Value::from_json(needs_json)?);
contexts.status = Status { success: false, failure: true, cancelled: false };

let condition = Condition::parse("needs.build.result == 'failure'")?;
condition.text();                // "success() && (needs.build.result == 'failure')"
condition.evaluate(&contexts)?;  // false: the implicit success() is false

let matrix = Template::parse("${{ fromJSON(needs.setup.outputs.matrix) }}")?;
let value = matrix.evaluate(&contexts)?; // an object: one expression keeps its kind
let name = Template::parse("build (${{ matrix.os }})")?; // text around: a string
let expr = Expr::parse("contains(github.event.issue.labels.*.name, 'bug')")?;
expr.contexts();                 // ["github"]
```

## API

| Item | What |
| --- | --- |
| `Value`, `Object` | null, boolean, number (a double), string, array, object; an object keeps the order of its keys and reads a key with no regard to case. `is_truthy`, `to_number`, `to_text`, `loose_eq`, `loose_lt`, `loose_gt`, `to_json` (the form of `toJSON`), `to_json_line`, `from_json` (the reader of `fromJSON`) |
| `Expr` | `parse`, `evaluate`, `contexts` (each named value), `functions`, `has_status_function`, `check_contexts`, `as_string` |
| `Template`, `TemplatePart`, `has_expression`, `OPEN` | a text with `${{ }}` parts: `parse`, `evaluate`, `as_expression`, `expressions`, `parts` (each literal text and each expression with its index, in order; T-322), `is_literal`, `check_contexts` |
| `Condition` | an `if:`: `parse`, `evaluate`, `text` (with the implicit `success() &&`), `is_implicit`, `contexts` |
| `Contexts` | the named values of the caller (`insert`, `get`, `remove`, `names`), with `status` and `limits` |
| `Status` | the answers of `success()`, `failure()` and `cancelled()`; `always()` is true |
| `Limits` | `max_bytes`: the bytes of the text of `format`, `join` and `toJSON`, the input of `fromJSON` and the filtered arrays of one evaluation (1 MiB) |
| `Error`, `ErrorKind` | each failure by name, with the index of its character |
| `evaluate`, `is_name` | one expression from text; the rule of a name (a context, a property, a job id, a step id) |
| `MAX_LENGTH`, `MAX_DEPTH`, `MAX_JSON_DEPTH` | 21,000 characters and 50 levels of an expression (the runner's limits); 64 levels of a JSON document |

The caller gives each context, and the answers of the status functions.
At the level of a job, GitHub gives `github`, `needs`, `vars` and
`inputs` to `if` and `strategy`, and also `strategy` and `matrix` to
`runs-on` and `name`; `pod-actions-workflow` gives them so.

## Conformance

Each line is a rule of GitHub and the tests that hold it. The cited
documents are "Evaluate expressions in workflows and actions"; the runner
is `actions/runner`.

| Rule | Tests |
| --- | --- |
| Literals: `null`, `true`, `false` (case counts), numbers with a sign, a point, an exponent, `0x` and `0o`, `NaN`, `Infinity`, strings in `'` with `''` | `operators::the_literals_of_the_documents`, `the_case_of_a_literal_word_counts` |
| The coercion table: null 0, booleans 1 and 0, a string by JSON (empty 0, else NaN), a collection NaN; a number and a string compare as numbers; a boolean or null becomes a number | `operators::the_coercion_table_of_the_documents` |
| NaN equals nothing | `operators::nan_equals_nothing` |
| Strings compare with no regard to case (.NET `OrdinalIgnoreCase`: the upper case) | `operators::strings_compare_with_no_regard_to_case` |
| An array or an object equals only itself | `operators::collections_are_equal_only_to_themselves` |
| `<`, `<=`, `>`, `>=` of numbers, strings and booleans; false for other kinds | `operators::the_order_of_numbers_strings_and_booleans` |
| `&&` and `||` give an operand and stop at the answer | `operators::and_and_or_give_an_operand`, `and_and_or_stop_at_their_answer` |
| `!` and the truth of each kind | `operators::not_gives_the_truth` |
| The precedence | `operators::the_precedence_of_the_operators` |
| `.name`, `['name']`, `[n]` (rounded down; null past the end) | `operators::the_index_and_the_property` |
| The object filter `*` on an array and an object, and an index of a filtered array | `functions::the_object_filter_of_the_documents` |
| `contains`, `startsWith`, `endsWith`, `format`, `join`, `toJSON`, `fromJSON` with the examples of the documents | `functions::*` |
| `success`, `failure`, `always`, `cancelled` from the caller's status | `functions::the_status_functions_read_the_status_of_the_caller` |
| Function names with no regard to case; an unknown function and a bad count of arguments at the parse | `functions::function_names_ignore_case`, `errors::functions_are_checked_at_the_parse` |
| `hashFiles` parses and fails by name | `functions::hash_files_parses_and_fails_by_name` |
| A named value that the caller does not give is an error, also in a branch that is not evaluated | `errors::a_missing_context_is_an_error_by_name_in_each_branch` |
| 21,000 characters, 50 levels | `errors::the_length_limit_of_github`, `the_depth_limit_of_github` |
| A template: one expression keeps its kind; text around gives a string; the end of an expression is the first `}}` outside a string | `templates::*` |
| An `if:` with no status function is `success() && (if)`; `${{ }}` with text around is a string (true when not empty) | `templates::a_condition_*` |
| The number text of GitHub (.NET `G15`) | `number::tests` |

Not measured: the conformance against the official runner (no run of
`actions/runner` here). Known differences:

* `fromJSON` reads strict JSON (RFC 8259). The runner's Newtonsoft reader
  also takes comments, single quotes and other forms.
* Case: a character whose upper case is two characters (`ß`) keeps its
  own; .NET compares UTF-16 code units, this crate code points.
* The memory limit counts each made text of an evaluation in sum; the
  runner counts the results that it holds at one time.
* `toJSON` escapes `"`, `\` and the control characters; the runner's
  writer can escape more characters.

## Tests

`cargo test -p pod-actions-expr`. The fuzz target `actions_expr`
(`fuzz/`) runs the parser, the evaluator, the templates, the conditions
and the JSON reader on untrusted text.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podrunner | none (local mode, PodUtils/podrunner@T-046, waits) | `pod-actions-expr = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }`, through `pod-actions-workflow` |
