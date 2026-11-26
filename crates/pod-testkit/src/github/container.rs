//! The `container:` and `services:` of a job, from the control API to the
//! job message. The runner reads them as template tokens
//! (`AgentJobRequestMessage.JobContainer` and `JobServiceContainers`) and
//! converts them with `PipelineTemplateConverter.ConvertToJobContainer`:
//! a string is the image; a mapping takes only the keys of [`KEYS`]. A
//! string token is a JSON string, a sequence is `{"type": 1, "seq": [...]}`
//! and a mapping is `{"type": 2, "map": [{"Key": K, "Value": V}...]}`
//! (`TemplateTokenJsonConverter`).

use serde_json::{Map, Value, json};

/// The keys of a container that the runner converts. It refuses another
/// key, and `entrypoint` and `command` of a service need a setting of the
/// runner, so the fake refuses them too.
const KEYS: [&str; 6] = ["image", "options", "env", "ports", "volumes", "credentials"];

/// The token of `container:`; none for `null` or an absent field.
pub(super) fn container(v: &Value) -> Result<Option<Value>, String> {
    match v {
        Value::Null => Ok(None),
        v => one(v, "container").map(Some),
    }
}

/// The token of `services:`, a mapping of each network alias to a
/// container; none for `null` or an absent field.
pub(super) fn services(v: &Value) -> Result<Option<Value>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Object(all) => {
            let mut pairs = Vec::with_capacity(all.len());
            for (alias, c) in all {
                if alias.is_empty() {
                    return Err("a service has an empty name".into());
                }
                pairs.push((alias.clone(), one(c, &format!("service {alias}"))?));
            }
            Ok(Some(mapping(pairs)))
        }
        _ => Err("services is not an object of name to container".into()),
    }
}

/// The token of one container: a string, or an object of [`KEYS`].
fn one(v: &Value, what: &str) -> Result<Value, String> {
    let fields = match v {
        Value::String(image) if !image.is_empty() => return Ok(json!(image)),
        Value::Object(fields) => fields,
        _ => return Err(format!("{what} is not an image or an object")),
    };
    if !fields.get("image").is_some_and(|i| i.as_str().is_some_and(|i| !i.is_empty())) {
        return Err(format!("{what} has no image"));
    }
    let mut pairs = Vec::with_capacity(fields.len());
    for (key, value) in fields {
        let token = match key.as_str() {
            "image" | "options" => scalar(value),
            "env" => strings(value).map(mapping),
            "ports" | "volumes" => items(value).map(|seq| json!({ "type": 1, "seq": seq })),
            "credentials" => credentials(value),
            _ => return Err(format!("{what}: the key {key} is not one of {}", KEYS.join(", "))),
        };
        let token = token.ok_or_else(|| format!("{what}: {key} does not have the shape of a workflow"))?;
        pairs.push((key.clone(), token));
    }
    Ok(mapping(pairs))
}

/// A string token of a scalar: a workflow gives `5432` and `true` as text.
fn scalar(v: &Value) -> Option<Value> {
    match v {
        Value::String(s) => Some(json!(s)),
        Value::Number(n) => Some(json!(n.to_string())),
        Value::Bool(b) => Some(json!(b.to_string())),
        _ => None,
    }
}

/// The pairs of an object of scalars, each value as a string token.
fn strings(v: &Value) -> Option<Vec<(String, Value)>> {
    let fields: &Map<String, Value> = v.as_object()?;
    fields.iter().map(|(k, v)| Some((k.clone(), scalar(v)?))).collect()
}

/// The items of an array of scalars that are not empty, as string tokens.
fn items(v: &Value) -> Option<Vec<Value>> {
    v.as_array()?.iter().map(|i| scalar(i).filter(|s| *s != "")).collect()
}

/// `credentials`: `username` and `password`, each a text that is not empty.
fn credentials(v: &Value) -> Option<Value> {
    let pairs = strings(v)?;
    let known = |(k, t): &(String, Value)| matches!(k.as_str(), "username" | "password") && *t != "";
    pairs.iter().all(known).then(|| mapping(pairs))
}

/// A mapping token of the pairs, in their order.
fn mapping(pairs: Vec<(String, Value)>) -> Value {
    let map: Vec<Value> = pairs.into_iter().map(|(k, v)| json!({ "Key": k, "Value": v })).collect();
    json!({ "type": 2, "map": map })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value of `key` in a mapping token.
    fn get(token: &Value, key: &str) -> Value {
        let map = token["map"].as_array().expect("a mapping token");
        map.iter().find(|p| p["Key"] == key).map_or(Value::Null, |p| p["Value"].clone())
    }

    #[test]
    fn a_container_of_a_workflow_is_a_mapping_token() {
        let c = json!({
            "image": "postgres:16", "options": "--health-cmd pg_isready",
            "env": { "POSTGRES_PASSWORD": "made-up", "PORT": 5432 },
            "ports": ["5432:5432", 6379], "volumes": ["/data:/data"],
            "credentials": { "username": "u", "password": "made-up" },
        });
        let t = container(&c).unwrap().unwrap();
        assert_eq!(t["type"], 2);
        assert_eq!(get(&t, "image"), "postgres:16");
        assert_eq!(get(&t, "options"), "--health-cmd pg_isready");
        assert_eq!(get(&get(&t, "env"), "PORT"), "5432", "a number of a workflow is text");
        assert_eq!(get(&t, "ports"), json!({ "type": 1, "seq": ["5432:5432", "6379"] }));
        assert_eq!(get(&t, "volumes")["seq"], json!(["/data:/data"]));
        assert_eq!(get(&get(&t, "credentials"), "username"), "u");
        assert_eq!(container(&json!("alpine:3")).unwrap(), Some(json!("alpine:3")), "an image alone");
        assert_eq!(container(&Value::Null).unwrap(), None);
    }

    #[test]
    fn services_are_a_mapping_of_each_name_to_a_container() {
        let s = json!({ "db": { "image": "postgres:16" }, "cache": "redis:7" });
        let t = services(&s).unwrap().unwrap();
        assert_eq!(get(&get(&t, "db"), "image"), "postgres:16");
        assert_eq!(get(&t, "cache"), "redis:7");
        assert_eq!(services(&Value::Null).unwrap(), None);
    }

    #[test]
    fn a_container_that_the_runner_refuses_is_refused() {
        for bad in [
            json!(""),
            json!(5),
            json!({ "options": "--rm" }),
            json!({ "image": "" }),
            json!({ "image": "a", "command": "sleep" }),
            json!({ "image": "a", "entrypoint": "sh" }),
            json!({ "image": "a", "env": ["A=1"] }),
            json!({ "image": "a", "ports": "80" }),
            json!({ "image": "a", "ports": [""] }),
            json!({ "image": "a", "volumes": [{}] }),
            json!({ "image": "a", "credentials": { "token": "t" } }),
            json!({ "image": "a", "credentials": { "username": "" } }),
        ] {
            assert!(container(&bad).is_err(), "{bad}");
            assert!(services(&json!({ "s": bad })).is_err(), "{bad}");
        }
        assert!(services(&json!(["db"])).is_err());
        assert!(services(&json!({ "": "redis:7" })).is_err());
    }
}
