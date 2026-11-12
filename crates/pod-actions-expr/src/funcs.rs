//! The functions of a workflow, as the runner has them.

use crate::error::{Error, ErrorKind};
use crate::eval::{Eval, Out};
use crate::json;
use crate::parse::{Function, Node};
use crate::value::{Value, folded};

pub(crate) fn call(eval: &mut Eval<'_>, function: Function, args: &[Node], at: usize) -> Result<Out, Error> {
    let value = match function {
        Function::Contains => Value::Bool(contains(eval, args)?),
        Function::StartsWith => Value::Bool(affix(eval, args, |s, p| s.starts_with(p))?),
        Function::EndsWith => Value::Bool(affix(eval, args, |s, p| s.ends_with(p))?),
        Function::Format => Value::String(format(eval, args).map_err(|e| e.or_at(at))?),
        Function::Join => Value::String(join(eval, args)?),
        Function::ToJson => {
            let value = eval.eval(&args[0])?.value;
            let text = json::write(&value, true);
            eval.spend(text.len())?;
            Value::String(text)
        }
        Function::FromJson => {
            let text = eval.eval(&args[0])?.value.to_text();
            eval.spend(text.len() * 2)?;
            json::read(&text).map_err(|why| Error::at(ErrorKind::Json(why), at))?
        }
        Function::HashFiles => return Err(Error::at(ErrorKind::NeedsIo("hashFiles".into()), at)),
        Function::Success => Value::Bool(eval.contexts.status.success),
        Function::Failure => Value::Bool(eval.contexts.status.failure),
        Function::Cancelled => Value::Bool(eval.contexts.status.cancelled),
        Function::Always => Value::Bool(true),
    };
    Ok(Out::plain(value))
}

impl Error {
    /// The failure at `at` where it has no place yet.
    fn or_at(mut self, at: usize) -> Error {
        self.at = self.at.or(Some(at));
        self
    }
}

/// `contains(search, item)`: a primitive search is a text that holds the
/// text of `item`, with no regard to case; an array search holds an item
/// that is `==` to `item`. Each other case is false.
fn contains(eval: &mut Eval<'_>, args: &[Node]) -> Result<bool, Error> {
    let search = eval.eval(&args[0])?.value;
    if search.is_primitive() {
        let item = eval.eval(&args[1])?.value;
        if !item.is_primitive() {
            return Ok(false);
        }
        return Ok(folded(&search.to_text()).contains(&folded(&item.to_text())));
    }
    if let Value::Array(items) = &search
        && !items.is_empty()
    {
        let item = eval.eval(&args[1])?.value;
        return Ok(items.iter().any(|i| item.loose_eq(i)));
    }
    Ok(false)
}

/// `startsWith` and `endsWith`: two primitives as text, with no regard to
/// case; false for a collection.
fn affix(eval: &mut Eval<'_>, args: &[Node], test: fn(&str, &str) -> bool) -> Result<bool, Error> {
    let left = eval.eval(&args[0])?.value;
    if !left.is_primitive() {
        return Ok(false);
    }
    let right = eval.eval(&args[1])?.value;
    if !right.is_primitive() {
        return Ok(false);
    }
    Ok(test(&folded(&left.to_text()), &folded(&right.to_text())))
}

/// `join(array, separator)`: the text of each item with the separator
/// (`,` by default, or the text of a primitive separator) between them. A
/// primitive gives its text; an object or an empty array the empty text.
fn join(eval: &mut Eval<'_>, args: &[Node]) -> Result<String, Error> {
    let items = eval.eval(&args[0])?.value;
    match &items {
        Value::Array(list) if !list.is_empty() => {
            let mut separator = ",".to_string();
            if list.len() > 1
                && let Some(arg) = args.get(1)
            {
                let value = eval.eval(arg)?.value;
                if value.is_primitive() {
                    separator = value.to_text();
                }
            }
            let mut out = String::new();
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    eval.spend(separator.len())?;
                    out.push_str(&separator);
                }
                let text = item.to_text();
                eval.spend(text.len())?;
                out.push_str(&text);
            }
            Ok(out)
        }
        v if v.is_primitive() => Ok(v.to_text()),
        _ => Ok(String::new()),
    }
}

/// `format(text, args...)`: `{N}` takes the text of argument N (0 to 255);
/// `{{` and `}}` are braces. A format specifier (`{0:x}`), a brace alone or
/// an index past the arguments is an error. An argument is evaluated
/// only when the text names it.
fn format(eval: &mut Eval<'_>, args: &[Node]) -> Result<String, Error> {
    let text: Vec<char> = eval.eval(&args[0])?.value.to_text().chars().collect();
    eval.spend(text.len())?;
    let mut values: Vec<Option<String>> = vec![None; args.len() - 1];
    let bad = |why: &str| Error::new(ErrorKind::Format(format!("{why} in '{}'", text.iter().collect::<String>())));
    let mut out = String::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == '}' {
            if text.get(i + 1) != Some(&'}') {
                return Err(bad("a `}` with no `{`"));
            }
            out.push('}');
            i += 2;
        } else if c != '{' {
            out.push(c);
            i += 1;
        } else if text.get(i + 1) == Some(&'{') {
            out.push('{');
            i += 2;
        } else {
            let start = i + 1;
            let mut j = start;
            while j < text.len() && text[j].is_ascii_digit() {
                j += 1;
            }
            let digits: String = text[start..j].iter().collect();
            if digits.is_empty() || text.get(j) != Some(&'}') {
                let why = if text.get(j) == Some(&':') { "a format specifier" } else { "a `{` with no index" };
                return Err(bad(why));
            }
            let index: usize = match digits.parse::<u8>() {
                Ok(n) => usize::from(n),
                Err(_) => return Err(bad("an index past 255")),
            };
            if index >= values.len() {
                return Err(bad(&format!("the index {index} with {} arguments", values.len())));
            }
            if values[index].is_none() {
                values[index] = Some(eval.eval(&args[index + 1])?.value.to_text());
            }
            // Each copy of an argument takes from the limit, so a format
            // of a format cannot grow past it.
            let arg = values[index].as_deref().unwrap_or_default();
            eval.spend(arg.len())?;
            out.push_str(arg);
            i = j + 1;
        }
    }
    Ok(out)
}
