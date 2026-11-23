//! A site from a directory: its pages, its name routing, its 404, its
//! refusal of a traversal, and the words that no body may hold.

use std::fs;
use std::path::{Path, PathBuf};

use pod_cover::{Site, SiteError, Sites};

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A temporary directory that removes itself.
struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new(tag: &str) -> Temp {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pod-cover-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a temporary directory");
        Temp { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A tree of one small page: an index, a style sheet, robots.txt and a 404.
fn tree(root: &Path) {
    fs::create_dir_all(root.join("assets")).expect("the asset directory");
    fs::write(
        root.join("index.html"),
        "<!doctype html>\n<html><head><title>Field notes</title></head>\n<body><h1>Field notes</h1><p>Pages from the hill.</p></body></html>\n",
    )
    .expect("the index");
    fs::write(root.join("assets/site.css"), "body { color: #222; }\n").expect("the style sheet");
    fs::write(root.join("robots.txt"), "User-agent: *\nDisallow: /private\n").expect("robots.txt");
    fs::create_dir_all(root.join("about")).expect("the about directory");
    fs::write(root.join("about/index.html"), "<!doctype html><title>About</title>\nAbout the hill.\n")
        .expect("the about page");
    fs::write(
        root.join("404.html"),
        "<!doctype html>\n<html><head><title>Not Found</title></head>\n<body><h1>Not Found</h1><p>No page here.</p></body></html>\n",
    )
    .expect("the 404 page");
}

fn get(target: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nHost: cover.example\r\n\r\n").into_bytes()
}

fn body_text(response: &pod_cover::Response) -> String {
    String::from_utf8_lossy(&response.body).into_owned()
}

#[test]
fn the_index_an_asset_and_robots_txt_are_served() {
    let temp = Temp::new("site-pages");
    tree(temp.path());
    let site = Site::load(temp.path()).expect("a site");
    for (target, kind) in [
        ("/", "text/html; charset=utf-8"),
        ("/assets/site.css", "text/css; charset=utf-8"),
        ("/robots.txt", "text/plain; charset=utf-8"),
    ] {
        let response = site.respond(&get(target));
        assert_eq!(response.status, 200, "{target}");
        assert!(!response.body.is_empty(), "{target}");
        let header = |name: &str| response.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
        assert_eq!(header("Content-Type"), Some(kind), "{target}");
        assert_eq!(header("Server"), Some("nginx"), "{target}");
        assert_eq!(header("Content-Length"), Some(response.body.len().to_string().as_str()), "{target}");
    }
    assert!(body_text(&site.respond(&get("/"))).contains("Field notes"));
    assert!(body_text(&site.respond(&get("/robots.txt"))).contains("User-agent"));
    // A directory serves its own index.
    assert_eq!(site.respond(&get("/about/")).status, 200);
    assert!(body_text(&site.respond(&get("/about/"))).contains("About the hill"));
}

#[test]
fn an_unknown_name_gets_the_default_site_404() {
    let temp = Temp::new("site-names");
    let default = temp.path().join("default");
    let named = temp.path().join("named");
    fs::create_dir_all(&default).expect("the default tree");
    tree(&default);
    fs::create_dir_all(&named).expect("the named tree");
    tree(&named);
    let mut sites = Sites::new(Site::load(&default).expect("the default"));
    sites.insert("cover.example", Site::load(&named).expect("the named"));

    let known = sites.respond(Some("cover.example"), &get("/"));
    assert_eq!(known.status, 200);
    assert!(body_text(&known).contains("Field notes"));

    for name in [Some("unknown.example"), None] {
        let response = sites.respond(name, &get("/"));
        assert_eq!(response.status, 404, "{name:?}");
        assert!(body_text(&response).contains("No page here."), "{name:?}");
        // The default site's own 404, not the named site's index.
        assert!(!body_text(&response).contains("Field notes"), "{name:?}");
    }
    // A case difference of a name still routes.
    assert_eq!(sites.respond(Some("Cover.Example"), &get("/")).status, 200);
}

#[test]
fn a_traversal_is_refused() {
    let temp = Temp::new("site-traversal");
    let root = temp.path().join("site");
    fs::create_dir_all(&root).expect("the site tree");
    tree(&root);
    fs::write(temp.path().join("secret.txt"), "a value outside the site\n").expect("the secret");
    let site = Site::load(&root).expect("a site");
    for target in [
        "/../secret.txt",
        "/assets/../../secret.txt",
        "/%2e%2e/secret.txt",
        "/%2E%2E/secret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/..%2fsecret.txt",
        "/a/..\\secret.txt",
    ] {
        let response = site.respond(&get(target));
        assert_eq!(response.status, 404, "{target}");
        assert!(!body_text(&response).contains("outside the site"), "{target}");
    }
    // A file with no traversal is served, so the refusal is not a blanket.
    assert_eq!(site.respond(&get("/assets/site.css")).status, 200);
}

#[test]
fn no_body_names_the_project_or_the_contract() {
    let temp = Temp::new("site-words");
    tree(temp.path());
    let site = Site::load(temp.path()).expect("a site");
    let mut responses = vec![
        site.respond(&get("/")),
        site.respond(&get("/assets/site.css")),
        site.respond(&get("/robots.txt")),
        site.respond(&get("/not-here")),
        site.respond(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"),
        site.respond(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n"),
        site.respond(b"GET /%2e%2e/x HTTP/1.1\r\nHost: x\r\n\r\n"),
    ];
    // The built-in 404 of a site with no 404.html.
    let bare = Temp::new("site-bare");
    fs::write(bare.path().join("index.html"), "<!doctype html><title>Bare</title>\n").expect("the index");
    let bare_site = Site::load(bare.path()).expect("a bare site");
    responses.push(bare_site.respond(&get("/missing")));
    for response in responses {
        let text = body_text(&response).to_ascii_lowercase();
        let header = response
            .headers
            .iter()
            .map(|(n, v)| format!("{n}: {v}"))
            .collect::<Vec<_>>()
            .join("\r\n")
            .to_ascii_lowercase();
        for word in ["podutils", "podcrates", "podssh", "podnet", "contract", env!("CARGO_PKG_VERSION")] {
            assert!(!text.contains(word), "{word} in {:?}", body_text(&response));
            assert!(!header.contains(word), "{word} in {header}");
        }
    }
}

#[test]
fn a_head_has_the_length_and_no_body() {
    let temp = Temp::new("site-head");
    tree(temp.path());
    let site = Site::load(temp.path()).expect("a site");
    let response = site.respond(b"HEAD / HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(response.status, 200);
    assert!(response.head_only);
    let bytes = response.to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("Content-Length: "));
    assert!(text.ends_with("\r\n\r\n"), "{text:?}");
    assert!(!text.contains("Field notes"));
}

#[test]
fn a_server_value_that_names_the_project_is_refused() {
    let temp = Temp::new("site-server");
    tree(temp.path());
    let mut site = Site::load(temp.path()).expect("a site");
    assert!(site.set_server("nginx").is_ok());
    assert_eq!(site.server(), "nginx");
    for bad in ["podssh", "PodUtils", "podnet relay", "line\r\nbreak", ""] {
        assert!(matches!(site.set_server(bad), Err(SiteError::Server(_))), "{bad:?}");
    }
    assert_eq!(site.server(), "nginx");
}

#[test]
fn a_tree_with_no_index_is_refused_by_name() {
    let temp = Temp::new("site-noindex");
    fs::write(temp.path().join("robots.txt"), "User-agent: *\n").expect("robots.txt");
    let error = Site::load(temp.path()).expect_err("no index");
    assert!(matches!(error, SiteError::MissingIndex(_)), "{error}");
    assert!(error.to_string().contains("no index.html"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_link_outside_the_root_is_refused() {
    let temp = Temp::new("site-link");
    let root = temp.path().join("site");
    fs::create_dir_all(&root).expect("the site tree");
    tree(&root);
    fs::write(temp.path().join("secret.txt"), "a value outside the site\n").expect("the secret");
    std::os::unix::fs::symlink(temp.path().join("secret.txt"), root.join("link.txt")).expect("a link");
    let error = Site::load(&root).expect_err("a link that leaves the root");
    assert!(matches!(error, SiteError::Escape(_)), "{error}");
    assert!(error.to_string().contains("outside the site root"), "{error}");
}
