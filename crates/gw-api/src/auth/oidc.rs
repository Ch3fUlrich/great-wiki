//! The OpenID Connect authorization-code flow against Authelia.
//!
//! great-wiki is an OIDC *client*, not a forward-auth tenant: the edge does not gate the
//! wiki, the application does. That is what makes public pages readable without signing
//! in, per-person access possible, and a local guest account able to exist at all —
//! Authelia does not know about those, and never will (ADR 0002).
//!
//! Four things are checked on the way back in, and each one is load-bearing rather than
//! ceremonial:
//!
//! - **PKCE with S256**, although this is a confidential client with a secret. The secret
//!   defends the token exchange only as long as it stays secret; PKCE removes the
//!   authorization-code interception class outright, because the verifier never leaves
//!   this browser.
//! - **`state`**, compared against a cookie this browser was issued. This is the CSRF
//!   defence for the whole flow: without it, somebody can complete *their* login inside
//!   *your* session.
//! - **`nonce`**, compared against the id token's claim, which binds the token to this
//!   particular authorization request and stops a token minted elsewhere being replayed.
//! - **The signature and the issuer.** An unverified token's claims are an attacker's
//!   input, not an identity.

use crate::error::ApiError;
use crate::routes::AppState;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum_extra::extract::CookieJar;
use base64::Engine;
use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use subtle::ConstantTimeEq;

use super::session::{
    cleared_cookie, flow_cookie, found, hash_token, new_session_token, session_cookie,
};

/// `openid` is mandatory; `groups` is what D-M2-1 keys default reach on, so a sign-in
/// without it would give everybody the public-only baseline and look like a permission
/// bug rather than a scope mistake.
const SCOPE: &str = "openid profile email groups";

pub const STATE_COOKIE: &str = "__Host-gw_oidc_state";
pub const NONCE_COOKIE: &str = "__Host-gw_oidc_nonce";
pub const VERIFIER_COOKIE: &str = "__Host-gw_oidc_verifier";

/// How long a half-finished login stays valid. Long enough to read a consent screen and
/// reach for a second factor, short enough that an abandoned tab is not a standing half
/// of a flow somebody else could complete.
const FLOW_TTL_SECONDS: i64 = 600;

/// The provider is on the other side of the network. Every call to it is bounded, because
/// a sign-in that hangs is a sign-in that holds a connection open indefinitely.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// What it takes to be this client.
#[derive(Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

// Hand-written and redacting, for the same reason `ProxyGuard`'s is: `#[derive(Debug)]`
// would put the client secret into any log line that ever formats the configuration,
// including one added years from now at debug level.
impl std::fmt::Debug for OidcConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OidcConfig")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri)
            .finish()
    }
}

/// Why a sign-in did not happen.
///
/// The split is the whole point. `Rejected` means the response was well-formed and wrong
/// — a bad state, a bad signature, somebody else's nonce — and answers 403. `Provider`
/// means the identity provider could not be reached or answered nonsense, which is a 503
/// and not the caller's fault. Collapsing them would report every attack as an outage and
/// every outage as a refusal.
#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    #[error("the identity provider could not be reached: {0}")]
    Provider(String),
    #[error("the sign-in was refused: {0}")]
    Rejected(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<OidcError> for ApiError {
    fn from(error: OidcError) -> Self {
        match error {
            OidcError::Provider(_) => ApiError::Unavailable,
            OidcError::Rejected(_) => ApiError::Forbidden,
            OidcError::Internal(e) => ApiError::Internal(e),
        }
    }
}

fn rejected(reason: impl Into<String>) -> OidcError {
    OidcError::Rejected(reason.into())
}

/// What the provider says about itself.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub userinfo_endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: String,
    access_token: Option<String>,
}

/// The claims this application reads. Everything else in the token is ignored rather than
/// captured: a claim that is not read cannot be relied on by accident.
#[derive(Debug, Deserialize)]
pub struct IdClaims {
    pub sub: String,
    pub nonce: Option<String>,
    pub preferred_username: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
    /// `None` means the token carried no `groups` claim at all, which is a different thing
    /// from carrying an empty one — the first sends us to the userinfo endpoint, the
    /// second means this person is genuinely in no groups.
    pub groups: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    sub: String,
    preferred_username: Option<String>,
    name: Option<String>,
    email: Option<String>,
    groups: Option<Vec<String>>,
}

/// A verified identity, ready to be written to the store.
#[derive(Debug)]
pub struct VerifiedIdentity {
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub groups: Vec<String>,
}

/// The three secrets one login attempt is built on.
pub struct FlowSecrets {
    pub state: String,
    pub nonce: String,
    pub verifier: String,
}

impl FlowSecrets {
    pub fn generate() -> Self {
        Self {
            state: random_secret(),
            nonce: random_secret(),
            verifier: random_secret(),
        }
    }
}

/// 32 bytes from the operating system's CSPRNG, base64url without padding.
///
/// 43 characters from the unreserved set, which is exactly what RFC 7636 requires of a
/// `code_verifier` (43–128 characters of `[A-Za-z0-9-._~]`), and is more than enough for
/// `state` and `nonce` besides.
pub fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The S256 transformation from RFC 7636: `BASE64URL(SHA256(ASCII(verifier)))`.
pub fn code_challenge_s256(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    // `==` on strings returns at the first differing byte and so times how much of a guess
    // was right. `state` and `nonce` are single-use, which blunts that, but a comparison
    // that leaks nothing costs the same and cannot be got wrong later.
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

pub struct OidcClient {
    config: OidcConfig,
    http: reqwest::Client,
    /// Fetched once. The endpoints of an identity provider do not move between requests,
    /// and refetching would put a network round trip in front of every sign-in for no
    /// benefit. The key set is NOT cached — see `fetch_jwks`.
    discovery: tokio::sync::OnceCell<Discovery>,
}

impl OidcClient {
    pub fn new(config: OidcConfig) -> anyhow::Result<Self> {
        // rustls needs a process-wide crypto provider, and the crate's default is
        // `aws-lc-rs`, which needs CMake to build. `ring` needs neither. Installing twice
        // is not an error worth propagating — the second caller simply loses the race and
        // gets the same provider.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            // A token exchange follows nothing. A redirect from a token endpoint is either
            // a misconfiguration or an attempt to get this client to POST its credentials
            // somewhere else.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("great-wiki/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            config,
            http,
            discovery: tokio::sync::OnceCell::new(),
        })
    }

    pub fn config(&self) -> &OidcConfig {
        &self.config
    }

    pub async fn discovery(&self) -> Result<&Discovery, OidcError> {
        self.discovery
            .get_or_try_init(|| async {
                let url = format!(
                    "{}/.well-known/openid-configuration",
                    self.config.issuer.trim_end_matches('/')
                );
                let document: Discovery = self
                    .http
                    .get(&url)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .map_err(|e| OidcError::Provider(format!("discovery failed: {e}")))?
                    .json()
                    .await
                    .map_err(|e| OidcError::Provider(format!("discovery is not valid: {e}")))?;

                // The document must claim the issuer we asked for. A discovery response
                // that names somebody else would point the token exchange and the key set
                // at another server while every later check still passed — this is the
                // one place that mix-up can be caught.
                if document.issuer.trim_end_matches('/') != self.config.issuer.trim_end_matches('/')
                {
                    return Err(OidcError::Provider(format!(
                        "discovery at {url} claims issuer `{}`",
                        document.issuer
                    )));
                }
                Ok(document)
            })
            .await
    }

    /// Where to send the browser, and what to remember while it is away.
    pub fn authorization_url(&self, discovery: &Discovery, flow: &FlowSecrets) -> String {
        let mut url = match url::Url::parse(&discovery.authorization_endpoint) {
            Ok(url) => url,
            // Unreachable in practice: discovery already parsed as JSON and the endpoint
            // came from the provider. Returning the raw string keeps this infallible
            // rather than making every caller handle an error that cannot happen.
            Err(_) => return discovery.authorization_endpoint.clone(),
        };
        url.query_pairs_mut()
            .clear()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", SCOPE)
            .append_pair("state", &flow.state)
            .append_pair("nonce", &flow.nonce)
            .append_pair("code_challenge", &code_challenge_s256(&flow.verifier))
            .append_pair("code_challenge_method", "S256");
        url.to_string()
    }

    async fn exchange_code(&self, code: &str, verifier: &str) -> Result<TokenResponse, OidcError> {
        let discovery = self.discovery().await?;

        // `client_secret_basic`: the secret goes in the `Authorization` header, not in the
        // form body, so it does not land in an access log that records POST parameters.
        let response = self
            .http
            .post(&discovery.token_endpoint)
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .map_err(|e| OidcError::Provider(format!("token endpoint unreachable: {e}")))?;

        // A 4xx here is the provider refusing the code — a replayed code, a verifier that
        // does not match the challenge, an expired grant. That is a refusal, not an
        // outage.
        if response.status().is_client_error() {
            return Err(rejected(format!(
                "the provider refused the code exchange ({})",
                response.status()
            )));
        }

        response
            .error_for_status()
            .map_err(|e| OidcError::Provider(format!("token endpoint failed: {e}")))?
            .json()
            .await
            .map_err(|e| OidcError::Provider(format!("token response is not valid: {e}")))
    }

    /// The provider's signing keys.
    ///
    /// Deliberately fetched on every sign-in rather than cached. Sign-ins are rare — one
    /// round trip each — and a cached key set is how a key rotation turns into "nobody can
    /// log in and the logs say the signature is bad".
    async fn fetch_jwks(&self, jwks_uri: &str) -> Result<JwkSet, OidcError> {
        self.http
            .get(jwks_uri)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| OidcError::Provider(format!("key set unreachable: {e}")))?
            .json()
            .await
            .map_err(|e| OidcError::Provider(format!("key set is not valid: {e}")))
    }

    async fn fetch_userinfo(
        &self,
        endpoint: &str,
        access_token: &str,
    ) -> Result<UserInfo, OidcError> {
        self.http
            .get(endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| OidcError::Provider(format!("userinfo unreachable: {e}")))?
            .json()
            .await
            .map_err(|e| OidcError::Provider(format!("userinfo is not valid: {e}")))
    }

    /// Turn verified claims into the identity the store will record.
    ///
    /// Authelia can be configured to put profile claims in the id token or to keep them at
    /// the userinfo endpoint, and which it does is not this code's business to assume. A
    /// missing `groups` claim therefore means "ask", not "they are in none" — the second
    /// reading would silently demote everybody to the public-only baseline.
    async fn resolve_identity(
        &self,
        claims: IdClaims,
        access_token: Option<&str>,
    ) -> Result<VerifiedIdentity, OidcError> {
        let mut username = claims.preferred_username;
        let mut display_name = claims.name;
        let mut email = claims.email;
        let mut groups = claims.groups;

        if username.is_none() || groups.is_none() {
            let discovery = self.discovery().await?;
            let endpoint = discovery.userinfo_endpoint.as_deref().ok_or_else(|| {
                rejected("the id token is incomplete and there is no userinfo endpoint to ask")
            })?;
            let access_token = access_token.ok_or_else(|| {
                rejected("the id token is incomplete and no access token came with it")
            })?;

            let info = self.fetch_userinfo(endpoint, access_token).await?;
            // Required by the specification, and it is the check that stops one person's
            // profile being grafted onto another person's token.
            if info.sub != claims.sub {
                return Err(rejected(
                    "userinfo describes a different subject than the id token",
                ));
            }
            username = username.or(info.preferred_username);
            display_name = display_name.or(info.name);
            email = email.or(info.email);
            groups = groups.or(info.groups);
        }

        // Fail closed rather than falling back to `sub`. A principal keyed by an opaque
        // identifier would be created once and then never matched again when the username
        // did arrive, quietly producing two accounts for one person.
        let username = username
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .ok_or_else(|| rejected("the provider supplied no preferred_username"))?;

        Ok(VerifiedIdentity {
            display_name: display_name.unwrap_or_else(|| username.clone()),
            username,
            email,
            groups: groups.unwrap_or_default(),
        })
    }
}

/// Which algorithm to verify with — taken from the KEY, never from the token's header.
///
/// This is the algorithm-confusion defence, and it is the reason this function exists at
/// all rather than the header's `alg` being passed to `Validation::new`. Given a verifier
/// that believes the header, an attacker takes the provider's *public* RSA key, uses it as
/// an HMAC secret, signs a token of their choosing with `alg: HS256`, and the verifier
/// checks it against a value that was never secret. A symmetric key in an id-token key set
/// has no legitimate use here, so it is refused outright.
fn algorithm_for(jwk: &Jwk) -> Result<Algorithm, OidcError> {
    if matches!(jwk.algorithm, AlgorithmParameters::OctetKey(_)) {
        return Err(rejected(
            "the provider published a symmetric key; an id token is never verified with one",
        ));
    }

    if let Some(declared) = jwk.common.key_algorithm {
        return Algorithm::try_from(declared)
            .map_err(|_| rejected(format!("unsupported key algorithm {declared:?}")));
    }

    // A key set that omits `alg` is legal. The per-key-type default is what every other
    // client uses, and it cannot open the symmetric hole because that was refused above.
    Ok(match jwk.algorithm {
        AlgorithmParameters::RSA(_) => Algorithm::RS256,
        AlgorithmParameters::EllipticCurve(_) => Algorithm::ES256,
        AlgorithmParameters::OctetKeyPair(_) => Algorithm::EdDSA,
        _ => return Err(rejected("the provider published a key of an unknown type")),
    })
}

/// Verify an id token against a key set. No I/O, so every refusal below is a unit test.
pub fn verify_id_token(
    token: &str,
    jwks: &JwkSet,
    issuer: &str,
    client_id: &str,
    expected_nonce: &str,
) -> Result<IdClaims, OidcError> {
    let header =
        decode_header(token).map_err(|e| rejected(format!("unreadable id token header: {e}")))?;
    let kid = header
        .kid
        .ok_or_else(|| rejected("the id token names no signing key"))?;
    let jwk = jwks
        .find(&kid)
        .ok_or_else(|| rejected(format!("the provider does not publish key `{kid}`")))?;

    let key = DecodingKey::from_jwk(jwk).map_err(|e| rejected(format!("unusable key: {e}")))?;
    let mut validation = Validation::new(algorithm_for(jwk)?);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[client_id]);
    validation.validate_exp = true;
    // Spelled out rather than left to the default, which requires only `exp`. An id token
    // with no `iss` or no `aud` would otherwise pass the claim check by having nothing to
    // check.
    validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);

    let data = decode::<IdClaims>(token, &key, &validation)
        .map_err(|e| rejected(format!("id token rejected: {e}")))?;

    match data.claims.nonce.as_deref() {
        Some(nonce) if constant_time_eq(nonce, expected_nonce) => {}
        Some(_) => {
            return Err(rejected(
                "the id token's nonce is not the one this browser was issued",
            ))
        }
        None => return Err(rejected("the id token carries no nonce")),
    }

    Ok(data.claims)
}

// -------------------------------------------------------------------------------------
// Handlers.
// -------------------------------------------------------------------------------------

/// Begin a sign-in: mint the three secrets, remember them in cookies, send the browser on.
///
/// The secrets live in `HttpOnly` cookies rather than in a server-side table because they
/// are meaningful only to the browser that was issued them — which is exactly the property
/// `state` is there to establish. A shared table keyed by `state` would have to be swept,
/// and would let one browser's flow be completed by another if the key ever leaked.
pub async fn login(State(state): State<AppState>) -> Result<Response, ApiError> {
    let client = state.oidc.as_ref().ok_or(ApiError::Unavailable)?;
    let discovery = client.discovery().await.map_err(ApiError::from)?;

    let flow = FlowSecrets::generate();
    let target = client.authorization_url(discovery, &flow);

    let jar = CookieJar::new()
        .add(flow_cookie(STATE_COOKIE, flow.state, FLOW_TTL_SECONDS))
        .add(flow_cookie(NONCE_COOKIE, flow.nonce, FLOW_TTL_SECONDS))
        .add(flow_cookie(
            VERIFIER_COOKIE,
            flow.verifier,
            FLOW_TTL_SECONDS,
        ));

    Ok((jar, found(&target)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Finish a sign-in.
///
/// Returns a `Response` rather than a `Result` so that the flow cookies are cleared on
/// **every** outcome, refusal included: a state that has been presented once must not be
/// available to present again.
pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(client) = state.oidc.clone() else {
        return ApiError::Unavailable.into_response();
    };

    let spent = jar
        .clone()
        .remove(cleared_cookie(STATE_COOKIE))
        .remove(cleared_cookie(NONCE_COOKIE))
        .remove(cleared_cookie(VERIFIER_COOKIE));

    match complete_sign_in(&state, &client, &jar, &query).await {
        Ok(token) => (spent.add(session_cookie(token)), found("/")).into_response(),
        Err(error) => {
            // Logged in full, returned as nothing: which check refused a sign-in is an
            // operator's business, and an oracle for anyone else.
            tracing::warn!(%error, "sign-in refused");
            (spent, ApiError::from(error)).into_response()
        }
    }
}

async fn complete_sign_in(
    state: &AppState,
    client: &OidcClient,
    jar: &CookieJar,
    query: &CallbackQuery,
) -> Result<String, OidcError> {
    if let Some(error) = &query.error {
        return Err(rejected(format!("the provider returned `{error}`")));
    }

    // The state check first, and against a cookie rather than anything in the URL. An
    // absent cookie is a refusal, not a reason to skip the check: "no state to compare"
    // is precisely the situation an attacker arranges.
    let expected_state = flow_value(jar, STATE_COOKIE)?;
    let presented_state = query
        .state
        .as_deref()
        .ok_or_else(|| rejected("the callback carried no `state`"))?;
    if !constant_time_eq(&expected_state, presented_state) {
        return Err(rejected(
            "the `state` does not match the one this browser was issued",
        ));
    }

    let nonce = flow_value(jar, NONCE_COOKIE)?;
    let verifier = flow_value(jar, VERIFIER_COOKIE)?;
    let code = query
        .code
        .as_deref()
        .ok_or_else(|| rejected("the callback carried no authorization code"))?;

    let tokens = client.exchange_code(code, &verifier).await?;
    let discovery = client.discovery().await?;
    let jwks = client.fetch_jwks(&discovery.jwks_uri).await?;
    let claims = verify_id_token(
        &tokens.id_token,
        &jwks,
        &client.config.issuer,
        &client.config.client_id,
        &nonce,
    )?;
    let identity = client
        .resolve_identity(claims, tokens.access_token.as_deref())
        .await?;

    // Groups are REPLACED here, never merged — `upsert_oidc_principal` does that, and it
    // is why losing a group in Authelia takes effect at the next sign-in.
    let principal = state
        .store
        .upsert_oidc_principal(
            &identity.username,
            &identity.display_name,
            identity.email.as_deref(),
            &identity.groups,
        )
        .await
        .map_err(OidcError::Internal)?;

    let token = new_session_token();
    state
        .store
        .create_session(
            &principal.id,
            &hash_token(&token),
            gw_store::SESSION_TTL_SECONDS,
        )
        .await
        .map_err(OidcError::Internal)?;

    tracing::info!(
        username = %identity.username,
        groups = ?identity.groups,
        "sign-in completed"
    );
    Ok(token)
}

fn flow_value(jar: &CookieJar, name: &str) -> Result<String, OidcError> {
    jar.get(name)
        .map(|cookie| cookie.value().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| rejected(format!("this browser presented no `{name}`")))
}

#[cfg(test)]
mod tests {
    use super::{algorithm_for, code_challenge_s256, random_secret, OidcConfig};
    use jsonwebtoken::jwk::{
        AlgorithmParameters, CommonParameters, Jwk, KeyAlgorithm, OctetKeyParameters, OctetKeyType,
        RSAKeyParameters, RSAKeyType,
    };
    use jsonwebtoken::Algorithm;
    use std::collections::HashSet;

    #[test]
    fn the_code_challenge_matches_the_published_rfc_7636_vector() {
        // Appendix B of RFC 7636. A test vector rather than a round trip: computing the
        // challenge with the same code that verifies it would agree with itself while
        // both were wrong, and the provider is the one that has to agree.
        assert_eq!(
            code_challenge_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_verifier_is_a_legal_length_and_alphabet() {
        let verifier = random_secret();
        assert!(
            (43..=128).contains(&verifier.len()),
            "RFC 7636 requires 43 to 128 characters, got {}",
            verifier.len()
        );
        assert!(
            verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)),
            "the unreserved set only: {verifier}"
        );
    }

    #[test]
    fn flow_secrets_do_not_repeat() {
        let secrets: HashSet<String> = (0..1000).map(|_| random_secret()).collect();
        assert_eq!(secrets.len(), 1000);
    }

    fn rsa_key(algorithm: Option<KeyAlgorithm>) -> Jwk {
        Jwk {
            common: CommonParameters {
                key_algorithm: algorithm,
                ..Default::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                key_type: RSAKeyType::RSA,
                n: "not-a-real-modulus".into(),
                e: "AQAB".into(),
            }),
        }
    }

    #[test]
    fn a_symmetric_key_can_never_verify_an_id_token() {
        // The algorithm-confusion attack in one assertion: if this key were accepted, a
        // token signed with the provider's public key as an HMAC secret would verify.
        let hmac = Jwk {
            common: CommonParameters {
                key_algorithm: Some(KeyAlgorithm::HS256),
                ..Default::default()
            },
            algorithm: AlgorithmParameters::OctetKey(OctetKeyParameters {
                key_type: OctetKeyType::Octet,
                value: "c2VjcmV0".into(),
            }),
        };
        assert!(algorithm_for(&hmac).is_err());
    }

    #[test]
    fn the_algorithm_comes_from_the_key_that_declares_it() {
        assert_eq!(
            algorithm_for(&rsa_key(Some(KeyAlgorithm::RS256))).unwrap(),
            Algorithm::RS256
        );
        assert_eq!(
            algorithm_for(&rsa_key(Some(KeyAlgorithm::PS512))).unwrap(),
            Algorithm::PS512
        );
    }

    #[test]
    fn a_key_that_declares_no_algorithm_falls_back_by_key_type() {
        assert_eq!(algorithm_for(&rsa_key(None)).unwrap(), Algorithm::RS256);
    }

    #[test]
    fn the_client_secret_is_not_in_the_debug_rendering() {
        let config = OidcConfig {
            issuer: "https://auth.example.invalid".into(),
            client_id: "great-wiki".into(),
            client_secret: "nicht-das-echte-geheimnis".into(),
            redirect_uri: "https://wiki.example.invalid/auth/callback".into(),
        };
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("nicht-das-echte-geheimnis"),
            "{rendered}"
        );
        assert!(rendered.contains("great-wiki"));
    }
}
