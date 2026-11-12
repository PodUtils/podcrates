//! The cells of a matrix, with the rules of the documents ("Running
//! variations of jobs in a workflow"): the product of the vectors (the first
//! key changes last); then each `exclude` entry takes out each cell that
//! has each of its values (a partial match); then each `include` entry adds
//! its values to each cell of the product whose original values it does not
//! change, or else it becomes a cell of its own. A value that an include
//! entry added can change; an original value cannot.

use pod_actions_expr::{Object, Value};

/// The most cells of one matrix, as in GitHub.
pub const MAX_CELLS: usize = 256;
/// The most cells of the product before `exclude`: a larger product is not
/// computed.
pub const MAX_PRODUCT: usize = 65_536;

/// The cells of an evaluated matrix: each an object of its keys and values.
pub fn cells(matrix: &Value) -> Result<Vec<Object>, String> {
    let Some(matrix) = matrix.as_object() else {
        return Err(format!("the matrix must be a mapping, not {}", matrix.kind()));
    };
    let mut vectors: Vec<(&str, &[Value])> = Vec::new();
    let mut include: &[Value] = &[];
    let mut exclude: &[Value] = &[];
    for (key, value) in matrix.iter() {
        let Some(items) = value.as_array() else {
            return Err(format!("`{key}` must be a sequence, not {}", value.kind()));
        };
        match key {
            "include" => include = entries(key, items)?,
            "exclude" => exclude = entries(key, items)?,
            _ if items.is_empty() => return Err(format!("the vector `{key}` has no value")),
            _ => vectors.push((key, items)),
        }
    }
    let mut product: Vec<Object> = Vec::new();
    if !vectors.is_empty() {
        let size = vectors.iter().try_fold(1usize, |n, (_, items)| n.checked_mul(items.len()));
        if size.is_none_or(|n| n > MAX_PRODUCT) {
            return Err(format!("the product of the vectors passes {MAX_PRODUCT} cells"));
        }
        product.push(Object::new());
        for (key, items) in &vectors {
            let mut next = Vec::with_capacity(product.len() * items.len());
            for cell in &product {
                for item in *items {
                    let mut cell = cell.clone();
                    cell.insert(*key, item.clone());
                    next.push(cell);
                }
            }
            product = next;
        }
    }
    product.retain(|cell| !exclude.iter().filter_map(Value::as_object).any(|ex| matches(cell, ex, None)));
    let originals = product.len();
    let mut extra: Vec<Object> = Vec::new();
    for entry in include.iter().filter_map(Value::as_object) {
        let mut matched = false;
        for cell in product.iter_mut().take(originals) {
            if matches(cell, entry, Some(&vectors)) {
                matched = true;
                for (k, v) in entry.iter() {
                    cell.insert(k, v.clone());
                }
            }
        }
        if !matched {
            extra.push(entry.clone());
        }
    }
    product.extend(extra);
    if product.is_empty() {
        return Err("the matrix has no cell".into());
    }
    if product.len() > MAX_CELLS {
        return Err(format!("the matrix has {} cells; the limit is {MAX_CELLS}", product.len()));
    }
    Ok(product)
}

fn entries<'a>(key: &str, items: &'a [Value]) -> Result<&'a [Value], String> {
    match items.iter().find(|i| i.as_object().is_none()) {
        Some(bad) => Err(format!("each entry of `{key}` must be a mapping, not {}", bad.kind())),
        None => Ok(items),
    }
}

/// Whether `cell` has each value of `entry`. With `vectors`, only the keys
/// of the vectors count (an include entry); else each key of the entry
/// must be in the cell (an exclude entry).
fn matches(cell: &Object, entry: &Object, vectors: Option<&[(&str, &[Value])]>) -> bool {
    entry.iter().all(|(k, v)| match vectors {
        Some(vectors) if !vectors.iter().any(|(name, _)| *name == k) => true,
        _ => cell.get(k).is_some_and(|c| same(c, v)),
    })
}

/// Deep equality of two values: strings with case, numbers by value, the
/// keys of an object in any order.
pub(crate) fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| same(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        (Value::Number(x), Value::Number(y)) => x == y,
        _ => a == b,
    }
}
