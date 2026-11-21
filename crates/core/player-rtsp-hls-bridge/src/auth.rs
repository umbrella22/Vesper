use base64::{Engine, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};
use percent_encoding::percent_decode_str;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::fmt::Write;
use url::Url;

use crate::BridgeError as Error;

pub(crate) struct Credentials {
    user: String,
    password: String,
}

impl Credentials {
    pub fn take(url: &mut Url) -> Result<Option<Self>, Error> {
        if url.username().is_empty() && url.password().is_none() {
            return Ok(None);
        }
        let decode = |value: &str| {
            percent_decode_str(value)
                .decode_utf8()
                .map(|value| value.into_owned())
                .map_err(|_| Error::InvalidUrl)
        };
        let user = decode(url.username())?;
        let password = decode(url.password().unwrap_or_default())?;
        if user.is_empty()
            || user.contains(':')
            || user.chars().chain(password.chars()).any(char::is_control)
        {
            return Err(Error::InvalidUrl);
        }
        url.set_password(None).map_err(|_| Error::InvalidUrl)?;
        url.set_username("").map_err(|_| Error::InvalidUrl)?;
        Ok(Some(Self { user, password }))
    }
}

#[derive(Clone, Copy)]
enum Algorithm {
    Md5,
    Sha256,
}

impl Algorithm {
    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha256 => "SHA-256",
        }
    }

    fn hash(self, input: &str) -> String {
        match self {
            Self::Md5 => hex::encode(Md5::digest(input.as_bytes())),
            Self::Sha256 => hex::encode(Sha256::digest(input.as_bytes())),
        }
    }
}

pub(crate) enum Challenge {
    Basic,
    Digest(DigestChallenge),
}

pub(crate) struct DigestChallenge {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    algorithm: Algorithm,
    qop_auth: bool,
    pub stale: bool,
    count: u32,
}

impl Challenge {
    pub fn select<'a>(headers: impl Iterator<Item = &'a str>) -> Option<Self> {
        headers
            .filter_map(Self::parse)
            .max_by_key(|item| match item {
                Self::Basic => 0,
                Self::Digest(state) => match state.algorithm {
                    Algorithm::Md5 => 1,
                    Algorithm::Sha256 => 2,
                },
            })
    }

    fn parse(header: &str) -> Option<Self> {
        let (scheme, params) = header.split_once(char::is_whitespace)?;
        if scheme.eq_ignore_ascii_case("Basic") {
            return Some(Self::Basic);
        }
        if !scheme.eq_ignore_ascii_case("Digest") {
            return None;
        }
        let mut params = parse_parameters(params)?;
        let algorithm = match params.remove("algorithm").as_deref() {
            None => Algorithm::Md5,
            Some(value) if value.eq_ignore_ascii_case("MD5") => Algorithm::Md5,
            Some(value) if value.eq_ignore_ascii_case("SHA-256") => Algorithm::Sha256,
            _ => return None,
        };
        let qop_auth = match params.remove("qop") {
            None => false,
            Some(qop)
                if qop
                    .split(',')
                    .any(|value| value.trim().eq_ignore_ascii_case("auth")) =>
            {
                true
            }
            Some(_) => return None,
        };
        if params
            .get("userhash")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        {
            return None;
        }
        Some(Self::Digest(DigestChallenge {
            realm: params.remove("realm")?,
            nonce: params.remove("nonce").filter(|nonce| !nonce.is_empty())?,
            opaque: params.remove("opaque"),
            algorithm,
            qop_auth,
            stale: params
                .get("stale")
                .is_some_and(|value| value.eq_ignore_ascii_case("true")),
            count: 0,
        }))
    }

    pub fn stale(&self) -> bool {
        matches!(self, Self::Digest(value) if value.stale)
    }

    pub fn authorization(
        &mut self,
        credentials: &Credentials,
        method: &str,
        uri: &Url,
    ) -> Result<String, Error> {
        match self {
            Self::Basic => Ok(format!(
                "Basic {}",
                STANDARD.encode(format!("{}:{}", credentials.user, credentials.password))
            )),
            Self::Digest(digest) => digest.authorization(credentials, method, uri.as_str()),
        }
    }
}

impl DigestChallenge {
    fn authorization(
        &mut self,
        credentials: &Credentials,
        method: &str,
        uri: &str,
    ) -> Result<String, Error> {
        let algorithm = self.algorithm;
        let a1 = algorithm.hash(&format!(
            "{}:{}:{}",
            credentials.user, self.realm, credentials.password
        ));
        let a2 = algorithm.hash(&format!("{method}:{uri}"));
        let mut parameters = String::new();
        let response = if self.qop_auth {
            self.count = self
                .count
                .checked_add(1)
                .ok_or(Error::AuthenticationRequired)?;
            let nc = format!("{:08x}", self.count);
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|_| Error::AuthenticationRequired)?;
            let mut cnonce = String::with_capacity(32);
            for byte in bytes {
                let _ = write!(cnonce, "{byte:02x}");
            }
            parameters = format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\"");
            algorithm.hash(&format!("{a1}:{}:{nc}:{cnonce}:auth:{a2}", self.nonce))
        } else {
            algorithm.hash(&format!("{a1}:{}:{a2}", self.nonce))
        };
        if let Some(opaque) = &self.opaque {
            let _ = write!(parameters, ", opaque={}", quoted(opaque));
        }
        Ok(format!(
            "Digest username={}, realm={}, nonce={}, uri={}, response=\"{response}\", algorithm={}{parameters}",
            quoted(&credentials.user),
            quoted(&self.realm),
            quoted(&self.nonce),
            quoted(uri),
            algorithm.name()
        ))
    }
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn parse_parameters(mut input: &str) -> Option<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    while !input.trim().is_empty() {
        if values.len() >= 32 {
            return None;
        }
        let (key, rest) = input.split_once('=')?;
        let name = key.trim().to_ascii_lowercase();
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return None;
        }
        let rest = rest.trim_start();
        let (value, rest) = if let Some(text) = rest.strip_prefix('"') {
            let mut value = String::new();
            let mut escaped = false;
            let mut end = None;
            for (index, ch) in text.char_indices() {
                if ch.is_control() {
                    return None;
                }
                if escaped {
                    value.push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    end = Some(index + 1);
                    break;
                } else {
                    value.push(ch);
                }
            }
            (value, &text[end?..])
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let value = rest[..end].trim();
            if value.bytes().any(|byte| !byte.is_ascii_graphic()) {
                return None;
            }
            (value.to_owned(), &rest[end..])
        };
        if values.insert(name, value).is_some() {
            return None;
        }
        let rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        input = rest.strip_prefix(',')?;
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_removed_and_sha256_is_preferred() {
        let mut uri =
            Url::parse("rtsp://camera:pa%40ss@127.0.0.1:8554/clock").expect("fixture URI");
        let credentials = Credentials::take(&mut uri)
            .expect("valid credentials")
            .expect("credentials");
        assert_eq!(uri.as_str(), "rtsp://127.0.0.1:8554/clock");
        let mut challenge = Challenge::select(
            [
                "Basic realm=\"Clock\"",
                "Digest realm=\"Clock\", nonce=\"nonce-1\"",
                "Digest realm=\"Clock\", nonce=\"nonce-1\", algorithm=SHA-256",
            ]
            .into_iter(),
        )
        .expect("supported challenge");
        let value = challenge
            .authorization(&credentials, "DESCRIBE", &uri)
            .expect("answer");
        assert!(value.contains(
            "response=\"009fc9986498a2e5388817369adc3d8cc19e22b66b1ee0f2c542a439cd85a081\""
        ));
        assert!(!value.contains("pa@ss"));
        let mut basic =
            Challenge::select(["Basic realm=\"Clock\""].into_iter()).expect("Basic challenge");
        assert_eq!(
            basic
                .authorization(&credentials, "DESCRIBE", &uri)
                .expect("Basic answer"),
            "Basic Y2FtZXJhOnBhQHNz"
        );
    }

    #[test]
    fn digest_qop_auth_preserves_the_uri_and_advances_nonce_count() {
        let uri = Url::parse("rtsp://127.0.0.1:8554/clock").expect("URI");
        let credentials = Credentials {
            user: "camera".into(),
            password: "pa@ss".into(),
        };
        let mut challenge = Challenge::select([
            r#"Digest realm="Clock", nonce="nonce-1", qop="auth-int,auth", opaque="some,opaque\"value""#,
        ].into_iter()).expect("Digest challenge");
        for nonce_count in ["00000001", "00000002"] {
            let value = challenge
                .authorization(&credentials, "PLAY", &uri)
                .expect("answer");
            let fields = parse_parameters(value.strip_prefix("Digest ").expect("Digest"))
                .expect("parameters");
            assert_eq!(fields["qop"], "auth");
            assert_eq!(fields["nc"], nonce_count);
            assert_eq!(fields["opaque"], "some,opaque\"value");
            assert_eq!(fields["uri"], uri.as_str());
            assert_eq!(fields["cnonce"].len(), 32);
            let ha1 = Algorithm::Md5.hash("camera:Clock:pa@ss");
            let ha2 = Algorithm::Md5.hash("PLAY:rtsp://127.0.0.1:8554/clock");
            assert_eq!(
                fields["response"],
                Algorithm::Md5.hash(&format!(
                    "{ha1}:nonce-1:{nonce_count}:{}:auth:{ha2}",
                    fields["cnonce"]
                ))
            );
        }
    }
}
