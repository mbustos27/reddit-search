//! HTTP behind a trait, with a real HTTPS client and a replaying one for tests. Also the clock, so pacing is testable.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub trait Transport {
    /// `Err` means no HTTP response at all (DNS, TLS, timeout). Any status code is an `Ok`.
    fn get(&self, url: &str) -> Result<Response, String>;
}

pub trait Clock {
    fn now_ms(&self) -> u64;
    fn sleep_ms(&self, ms: u64);
}

pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock(Instant::now())
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// HTTPS through Windows' own TLS (SChannel) and the Windows certificate store. One identity, stated plainly; it is
/// never varied to get past a refusal. Redirects are not followed, so a request can only ever reach the host it names.
pub struct Https {
    agent: ureq::Agent,
}

pub const USER_AGENT: &str = concat!("rdt/", env!("CARGO_PKG_VERSION"), " (personal research tool)");
const MAX_BODY: u64 = 64 * 1024 * 1024;

impl Default for Https {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(45)))
            .http_status_as_error(false)
            .max_redirects(0)
            .https_only(true)
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
            .build()
            .into();
        Https { agent }
    }
}

impl Transport for Https {
    fn get(&self, url: &str) -> Result<Response, String> {
        let mut resp = self.agent.get(url).call().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let headers = resp.headers().iter().map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned())).collect();
        let body = resp.body_mut().with_config().limit(MAX_BODY).read_to_string().map_err(|e| e.to_string())?;
        Ok(Response { status, headers, body })
    }
}

/// Replays canned responses: the first route whose text appears in the URL answers. Records every URL asked for.
#[derive(Default)]
pub struct Fixtures {
    routes: Vec<(String, Response)>,
    pub calls: RefCell<Vec<String>>,
}

impl Fixtures {
    pub fn route(mut self, url_contains: &str, status: u16, headers: &[(&str, &str)], body: &str) -> Self {
        let headers = headers.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        self.routes.push((url_contains.to_owned(), Response { status, headers, body: body.to_owned() }));
        self
    }
}

impl Transport for Fixtures {
    fn get(&self, url: &str) -> Result<Response, String> {
        self.calls.borrow_mut().push(url.to_owned());
        self.routes.iter().find(|(part, _)| url.contains(part.as_str())).map(|(_, r)| r.clone()).ok_or_else(|| format!("no fixture for {url}"))
    }
}

/// A clock that only moves when told to sleep, and remembers each sleep.
#[derive(Default)]
pub struct FakeClock {
    now: Cell<u64>,
    pub sleeps: RefCell<Vec<u64>>,
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn sleep_ms(&self, ms: u64) {
        self.sleeps.borrow_mut().push(ms);
        self.now.set(self.now.get() + ms);
    }
}

impl FakeClock {
    pub fn advance(&self, ms: u64) {
        self.now.set(self.now.get() + ms);
    }
}
