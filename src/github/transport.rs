//! Proxy-aware HTTP transport for the GitHub API.
//!
//! octocrab builds its own hyper client and that client has no notion of an
//! HTTP proxy: `OctocrabBuilder::build()` hardcodes a TLS connector on top of
//! hyper-util's `HttpConnector`, which never consults `HTTPS_PROXY` and
//! friends. Behind a corporate CONNECT proxy that makes every GitHub API call
//! fail on connect — `client error (Connect): deadline has elapsed` — while
//! `git` (which honours `http_proxy`) keeps working, so `stax submit` fetches
//! successfully and then dies on the first API request.
//!
//! When proxy environment variables are present we therefore run octocrab on
//! top of `reqwest` instead — the same client stack the GitLab and Gitea
//! forges already use. reqwest speaks CONNECT, honours `NO_PROXY` and proxy
//! credentials, and verifies certificates through the platform verifier, which
//! also covers proxies that terminate TLS with a CA from the OS trust store.
//!
//! `OctocrabBuilder::with_service()` bypasses octocrab's own layer stack, so
//! the three layers that make an `Octocrab` behave like an `Octocrab` are
//! reapplied here in octocrab's own order: extra headers (a `User-Agent` is
//! mandatory for GitHub), base URI resolution, and the auth header. Retry is
//! reimplemented as a loop because octocrab's `RetryConfig` policy is tied to
//! `hyper_util::client::legacy::Error`, an error type we cannot produce.
//!
//! This is the only GitHub transport: it runs whether or not a proxy is
//! configured, so the proxied and unproxied paths cannot drift apart, and
//! GitHub now reaches the network exactly the way GitLab and Gitea already do
//! (`forge::build_http_client`) — same client, same timeout shape, same
//! `stax` user-agent.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use http::header::{HeaderValue, USER_AGENT};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use octocrab::service::middleware::auth_header::AuthHeaderLayer;
use octocrab::service::middleware::base_uri::BaseUriLayer;
use octocrab::service::middleware::extra_headers::ExtraHeadersLayer;
use octocrab::{AuthState, OctoBody, Octocrab, OctocrabBuilder};

const GITHUB_API_BASE_URI: &str = "https://api.github.com";
const GITHUB_UPLOAD_BASE_URI: &str = "https://uploads.github.com";

/// Timeouts and retry budget for GitHub API requests. The shape matches
/// `forge::build_http_client`, so every forge behaves the same way on a slow
/// or half-open connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
const RETRY_COUNT: usize = 1;

/// Proxy environment variables, most specific first — the same set and order
/// reqwest, curl and git resolve.
const PROXY_ENV_VARS: [&str; 6] = [
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The proxy variable in effect for this process, as `(name, value)`.
fn proxy_env_override() -> Option<(&'static str, String)> {
    proxy_env_from(|name| std::env::var(name).ok())
}

fn proxy_env_from<F>(lookup: F) -> Option<(&'static str, String)>
where
    F: Fn(&str) -> Option<String>,
{
    PROXY_ENV_VARS.iter().find_map(|name| {
        let value = lookup(name)?;
        let value = value.trim();
        (!value.is_empty()).then(|| (*name, value.to_string()))
    })
}

/// Strip any `user:password@` userinfo so a proxy URL is safe to print.
fn redact_proxy_url(value: &str) -> String {
    let (scheme, rest) = match value.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, value),
    };
    let rest = match rest.rsplit_once('@') {
        Some((_, host)) => format!("***@{host}"),
        None => rest.to_string(),
    };
    match scheme {
        Some(scheme) => format!("{scheme}://{rest}"),
        None => rest,
    }
}

/// Guidance for an error that never got as far as talking to GitHub, or `None`
/// when the error is not a connect failure.
///
/// A connect failure says nothing about the token, so it must not inherit the
/// auth guidance `enrich_api_error` adds otherwise. What matters instead is
/// whether the request was supposed to go through a proxy.
pub(crate) fn connect_failure_context(message: &str) -> Option<String> {
    const CONNECT_MARKERS: [&str; 6] = [
        "(Connect)",
        "deadline has elapsed",
        "error trying to connect",
        "dns error",
        "operation timed out",
        "Connection refused",
    ];

    if !CONNECT_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
    {
        return None;
    }

    Some(connect_failure_hint(proxy_env_override()))
}

fn connect_failure_hint(proxy: Option<(&str, String)>) -> String {
    match proxy {
        Some((name, value)) => format!(
            "Could not reach the GitHub API. Requests use the proxy from {}={}; check that the \
             proxy is up and permits CONNECT to the API host, or exclude the host via NO_PROXY.",
            name,
            redact_proxy_url(&value),
        ),
        None => "Could not reach the GitHub API, and no HTTP proxy is configured. If this network \
                 requires one, set HTTPS_PROXY (stax honours ALL_PROXY/HTTPS_PROXY/HTTP_PROXY and \
                 NO_PROXY); otherwise check your VPN, DNS, or firewall."
            .to_string(),
    }
}

/// Build the `Octocrab` stax talks to GitHub through.
pub(crate) fn build_client(token: &str, api_base_url: Option<&str>) -> Result<Octocrab> {
    build_client_with(
        token,
        api_base_url,
        Timeouts {
            connect: CONNECT_TIMEOUT,
            read: READ_TIMEOUT,
            total: TOTAL_TIMEOUT,
        },
        RETRY_COUNT,
    )
}

struct Timeouts {
    connect: Duration,
    read: Duration,
    total: Duration,
}

fn build_client_with(
    token: &str,
    api_base_url: Option<&str>,
    timeouts: Timeouts,
    retries: usize,
) -> Result<Octocrab> {
    let base_uri: http::Uri = api_base_url
        .unwrap_or(GITHUB_API_BASE_URI)
        .parse()
        .context("Failed to parse GitHub API base URL")?;
    let upload_uri: http::Uri = GITHUB_UPLOAD_BASE_URI
        .parse()
        .expect("static GitHub upload URI is valid");

    // reqwest is built with `rustls-no-provider`, so building a client panics
    // unless a crypto provider is installed. `cli::run` installs one at
    // startup; do it here too so a client built outside that entry point
    // (tests, library use) cannot panic.
    ensure_rustls_provider();

    let http = reqwest::Client::builder()
        .connect_timeout(timeouts.connect)
        .read_timeout(timeouts.read)
        .timeout(timeouts.total)
        // Match octocrab's redirect behaviour: cross-origin redirects are
        // followed (GitHub sends them for artifact and log downloads) but
        // reqwest strips `Authorization` when the origin changes, so the token
        // never reaches another host.
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("Failed to build proxy-aware GitHub HTTP client")?;

    let auth_header = HeaderValue::from_str(&format!("Bearer {}", token))
        .context("GitHub token cannot be sent in an Authorization header")?;
    let extra_headers = Arc::new(vec![(USER_AGENT, HeaderValue::from_static("stax"))]);

    let service = tower::service_fn(move |request: Request<OctoBody>| {
        let http = http.clone();
        async move { send_with_retry(http, request, retries).await }
    });

    OctocrabBuilder::new_empty()
        .with_service(service)
        .with_layer(&ExtraHeadersLayer::new(extra_headers))
        .with_layer(&BaseUriLayer::new(base_uri.clone()))
        .with_layer(&AuthHeaderLayer::new(
            Some(auth_header),
            base_uri,
            upload_uri,
        ))
        .with_auth(AuthState::None)
        .build()
        .context("Failed to build proxy-aware GitHub client")
}

/// Send one octocrab request through reqwest, retrying transport failures and
/// the status codes octocrab's own `RetryConfig::Simple` retries.
async fn send_with_retry(
    http: reqwest::Client,
    request: Request<OctoBody>,
    retries: usize,
) -> Result<Response<reqwest::Body>, BoxError> {
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map_err(|err| -> BoxError {
            format!("Failed to buffer GitHub request body: {err}").into()
        })?
        .to_bytes();

    let request =
        reqwest::Request::try_from(Request::from_parts(parts, reqwest::Body::from(body)))?;

    let mut attempts_left = retries;
    loop {
        // A buffered body always clones; this only fails if that stops holding.
        let attempt = request
            .try_clone()
            .ok_or("GitHub request body cannot be replayed")?;

        let result = http.execute(attempt).await;
        let retryable = match &result {
            Ok(response) => is_retryable_status(response.status()),
            Err(_) => true,
        };
        if retryable && attempts_left > 0 {
            attempts_left -= 1;
            continue;
        }

        return Ok(result?.into());
    }
}

fn ensure_rustls_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    fn test_client(server: &MockServer, retries: usize) -> Octocrab {
        ensure_crypto_provider();
        build_client_with(
            "test-token",
            Some(&server.uri()),
            Timeouts {
                connect: Duration::from_secs(5),
                read: Duration::from_secs(5),
                total: Duration::from_secs(5),
            },
            retries,
        )
        .expect("GitHub client builds")
    }

    #[test]
    fn proxy_env_is_detected_in_resolution_order() {
        assert_eq!(
            proxy_env_from(lookup(&[
                ("HTTPS_PROXY", "http://specific:8080"),
                ("ALL_PROXY", "http://catch-all:8080"),
            ])),
            Some(("ALL_PROXY", "http://catch-all:8080".to_string()))
        );
        assert_eq!(
            proxy_env_from(lookup(&[("http_proxy", "http://lower:8080")])),
            Some(("http_proxy", "http://lower:8080".to_string()))
        );
    }

    #[test]
    fn blank_proxy_env_is_not_a_proxy() {
        assert_eq!(proxy_env_from(lookup(&[("HTTPS_PROXY", "   ")])), None);
        assert_eq!(proxy_env_from(lookup(&[])), None);
    }

    #[test]
    fn connect_failure_context_ignores_unrelated_errors() {
        assert_eq!(connect_failure_context("Bad credentials (401)"), None);
        assert!(
            connect_failure_context(
                "Service Error: client error (Connect): client error (Connect): deadline has elapsed"
            )
            .is_some()
        );
    }

    #[test]
    fn connect_failure_hint_names_the_proxy_variable_without_credentials() {
        let hint = connect_failure_hint(Some((
            "HTTPS_PROXY",
            "http://alice:s3cret@proxy.example:8080".to_string(),
        )));

        assert!(
            hint.contains("HTTPS_PROXY=http://***@proxy.example:8080"),
            "expected redacted proxy in hint: {hint}"
        );
        assert!(
            !hint.contains("s3cret") && !hint.contains("alice"),
            "proxy credentials leaked into hint: {hint}"
        );
    }

    #[test]
    fn connect_failure_hint_without_a_proxy_points_at_the_env_vars() {
        let hint = connect_failure_hint(None);

        assert!(hint.contains("HTTPS_PROXY"), "got: {hint}");
        assert!(hint.contains("NO_PROXY"), "got: {hint}");
    }

    #[test]
    fn proxy_url_credentials_are_redacted() {
        assert_eq!(
            redact_proxy_url("http://user:s3cret@proxy.example:8080"),
            "http://***@proxy.example:8080"
        );
        assert_eq!(
            redact_proxy_url("http://proxy.example:8080"),
            "http://proxy.example:8080"
        );
        assert_eq!(redact_proxy_url("proxy.example:8080"), "proxy.example:8080");
    }

    #[tokio::test]
    async fn client_sends_octocrab_headers_and_resolves_base_uri() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .and(header("user-agent", "stax"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "octocat",
                "id": 1,
                "node_id": "node",
                "avatar_url": "https://example.com/a.png",
                "gravatar_id": "",
                "url": "https://example.com/users/octocat",
                "html_url": "https://example.com/octocat",
                "followers_url": "https://example.com/followers",
                "following_url": "https://example.com/following",
                "gists_url": "https://example.com/gists",
                "starred_url": "https://example.com/starred",
                "subscriptions_url": "https://example.com/subscriptions",
                "organizations_url": "https://example.com/orgs",
                "repos_url": "https://example.com/repos",
                "events_url": "https://example.com/events",
                "received_events_url": "https://example.com/received_events",
                "type": "User",
                "site_admin": false
            })))
            .mount(&server)
            .await;

        let octocrab = test_client(&server, 0);
        let user = octocrab
            .current()
            .user()
            .await
            .expect("authenticated user request succeeds through the custom transport");

        assert_eq!(user.login, "octocat");
    }

    #[tokio::test]
    async fn client_retries_server_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "message": "server on fire"
            })))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true })),
            )
            .with_priority(2)
            .mount(&server)
            .await;

        let octocrab = test_client(&server, 1);
        let body: serde_json::Value = octocrab
            .get("/probe", None::<&()>)
            .await
            .expect("retry recovers from a single 500");

        assert_eq!(body["ok"], serde_json::json!(true));
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            2
        );
    }

    #[tokio::test]
    async fn client_surfaces_errors_without_retries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(serde_json::json!({ "message": "server on fire" })),
            )
            .mount(&server)
            .await;

        let octocrab = test_client(&server, 0);
        let error = octocrab
            .get::<serde_json::Value, _, ()>("/probe", None)
            .await
            .expect_err("a 500 without retries must surface");

        let octocrab::Error::GitHub { source, .. } = &error else {
            panic!("expected the 500 to surface as a GitHub API error, got: {error:?}");
        };
        assert_eq!(source.message, "server on fire");
        assert_eq!(source.status_code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            server.received_requests().await.unwrap_or_default().len(),
            1
        );
    }
}
