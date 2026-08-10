//! Where a request came from, for the purpose of throttling it.
//!
//! # There was no trustworthy client address before this file
//!
//! `proxy_guard` answers one question — "did this come through Caddy?" — and answers it
//! with a shared secret. It does not record a peer address, and there is no `ConnectInfo`
//! in the router, so nothing in the application knew a client address at all. This module
//! is the smallest honest way to get one, and the reasoning is worth stating because the
//! obvious shortcuts are all wrong:
//!
//! - **The TCP peer address is useless here.** Caddy runs on a different host, so every
//!   request's peer is Caddy. Throttling on it would put the whole internet in one bucket
//!   and ten wrong passwords would lock out every guest at once.
//! - **A raw `X-Forwarded-For` is worse than nothing.** Any client can send one, and an
//!   attacker who is being counted per address simply sends a different value each time.
//!   A counter that the party being counted can reset is not a counter.
//!
//! What *is* trustworthy is the combination: on a request the proxy boundary attested,
//! the header cannot have arrived from a client directly, because a client cannot produce
//! `GW_PROXY_SECRET`. So the rule is:
//!
//! | Request | Address used |
//! |---|---|
//! | Attested ([`Attested`] present) | the **rightmost** `X-Forwarded-For` entry |
//! | Attested, header absent or unparseable | [`UNATTRIBUTED`], one shared bucket |
//! | Not attested (loopback bind, `just dev`) | [`LOOPBACK`], one shared bucket |
//!
//! **Rightmost, not leftmost.** Caddy *appends* the address it observed to whatever
//! `X-Forwarded-For` arrived, so the last entry is the one entry no client wrote.
//! Everything to its left came from the client and is theirs to invent. Reading the
//! leftmost value — which is what "the original client IP" advice usually means, and is
//! correct only when every hop is trusted — would hand an attacker an unlimited supply of
//! addresses.
//!
//! **This assumes Caddy is the outermost proxy.** It is: Caddy on OPNsense terminates TLS
//! from the internet and forwards here. If a CDN or a second reverse proxy is ever put in
//! front, the rightmost entry becomes *that* proxy and every request will share one
//! bucket — noisy and over-strict, never permissive. That is the right direction for this
//! to fail in, but it is a thing to remember rather than to discover.

use crate::proxy_guard::Attested;
use axum::http::HeaderMap;
use std::net::IpAddr;

/// The header Caddy appends the observed client address to.
const FORWARDED_FOR: &str = "x-forwarded-for";

/// The bucket for an attested request whose `X-Forwarded-For` is missing or unreadable.
///
/// One shared bucket, deliberately. It is a misconfiguration — the proxy is supposed to
/// set the header — and sharing a bucket throttles harder than the truth would, never
/// softer. Fail closed.
pub const UNATTRIBUTED: &str = "unattributed";

/// The bucket for an unattested request. Reachable only on a loopback bind, where the
/// only client is this machine.
pub const LOOPBACK: &str = "loopback";

/// The address to count this attempt against.
///
/// Split from the handler and given no extractors so every rule above is a unit test that
/// needs neither a server nor a socket.
pub fn client_address(attested: Option<&Attested>, headers: &HeaderMap) -> String {
    if attested.is_none() {
        // Nothing in front of this server. `X-Forwarded-For` here is whatever the caller
        // felt like writing, and reading it would be strictly worse than ignoring it.
        return LOOPBACK.to_string();
    }

    let forwarded = headers
        .get(FORWARDED_FOR)
        .and_then(|value| value.to_str().ok())
        // `rsplit` first element == the last entry: the one Caddy appended.
        .and_then(|raw| raw.rsplit(',').next())
        .map(str::trim)
        // Parsed as an address rather than used as a string. It normalises the many
        // spellings of one IPv6 address into one bucket, and it means a header full of
        // junk cannot mint an unbounded number of rows in `login_attempts`.
        .and_then(|candidate| candidate.parse::<IpAddr>().ok());

    match forwarded {
        Some(address) => address.to_string(),
        None => {
            tracing::warn!(
                "an attested request carried no usable X-Forwarded-For; every sign-in \
                 attempt will share one throttling bucket until the proxy sets it"
            );
            UNATTRIBUTED.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{client_address, LOOPBACK, UNATTRIBUTED};
    use crate::proxy_guard::Attested;
    use axum::http::HeaderMap;

    fn forwarded(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().unwrap());
        headers
    }

    #[test]
    fn an_unattested_request_ignores_the_header_entirely() {
        // The header is free for anyone to write. Believing it here would let an attacker
        // rotate it and get a fresh budget every time.
        assert_eq!(
            client_address(None, &forwarded("203.0.113.7")),
            LOOPBACK,
            "a forged X-Forwarded-For was believed"
        );
        assert_eq!(client_address(None, &HeaderMap::new()), LOOPBACK);
    }

    #[test]
    fn an_attested_request_uses_the_rightmost_entry() {
        // Caddy appends what it observed. Everything to the left arrived from the client.
        assert_eq!(
            client_address(Some(&Attested), &forwarded("10.0.0.1, 203.0.113.7")),
            "203.0.113.7"
        );
        assert_eq!(
            client_address(Some(&Attested), &forwarded("203.0.113.7")),
            "203.0.113.7"
        );
    }

    #[test]
    fn a_client_supplied_left_hand_entry_cannot_change_the_bucket() {
        // The property the rightmost rule exists for, stated as one assertion: whatever an
        // attacker prepends, the bucket is the same.
        let real = client_address(Some(&Attested), &forwarded("203.0.113.7"));
        for forged in [
            "10.0.0.1, 203.0.113.7",
            "1.1.1.1, 2.2.2.2, 203.0.113.7",
            "nonsense, 203.0.113.7",
        ] {
            assert_eq!(client_address(Some(&Attested), &forwarded(forged)), real);
        }
    }

    #[test]
    fn one_address_spelled_several_ways_is_one_bucket() {
        // Otherwise an IPv6 client gets a fresh budget per spelling.
        let canonical = client_address(Some(&Attested), &forwarded("2001:db8::1"));
        for spelling in ["2001:0db8:0000:0000:0000:0000:0000:0001", "2001:DB8::1"] {
            assert_eq!(
                client_address(Some(&Attested), &forwarded(spelling)),
                canonical
            );
        }
    }

    #[test]
    fn an_attested_request_with_no_usable_header_shares_one_bucket_rather_than_getting_many() {
        // Fail closed: a misconfigured proxy must make throttling stricter, never
        // absent, and must not let a junk header mint an unbounded number of counters.
        for headers in [
            HeaderMap::new(),
            forwarded(""),
            forwarded("nicht-eine-adresse"),
            forwarded("999.999.999.999"),
        ] {
            assert_eq!(client_address(Some(&Attested), &headers), UNATTRIBUTED);
        }
    }
}
