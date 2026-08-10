//! The real breach corpus: Have I Been Pwned's range API.
//!
//! The protocol and every decision about it live in `gw_auth::breach`, which is where the
//! digest is computed and the response is read. This file is only the network half —
//! deliberately, so that the part with the security properties in it can be tested
//! exhaustively without a socket.

use gw_auth::breach::{BreachFuture, BreachRange, BreachUnavailable};
use std::time::Duration;

const ENDPOINT: &str = "https://api.pwnedpasswords.com/range";

/// Short. Setting a password must not hang on somebody else's server, and the outcome of
/// a timeout is "allow, and record that the check did not happen" rather than a failure,
/// so waiting longer buys nothing.
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct HibpCorpus {
    http: reqwest::Client,
    /// Overridable so a test can point at a local stand-in. Never used to send anything
    /// but a five-character prefix — see `gw_auth::breach`.
    endpoint: String,
}

impl HibpCorpus {
    pub fn new() -> anyhow::Result<Self> {
        // Same reason as `OidcClient::new`: rustls needs a process-wide provider and the
        // crate default needs CMake. Losing the race is not an error.
        let _ = rustls::crypto::ring::default_provider().install_default();

        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("great-wiki/", env!("CARGO_PKG_VERSION")))
                .build()?,
            endpoint: ENDPOINT.to_string(),
        })
    }

    pub fn at(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

impl BreachRange for HibpCorpus {
    fn fetch<'a>(&'a self, prefix: &'a str) -> BreachFuture<'a> {
        Box::pin(async move {
            self.http
                .get(format!("{}/{prefix}", self.endpoint))
                // Padding makes every response the same size, so an observer who can see
                // the length cannot narrow down which prefix was asked for.
                .header("Add-Padding", "true")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| BreachUnavailable::new(format!("range request failed: {e}")))?
                .text()
                .await
                .map_err(|e| BreachUnavailable::new(format!("range response unreadable: {e}")))
        })
    }
}
