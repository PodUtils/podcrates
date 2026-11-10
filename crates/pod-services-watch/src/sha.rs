//! `http-sha256`: the SHA-256 of a file at an endpoint, of each file under
//! its path, or of a range of it, and its length.

use pod_services::{Fact, Service, Watch};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::args::{Args, hex, https_url, named};
use crate::net::shown;
use crate::{Checked, Finding, Net, Outcome};

/// `http-sha256`: with `fact`, the SHA-256 of the file at the endpoint (or
/// of the range of `bytes` bytes from sector `start` of `sector` bytes)
/// is the fact's; with `length`, the length of the file is the fact's;
/// with `files` and `facts`, the SHA-256 of each file under the path of the
/// endpoint is its fact's. Each argument but `endpoint` and `files` names
/// a fact.
pub async fn check(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let endpoint = args.endpoint()?;
    let url = https_url(endpoint, None);
    let mut out = Outcome::default();
    if let Ok(files) = args.texts("files") {
        let facts = args.texts("facts")?;
        if files.len() != facts.len() {
            return Err(Finding::failed("the watch has a different count of files and facts"));
        }
        let base = url.trim_end_matches('/');
        for (file, fact) in files.iter().zip(&facts) {
            let fact = fact_of(service, fact)?;
            let (sum, _, _) = read(net, &format!("{base}/{file}"), None).await?;
            judge(&mut out, fact, &sum, file);
        }
        return Ok(out);
    }
    let fact = args.fact("fact")?;
    let range = match args.opt_text("start") {
        Some(_) => {
            let (start, sector, bytes) = (int(&args, "start")?, int(&args, "sector")?, int(&args, "bytes")?);
            Some((start * sector, bytes))
        }
        None => None,
    };
    let (sum, count, total) = read(net, &url, range).await?;
    if let Some((_, bytes)) = range
        && count != bytes
    {
        out.found(Finding::failed(format!("the range of {} gave {count} bytes, not {bytes}", shown(&url))));
    }
    judge(&mut out, fact, &sum, shown(&url));
    if let Some(length) = args.opt_fact("length")? {
        let want = length.value().as_int().ok_or_else(|| Finding::failed("the fact of length is no integer"))?;
        match total {
            Some(total) if total == want => out.saw(format!("{} is {total} bytes", shown(&url))),
            Some(total) => out.found(Finding::moved(named(length), want.to_string(), total.to_string())),
            None => out.found(Finding::failed(format!("{} gave no length", shown(&url)))),
        }
    }
    Ok(out)
}

fn fact_of<'a>(service: &'a Service, name: &str) -> Result<&'a Fact, Finding> {
    service.fact(name).ok_or_else(|| Finding::failed(format!("the service has no fact {name:?}")))
}

/// The integer of the fact that the argument `name` names.
fn int(args: &Args<'_>, name: &str) -> Result<u64, Finding> {
    let fact = args.fact(name)?;
    fact.value()
        .as_int()
        .and_then(|i| u64::try_from(i).ok())
        .ok_or_else(|| Finding::failed(format!("the fact {:?} is no count", fact.name())))
}

fn judge(out: &mut Outcome, fact: &Fact, sum: &str, what: &str) {
    match fact.value().as_text() {
        Some(want) if want.eq_ignore_ascii_case(sum) => out.saw(format!("the SHA-256 of {what} is the fact's")),
        Some(want) => out.found(Finding::moved(named(fact), want, sum)),
        None => out.found(Finding::failed(format!("the fact {:?} is no text", fact.name()))),
    }
}

/// The SHA-256 of the body of a GET of `url` (of the range `(start, count)`
/// when given), the count of bytes read, and the length of the whole file:
/// the `Content-Length` of a whole answer, the total of `Content-Range`.
async fn read(net: &Net, url: &str, range: Option<(u64, u64)>) -> Result<(String, u64, Option<i64>), Finding> {
    let mut request = net.request("GET", url).redirects(5);
    if let Some((start, count)) = range {
        request = request.header("Range", format!("bytes={start}-{}", start + count.max(1) - 1));
    }
    let mut answer = net.streamed(request).await?;
    let want = if range.is_some() { 206 } else { 200 };
    if answer.status != want {
        return Err(Finding::failed(format!("{} answered HTTP {}, not {want}", shown(url), answer.status)));
    }
    let total = match range {
        Some(_) => answer.header("content-range").and_then(|r| r.rsplit('/').next()).and_then(|t| t.parse().ok()),
        None => answer.header("content-length").and_then(|l| l.parse().ok()),
    };
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let n =
            answer.read(&mut chunk).await.map_err(|e| Finding::failed(format!("the body of {}: {e}", shown(url))))?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
        count += n as u64;
    }
    if range.is_none() && total.is_some_and(|t: i64| t as u64 != count) {
        return Err(Finding::failed(format!("{} ended after {count} bytes of its length", shown(url))));
    }
    Ok((hex(&hash.finalize()), count, total.or(range.is_none().then_some(count as i64))))
}
