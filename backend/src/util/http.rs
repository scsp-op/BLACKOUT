//! One place where this deployment says who it is.
//!
//! Two instances of this app now run against the same upstreams from
//! different hosts. Before this module they were indistinguishable: of the
//! twelve HTTP clients in `fetchers/`, nine sent no `User-Agent` at all and
//! the three that did disagreed with each other
//! (`globe-censorship-tracker/0.1`, `Censorship Tracker`,
//! `blackout-satellite-tracker/...`). Every client is built here instead, so
//! there is exactly one answer to "who is this".
//!
//! # What this does and does not buy
//!
//! It does **not** separate rate limits, and it is important not to believe
//! that it does. Every upstream here enforces limits on one of two things:
//!
//!   * the credential — Cloudflare Radar, Internet Society Pulse, and
//!     PeeringDB (in `scripts/gen_ixp_data.mjs`). Separate tokens per
//!     deployment are what separates these, and nothing else will.
//!   * the source IP — OONI, IODA, Tor Metrics, RIPEstat, CelesTrak,
//!     SatNOGS, Our World in Data. Two deployments on different platforms
//!     already have different addresses, so these are separate for free.
//!
//! What it does buy is that when an upstream throttles or blocks someone,
//! their logs say *which* deployment it was, and a maintainer who wants to
//! get in touch has somewhere to write. Several of these are free services
//! run by volunteers or a small nonprofit; identifying honestly costs
//! nothing. RIPEstat goes further and documents `sourceapp` as the
//! convention for registered callers, which is why `sourceapp` is derived
//! from the same identity rather than being its own unrelated string.

use std::sync::OnceLock;

/// What an unconfigured build calls itself. Deliberately not a real
/// deployment name: seeing `id=local` in a production log is the signal that
/// `DEPLOYMENT_ID` was never set.
const DEFAULT_DEPLOYMENT_ID: &str = "local";

/// Where an upstream operator can find out what this is. Overridable because
/// a fork or a second deployment should be able to point somewhere else.
const DEFAULT_CONTACT: &str = "https://github.com/moumenalaoui/globe";

/// Long enough for a descriptive slug, short enough that it cannot be used to
/// smuggle anything interesting into a header or a query string.
const MAX_ID_LEN: usize = 48;

/// A short slug naming this deployment, from `DEPLOYMENT_ID`.
///
/// Sanitised rather than trusted: this value ends up in a `User-Agent` header
/// and in a RIPEstat query parameter, and a stray newline or control
/// character would either fail `HeaderValue` construction — taking the whole
/// fetcher down — or corrupt the query. Anything outside `[A-Za-z0-9._-]` is
/// dropped, and a value that sanitises to nothing falls back to the default.
pub fn deployment_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let Some(raw) = std::env::var("DEPLOYMENT_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        else {
            return DEFAULT_DEPLOYMENT_ID.to_string();
        };

        let cleaned = sanitize(&raw);
        if cleaned.is_empty() {
            eprintln!(
                "WARNING: DEPLOYMENT_ID `{raw}` contains no usable characters — \
                 falling back to `{DEFAULT_DEPLOYMENT_ID}`."
            );
            return DEFAULT_DEPLOYMENT_ID.to_string();
        }
        if cleaned != raw {
            eprintln!("WARNING: DEPLOYMENT_ID `{raw}` was sanitised to `{cleaned}`.");
        }
        cleaned
    })
}

/// Contact URL advertised to upstreams, from `DEPLOYMENT_CONTACT`.
pub fn contact() -> &'static str {
    static CONTACT: OnceLock<String> = OnceLock::new();
    CONTACT.get_or_init(|| {
        std::env::var("DEPLOYMENT_CONTACT")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && v.chars().all(|c| !c.is_control()))
            .unwrap_or_else(|| DEFAULT_CONTACT.to_string())
    })
}

/// `blackout-<component>/<version> (+<contact>; id=<deployment>)`
///
/// `component` names the fetcher, matching the `fetch_runs` vocabulary, so a
/// provider's logs and this app's own bookkeeping use the same words for the
/// same traffic.
pub fn user_agent(component: &str) -> String {
    format!(
        "blackout-{component}/{} (+{}; id={})",
        env!("CARGO_PKG_VERSION"),
        contact(),
        deployment_id()
    )
}

/// The only way a fetcher should build an HTTP client. Callers add their own
/// timeout and anything else they need; the identity is already set.
///
/// Note what this deliberately is *not*: some reference implementations of
/// this kind of tool rotate randomised browser user-agents and fake
/// `X-Forwarded-For` headers so one client looks like many real users. That
/// is misrepresentation to dodge a provider's rate limiting, and it would not
/// even help here — the failure this codebase has actually seen is a TCP
/// connect timeout, before any header is sent.
pub fn client(component: &str) -> reqwest::ClientBuilder {
    reqwest::Client::builder().user_agent(user_agent(component))
}

/// The value sent as RIPEstat's `sourceapp`, from `RIPESTAT_SOURCEAPP` or
/// derived from the deployment id.
///
/// Derived rather than defaulted to a fixed string so that setting
/// `DEPLOYMENT_ID` alone is enough to separate two deployments everywhere
/// they are visible — forgetting the second variable cannot silently leave
/// them sharing an identity.
pub fn ripestat_sourceapp() -> &'static str {
    static SOURCEAPP: OnceLock<String> = OnceLock::new();
    SOURCEAPP.get_or_init(|| {
        std::env::var("RIPESTAT_SOURCEAPP")
            .ok()
            .map(|v| sanitize(v.trim()))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| format!("blackout-{}", deployment_id()))
    })
}

/// One line at boot naming this deployment, so a log tells you which of the
/// two instances you are reading before anything has gone wrong.
pub fn report_identity() {
    println!(
        "Outbound identity: {} (RIPEstat sourceapp: {})",
        user_agent("<fetcher>"),
        ripestat_sourceapp()
    );
    if deployment_id() == DEFAULT_DEPLOYMENT_ID {
        eprintln!(
            "WARNING: DEPLOYMENT_ID is unset, so this instance identifies itself as \
             `{DEFAULT_DEPLOYMENT_ID}` to every upstream. Set it on any real deployment so \
             two instances are distinguishable in a provider's logs."
        );
    }
}

fn sanitize(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(MAX_ID_LEN)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_slug_characters_and_drops_everything_else() {
        assert_eq!(sanitize("scsp-replit"), "scsp-replit");
        assert_eq!(sanitize("scsp_replit.v2"), "scsp_replit.v2");
        // The cases that would break a HeaderValue or a query string.
        assert_eq!(sanitize("bad\nvalue"), "badvalue");
        assert_eq!(sanitize("a b&c=d"), "abcd");
        assert_eq!(sanitize("émoji🚀"), "moji");
    }

    #[test]
    fn sanitize_is_bounded() {
        assert_eq!(sanitize(&"x".repeat(500)).len(), MAX_ID_LEN);
    }

    /// The builder is only useful if the header actually leaves the process.
    /// Asserting on `user_agent()`'s return value would pass even if
    /// `client()` forgot to apply it, so this makes a real request to a real
    /// socket and reads back what arrived.
    #[tokio::test]
    async fn the_identity_reaches_the_wire() {
        use axum::{Router, routing::get};
        use std::sync::{Arc, Mutex};

        let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let captured = seen.clone();
        let app = Router::new().route(
            "/",
            get(move |headers: axum::http::HeaderMap| {
                let captured = captured.clone();
                async move {
                    *captured.lock().unwrap() = headers
                        .get(reqwest::header::USER_AGENT.as_str())
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    "ok"
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = client("ioda").build().unwrap();
        client
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();

        let received = seen.lock().unwrap().clone().expect("no User-Agent arrived");
        assert!(
            received.starts_with("blackout-ioda/"),
            "component must be in the UA, got {received:?}"
        );
        assert!(
            received.contains(&format!("id={}", deployment_id())),
            "deployment id must be in the UA, got {received:?}"
        );
    }

    #[test]
    fn a_user_agent_is_always_a_valid_header_value() {
        // The whole point of sanitising: an unusable DEPLOYMENT_ID must not be
        // able to take every fetcher down at client-build time.
        let ua = format!(
            "blackout-ooni/{} (+{}; id={})",
            env!("CARGO_PKG_VERSION"),
            DEFAULT_CONTACT,
            sanitize("nasty\r\nvalue")
        );
        assert!(reqwest::header::HeaderValue::from_str(&ua).is_ok());
    }
}
