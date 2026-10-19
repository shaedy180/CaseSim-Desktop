//! Browser consent and native-only PKCE. Never read cookies or expose a token to JS.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::header::HeaderValue;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const AUTHORIZE: &str = "https://auth.shaedy.de/auth/desktop/authorize";
const TOKEN: &str = "https://auth.shaedy.de/auth/desktop/token";
const SCOPE: &str = "desktop-releases:read";
const WAIT: Duration = Duration::from_secs(300);
const FAILED: &str =
    "Update sign-in failed. Start a new check and allow downloads in your browser.";

pub struct Authorization {
    pub header: HeaderValue,
    pub expires_at: Instant,
}

struct Flow {
    listener: TcpListener,
    redirect: String,
    state: String,
    verifier: String,
}

fn random() -> Result<String, &'static str> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| FAILED)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn opaque(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

impl Flow {
    fn new() -> Result<Self, &'static str> {
        // Bind before opening the browser; never listen on a public interface.
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| FAILED)?;
        listener.set_nonblocking(true).map_err(|_| FAILED)?;
        let address = listener.local_addr().map_err(|_| FAILED)?;
        Ok(Self {
            listener,
            redirect: format!("http://{address}/desktop/callback"),
            state: random()?,
            verifier: random()?,
        })
    }

    fn browser_url(&self) -> tauri::Url {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()));
        let mut url = tauri::Url::parse(AUTHORIZE).expect("fixed authorization origin");
        url.query_pairs_mut().extend_pairs([
            ("client_id", "casesim-desktop"),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("redirect_uri", self.redirect.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", self.state.as_str()),
        ]);
        url
    }

    fn callback(&self, request: &str) -> Option<String> {
        let mut lines = request.split("\r\n");
        let parts: Vec<_> = lines.next()?.split(' ').collect();
        if parts.len() != 3 || parts[0] != "GET" || parts[2] != "HTTP/1.1" {
            return None;
        }
        // Reject absolute request targets, duplicate Host/query keys and rebinding.
        if !parts[1].starts_with("/desktop/callback?") || parts[1].contains('#') {
            return None;
        }
        let host = self.listener.local_addr().ok()?.to_string();
        let hosts: Vec<_> = lines
            .filter_map(|line| line.split_once(':'))
            .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
            .collect();
        if hosts.len() != 1 || hosts[0].1.trim() != host {
            return None;
        }
        let url = tauri::Url::parse(&format!("http://{host}{}", parts[1])).ok()?;
        if url.path() != "/desktop/callback" {
            return None;
        }
        let pairs: Vec<_> = url.query_pairs().collect();
        if pairs.len() != 2 {
            return None;
        }
        let state = pairs.iter().find(|(key, _)| key == "state")?.1.as_ref();
        let code = pairs.iter().find(|(key, _)| key == "code")?.1.as_ref();
        if state != self.state || !opaque(code) {
            return None;
        }
        Some(code.to_owned())
    }

    fn wait(self, timeout: Duration) -> Result<(Self, String), &'static str> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.listener.accept() {
                Ok((mut stream, address)) => {
                    if !address.ip().is_loopback() {
                        continue;
                    }
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .map_err(|_| FAILED)?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .map_err(|_| FAILED)?;
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while request.len() < 8192 && Instant::now() < deadline {
                        match stream.read(&mut buffer) {
                            Ok(0) | Err(_) => break,
                            Ok(count) => request.extend_from_slice(&buffer[..count]),
                        }
                        if request.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let code = if request.len() < 8192 && request.ends_with(b"\r\n\r\n") {
                        std::str::from_utf8(&request)
                            .ok()
                            .and_then(|text| self.callback(text))
                    } else {
                        None
                    };
                    let (status, body) = if code.is_some() {
                        (
                            "200 OK",
                            "Return to CaseSim Desktop. You can close this tab.",
                        )
                    } else {
                        (
                            "400 Bad Request",
                            "Invalid update callback. Return to CaseSim Desktop.",
                        )
                    };
                    // Fixed plain text only: never reflect codes, URLs or account information.
                    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}", body.len());
                    if let Some(code) = code {
                        return Ok((self, code));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(_) => return Err(FAILED),
            }
        }
        Err("Update sign-in timed out. Check again when you are ready to sign in.")
    }
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    token_type: String,
    expires_in: u64,
    scope: String,
}

impl Token {
    fn authorization(self, started: Instant) -> Result<Authorization, &'static str> {
        if self.token_type != "Bearer"
            || self.scope != SCOPE
            || !(6..=600).contains(&self.expires_in)
            || self.access_token.len() > 4096
            || self.access_token.is_empty()
        {
            return Err(FAILED);
        }
        let mut header =
            HeaderValue::from_str(&format!("Bearer {}", self.access_token)).map_err(|_| FAILED)?;
        header.set_sensitive(true);
        Ok(Authorization {
            header,
            expires_at: started + Duration::from_secs(self.expires_in - 5),
        })
    }
}

pub async fn authorize() -> Result<Authorization, &'static str> {
    let flow = Flow::new()?;
    open::that_detached(flow.browser_url().as_str()).map_err(|_| FAILED)?;
    let (flow, code) = tauri::async_runtime::spawn_blocking(move || flow.wait(WAIT))
        .await
        .map_err(|_| FAILED)??;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| FAILED)?;
    let started = Instant::now();
    let mut response = client
        .post(TOKEN)
        .json(&serde_json::json!({
            "client_id": "casesim-desktop", "grant_type": "authorization_code",
            "redirect_uri": flow.redirect, "code": code, "code_verifier": flow.verifier,
        }))
        .send()
        .await
        .map_err(|_| FAILED)?;
    if !response.status().is_success() {
        return Err(FAILED);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| FAILED)? {
        if bytes.len() + chunk.len() > 8192 {
            return Err(FAILED);
        }
        bytes.extend_from_slice(&chunk);
    }
    let token: Token = serde_json::from_slice(&bytes).map_err(|_| FAILED)?;
    token.authorization(started)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_roundtrip_does_not_reflect_the_code() {
        let flow = Flow::new().unwrap();
        let host = flow.listener.local_addr().unwrap();
        let request = format!(
            "GET /desktop/callback?code={}&state={} HTTP/1.1\r\nHost: {host}\r\n\r\n",
            "c".repeat(43),
            flow.state
        );
        let server =
            std::thread::spawn(move || flow.wait(Duration::from_secs(3)).map(|(_, code)| code));
        let mut client = std::net::TcpStream::connect(host).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        client.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("Cache-Control: no-store"));
        assert!(!response.contains(&"c".repeat(43)));
        assert_eq!(server.join().unwrap().unwrap(), "c".repeat(43));
    }
    #[test]
    fn pkce_uses_registered_origin_randomness_and_s256() {
        let first = Flow::new().unwrap();
        let second = Flow::new().unwrap();
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.state, second.state);
        assert!(opaque(&first.verifier) && opaque(&first.state));
        let url = first.browser_url();
        assert_eq!(url.origin().ascii_serialization(), "https://auth.shaedy.de");
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(pairs["redirect_uri"], first.redirect);
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(
            pairs["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(first.verifier.as_bytes()))
        );
        assert!(!url.as_str().contains(&first.verifier));
    }
    #[test]
    fn callback_rejects_spoofing_duplicates_wrong_state_and_paths() {
        let flow = Flow::new().unwrap();
        let host = flow.listener.local_addr().unwrap().to_string();
        let query = format!(
            "/desktop/callback?code={}&state={}",
            "c".repeat(43),
            flow.state
        );
        let good = format!("GET {query} HTTP/1.1\r\nHost: {host}\r\n\r\n");
        assert_eq!(flow.callback(&good), Some("c".repeat(43)));
        for bad in [
            good.replace(&host, "evil.test"),
            good.replace("GET ", "POST "),
            good.replace(&flow.state, &"x".repeat(43)),
            good.replace("callback?", "other?"),
            good.replace(" HTTP/1.1", "&code=duplicate HTTP/1.1"),
            good.replace("\r\n\r\n", &format!("\r\nHost: {host}\r\n\r\n")),
        ] {
            assert!(flow.callback(&bad).is_none());
        }
        assert!(Flow::new().unwrap().wait(Duration::ZERO).is_err());
    }
    #[test]
    fn token_is_bounded_scoped_sensitive_and_short_lived() {
        let started = Instant::now();
        let good = || Token {
            access_token: "fixture".into(),
            token_type: "Bearer".into(),
            expires_in: 600,
            scope: SCOPE.into(),
        };
        let grant = good().authorization(started).unwrap();
        assert!(grant.header.is_sensitive());
        assert_eq!(
            grant.expires_at.duration_since(started),
            Duration::from_secs(595)
        );
        let mut bad = good();
        bad.expires_in = 601;
        assert!(bad.authorization(started).is_err());
        let mut bad = good();
        bad.scope = "admin".into();
        assert!(bad.authorization(started).is_err());
        let mut bad = good();
        bad.access_token = "evil\r\nHeader: injected".into();
        assert!(bad.authorization(started).is_err());
    }
}
