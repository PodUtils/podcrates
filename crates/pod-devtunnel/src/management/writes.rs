//! The writes of the management API (T-378, part 2; GitHub #94): the list
//! of each tunnel of the user, a new tunnel, its delete, and a port of a
//! tunnel. Each takes a user token: a tunnel access token and no token
//! read one tunnel alone. A failure has the words of a read: the method,
//! the URL with no query, the form of the credential, the status, and of a
//! refusal only the title and the detail of its problem document.

use pod_core::entropy::Entropy;
use pod_core::{Class, Failure};
use pod_proxy::client::Request;
use serde::Deserialize;

use super::{Locator, Management, class_of, label, problem};
use crate::failure::{Step, fail, wrap};
use crate::info::ANSWER_MOST;
use crate::service;
use crate::token::Credential;

/// A tunnel of the user, as the API gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owned {
    #[serde(default)]
    pub tunnel_id: String,
    #[serde(default)]
    pub cluster_id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub ports: Vec<OwnedPort>,
}

/// A port of a tunnel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedPort {
    pub port_number: u16,
}

impl Owned {
    /// The tunnel as a [`Locator`] by its id and cluster.
    pub fn locator(&self) -> Result<Locator, Failure> {
        Locator::parse(&format!("{}.{}", self.tunnel_id, self.cluster_id))
    }
}

/// The list of the API: tunnels by region (`value[].value[]`).
#[derive(Deserialize)]
struct ByRegion {
    #[serde(default)]
    value: Vec<Region>,
}

#[derive(Deserialize)]
struct Region {
    #[serde(default)]
    value: Vec<Owned>,
}

/// A new id of a tunnel, of the vendor's form: an adjective, a noun, then
/// the characters of the tail (the facts `tunnel-id-*`).
pub fn new_id(entropy: &mut dyn Entropy) -> Result<String, Failure> {
    let texts = |name: &str| -> Vec<String> {
        service::service().texts(name).unwrap_or_default().iter().map(|t| t.as_ref().to_string()).collect()
    };
    let (adjectives, nouns) = (texts("tunnel-id-adjectives"), texts("tunnel-id-nouns"));
    let chars: Vec<char> = service::service().text("tunnel-id-chars").unwrap_or_default().chars().collect();
    let tail: usize = service::service().text("tunnel-id-tail").and_then(|t| t.parse().ok()).unwrap_or(0);
    if adjectives.is_empty() || nouns.is_empty() || chars.is_empty() || tail == 0 {
        return Err(fail(Step::Management, Class::Config, "pod-services holds no form of a tunnel id"));
    }
    let mut bytes = vec![0u8; tail + 2];
    entropy
        .fill(&mut bytes)
        .map_err(|_| fail(Step::Management, Class::Unavailable, "no random bytes for the id of a new tunnel"))?;
    // A modulo of a byte: the lists are short, and an id is no secret.
    let pick = |b: u8, n: usize| usize::from(b) % n;
    let mut id = format!("{}-{}-", adjectives[pick(bytes[0], adjectives.len())], nouns[pick(bytes[1], nouns.len())]);
    id.extend(bytes[2..].iter().map(|b| chars[pick(*b, chars.len())]));
    Ok(id)
}

impl Management {
    /// A request of the API with `credential`; `url` holds its query.
    fn write_request(&self, method: &'static str, url: String, credential: &Credential) -> Request {
        let request = Request::new(method, url);
        let request = match credential.header() {
            Some(value) => request.header("Authorization", value.as_str()),
            None => request,
        };
        request.header("Accept", "application/json").timeout(self.timeout).max_body(ANSWER_MOST)
    }

    /// The URL of `path` on `host`, with the version, then `query`.
    fn write_url(&self, host: &str, path: &str, query: &str) -> String {
        let more = if query.is_empty() { String::new() } else { format!("&{query}") };
        format!("{}{path}?api-version={}{more}", self.base.url_of(host), self.api_version)
    }

    /// The host of `locator`: the global host for a name, the host of its
    /// cluster for an id.
    fn host_of(&self, locator: &Locator) -> String {
        match locator {
            Locator::Name(_) => self.base.host.clone(),
            Locator::Id { cluster, .. } => self.base.cluster_host(cluster),
        }
    }

    /// Send `request`, shown as `shown`; the body of a success.
    async fn write(&self, request: Request, shown: &str) -> Result<Vec<u8>, Failure> {
        let line = request.line();
        let answer = self.client.send(request).await.map_err(|e| wrap(Step::Management, &line, e))?;
        if !answer.is_success() {
            let words = problem(&answer.body);
            let what = match words.is_empty() {
                true => format!("{shown} answered HTTP {}", answer.status),
                false => format!("{shown} answered HTTP {}: {words}", answer.status),
            };
            return Err(fail(Step::Management, class_of(answer.status), what));
        }
        Ok(answer.body)
    }

    fn shown(&self, method: &str, host: &str, path: &str, credential: &Credential) -> String {
        format!("{method} {}{path} with {}", self.base.url_of(host), credential.form())
    }

    /// Each tunnel of the user of `credential`, of each cluster.
    pub async fn list(&self, credential: &Credential) -> Result<Vec<Owned>, Failure> {
        let (host, path) = (self.base.host.clone(), service::tunnels_path());
        let url = self.write_url(&host, path, service::service().text("list-query").unwrap_or_default());
        let shown = self.shown("GET", &host, path, credential);
        let body = self.write(self.write_request("GET", url, credential), &shown).await?;
        let list: ByRegion = serde_json::from_slice(&body)
            .map_err(|e| fail(Step::Management, Class::Protocol, format!("{shown}: the list is not JSON: {e}")))?;
        Ok(list.value.into_iter().flat_map(|r| r.value).collect())
    }

    /// A new tunnel of the user of `credential`, with the id `id`
    /// ([`new_id`]), the name `name` where it has one, in the cluster
    /// `cluster`, or on the global host where none is named, as the
    /// vendor's client does with no recommendation.
    pub async fn create(
        &self,
        credential: &Credential,
        id: &str,
        name: Option<&str>,
        cluster: Option<&str>,
    ) -> Result<Owned, Failure> {
        let bad = |what: &str| fail(Step::Management, Class::Usage, format!("{what} is no label of a host name"));
        for part in [Some(id), name, cluster].into_iter().flatten() {
            if !label(part) {
                return Err(bad(&format!("{part:?}")));
            }
        }
        let host = cluster.map_or_else(|| self.base.host.clone(), |c| self.base.cluster_host(c));
        let path = format!("{}/{id}", service::tunnels_path());
        let mut doc = serde_json::json!({ "tunnelId": id });
        if let Some(name) = name {
            doc["name"] = name.into();
        }
        if let Some(cluster) = cluster {
            doc["clusterId"] = cluster.into();
        }
        let request = self
            .write_request("PUT", self.write_url(&host, &path, ""), credential)
            .header("Content-Type", "application/json")
            .body(doc.to_string());
        let shown = self.shown("PUT", &host, &path, credential);
        let body = self.write(request, &shown).await?;
        serde_json::from_slice(&body)
            .map_err(|e| fail(Step::Management, Class::Protocol, format!("{shown}: the tunnel is not JSON: {e}")))
    }

    /// Delete the tunnel `locator`.
    pub async fn delete(&self, credential: &Credential, locator: &Locator) -> Result<(), Failure> {
        let host = self.host_of(locator);
        let path = format!("{}/{}", service::tunnels_path(), locator.ident());
        let shown = self.shown("DELETE", &host, &path, credential);
        self.write(self.write_request("DELETE", self.write_url(&host, &path, ""), credential), &shown).await.map(drop)
    }

    /// Add the port `port` to the tunnel `locator`.
    pub async fn add_port(&self, credential: &Credential, locator: &Locator, port: u16) -> Result<OwnedPort, Failure> {
        if port == 0 {
            return Err(fail(Step::Management, Class::Usage, "the port 0 of a tunnel"));
        }
        let host = self.host_of(locator);
        let ports = service::service().text("ports-path").unwrap_or_default();
        let path = format!("{}/{}/{ports}/{port}", service::tunnels_path(), locator.ident());
        let request = self
            .write_request("PUT", self.write_url(&host, &path, ""), credential)
            .header("Content-Type", "application/json")
            .body(serde_json::json!({ "portNumber": port }).to_string());
        let shown = self.shown("PUT", &host, &path, credential);
        let body = self.write(request, &shown).await?;
        serde_json::from_slice(&body)
            .map_err(|e| fail(Step::Management, Class::Protocol, format!("{shown}: the port is not JSON: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed source of bytes.
    struct Fixed(u8);

    impl Entropy for Fixed {
        fn fill(&mut self, buf: &mut [u8]) -> Result<(), pod_core::entropy::NoRandom> {
            buf.fill(self.0);
            Ok(())
        }
    }

    /// An id of the vendor's form: an adjective, a noun, then 7 of the
    /// characters, each a label of a host name.
    #[test]
    fn a_new_id_has_the_vendor_s_form() {
        assert_eq!(new_id(&mut Fixed(0)).unwrap(), "fun-pond-bbbbbbb");
        let id = new_id(&mut pod_core::entropy::OsEntropy).unwrap();
        let parts: Vec<&str> = id.splitn(3, '-').collect();
        assert_eq!(parts.len(), 3, "{id}");
        assert_eq!(parts[2].len(), 7, "{id}");
        assert!(parts[2].chars().all(|c| "bcdfghjklmnpqrstvwxz0123456789".contains(c)), "{id}");
        assert!(label(&id), "{id}");
    }

    /// The list of the API, by region, gives each tunnel of each region.
    #[test]
    fn the_list_by_region_gives_each_tunnel() {
        let body = br#"{"value":[{"regionName":"us","value":[{"tunnelId":"a1","clusterId":"usw2","name":"box",
            "ports":[{"portNumber":22}]}]},{"value":[{"tunnelId":"b2","clusterId":"euw"}]}]}"#;
        let list: ByRegion = serde_json::from_slice(body).unwrap();
        let tunnels: Vec<Owned> = list.value.into_iter().flat_map(|r| r.value).collect();
        assert_eq!(tunnels.len(), 2);
        assert_eq!(tunnels[0].name.as_deref(), Some("box"));
        assert_eq!(tunnels[0].ports, vec![OwnedPort { port_number: 22 }]);
        assert_eq!(tunnels[1].locator().unwrap().to_string(), "b2.euw");
    }
}
