//! The programs of the clients of a job that the tests write (T-980): Go,
//! Java and Deno. Each reads its URL from `JOB_URL` and the placeholder
//! from `JOB_TOKEN`, and prints the status, the header `X-Seen` and the
//! body.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `net/http` of Go with the proxy of the environment, as `go mod
/// download` and each Go program use it; `crypto/x509` reads the roots.
pub const GO: &str = r#"package main

import (
	"fmt"
	"io"
	"net/http"
	"os"
)

func main() {
	req, err := http.NewRequest("GET", os.Getenv("JOB_URL"), nil)
	if err != nil {
		panic(err)
	}
	req.Header.Set("Authorization", "Bearer "+os.Getenv("JOB_TOKEN"))
	r, err := http.DefaultClient.Do(req)
	if err != nil {
		fmt.Fprintln(os.Stderr, "go:", err)
		os.Exit(2)
	}
	body, _ := io.ReadAll(r.Body)
	fmt.Println(r.StatusCode, "X-Seen: "+r.Header.Get("X-Seen"))
	fmt.Println(string(body))
}
"#;

/// The default `SSLSocketFactory` of the JVM over a CONNECT of its own: it
/// trusts the file of `javax.net.ssl.trustStore` (a JVM reads no proxy of
/// the environment, and turns Basic off for a tunnel by default).
pub const JAVA: &str = r#"
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import javax.net.ssl.*;

public class Client {
    static String head(InputStream in) throws IOException {
        StringBuilder out = new StringBuilder();
        while (!out.toString().endsWith("\r\n\r\n")) {
            int b = in.read();
            if (b < 0) throw new EOFException("the head ends early");
            out.append((char) b);
        }
        return out.toString();
    }

    public static void main(String[] args) throws Exception {
        URI proxy = URI.create(System.getenv("HTTPS_PROXY")), url = URI.create(System.getenv("JOB_URL"));
        String host = url.getHost();
        Socket socket = new Socket(proxy.getHost(), proxy.getPort());
        String auth = Base64.getEncoder().encodeToString(proxy.getUserInfo().getBytes(StandardCharsets.UTF_8));
        OutputStream out = socket.getOutputStream();
        out.write(("CONNECT " + host + ":443 HTTP/1.1\r\nHost: " + host + ":443\r\nProxy-Authorization: Basic "
            + auth + "\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
        out.flush();
        String connect = head(socket.getInputStream());
        if (!connect.startsWith("HTTP/1.1 200")) { System.err.println("CONNECT: " + connect); System.exit(3); }
        SSLSocket tls = (SSLSocket) ((SSLSocketFactory) SSLSocketFactory.getDefault()).createSocket(socket, host, 443, true);
        SSLParameters params = tls.getSSLParameters();
        params.setEndpointIdentificationAlgorithm("HTTPS");
        params.setApplicationProtocols(new String[] {"http/1.1"});
        tls.setSSLParameters(params);
        try {
            tls.startHandshake();
        } catch (SSLException e) {
            System.err.println("java: " + e);
            System.exit(4);
        }
        tls.getOutputStream().write(("GET " + url.getRawPath() + " HTTP/1.1\r\nHost: " + host
            + "\r\nAuthorization: Bearer " + System.getenv("JOB_TOKEN") + "\r\nConnection: close\r\n\r\n")
            .getBytes(StandardCharsets.US_ASCII));
        tls.getOutputStream().flush();
        // The far end of a tunnel that passes as it is keeps the connection:
        // read the body by its length.
        InputStream in = tls.getInputStream();
        String answer = head(in);
        int length = 0;
        for (String line : answer.split("\r\n")) {
            if (line.toLowerCase().startsWith("content-length:")) length = Integer.parseInt(line.substring(15).trim());
        }
        System.out.print(answer);
        System.out.println(new String(in.readNBytes(length), StandardCharsets.UTF_8));
    }
}
"#;

/// The `fetch` of Deno with the proxy of the environment.
pub const DENO: &str = r#"
const r = await fetch(Deno.env.get("JOB_URL"), { headers: { Authorization: "Bearer " + Deno.env.get("JOB_TOKEN") } })
  .catch((e) => { console.error("deno:", e.message); Deno.exit(2); });
console.log(r.status, "X-Seen: " + r.headers.get("x-seen"));
console.log(await r.text());
"#;

/// Write `source` to `dir/name`.
pub fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).unwrap();
    path
}

/// Build [`GO`] with `go` to `dir/go-client`, with the environment of the
/// test (its build cache) and no download of a toolchain.
pub fn build_go(go: &str, dir: &Path) -> Result<PathBuf, String> {
    let source = write(dir, "client.go", GO);
    let out = dir.join("go-client");
    let built = Command::new(go)
        .args(["build", "-o"])
        .arg(&out)
        .arg(&source)
        .current_dir(dir)
        .env("GOTOOLCHAIN", "local")
        .env("GOFLAGS", "")
        .env("CGO_ENABLED", "0")
        .output()
        .map_err(|e| format!("{go}: {e}"))?;
    if !built.status.success() {
        return Err(format!("{go} build: {}", String::from_utf8_lossy(&built.stderr)));
    }
    Ok(out)
}
