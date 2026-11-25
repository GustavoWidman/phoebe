//! sessions: a token any node checks alone, bound to a key pair the browser
//! made and cannot export. the token is the session key's MAC over who
//! signed in, on which device, with which public key, until when; every
//! request also carries a fresh signature by the private key over its
//! method, path and time, so a copied token is useless without the browser
//! that holds the key.
//!
//! token: `v1.<b64url(json(claims))>.<b64url(hmac_sha256(session key, the
//! first two parts))>`. proof: `Phoebe-Proof: <ms>.<b64url(r || s)>`, signed
//! over `phoebe-req-v1\n<METHOD>\n<path?query>\n<ms>\n<b64url(sha256(token))>`.
//! a captured proof replays only within [WINDOW], on the same path and
//! method, which every node can check without sharing nonces.

use std::{fmt, path::Path, str::FromStr, time::Duration};

use axum::http::{HeaderMap, Method, header};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD as B64};
use eyre::{Result, WrapErr, bail, eyre};
use hmac::{Hmac, KeyInit, Mac};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use super::wire::{Jwk, Ms};

/// how far a signed moment may be from the server's clock, either way
pub const WINDOW: Duration = Duration::from_secs(120);
/// how long a session lasts from signing in
pub const LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// how a browser names its session in `Authorization`
const SCHEME: &str = "Phoebe ";
pub const PROOF: &str = "phoebe-proof";
const VERSION: &str = "v1";
const REQUEST: &str = "phoebe-req-v1";
const SIGNIN: &str = "phoebe-signin-v1";

/// a session key: 32 bytes, written as 64 hex digits
#[derive(Clone, PartialEq, Eq)]
pub struct Key([u8; 32]);

/// the fleet's session keys. the first signs new sessions, and any of them
/// checks one, so a new key goes first and the old one stays until the
/// sessions it signed have expired; dropping a key ends them at once
pub struct Sessions {
    keys: Vec<(Kid, Key)>,
    lifetime: Duration,
}

/// the first bytes of a session key's sha256, naming the key a token was
/// signed with
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kid([u8; 4]);

/// what a token says, under the session key's MAC
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionClaims {
    pub v: u8,
    pub kid: Kid,
    /// the tailnet login that signed in
    pub sub: String,
    /// the tailnet node it signed in from
    pub device: String,
    /// the key every request must be signed with
    pub key: DeviceKey,
    /// when it signed in, always on the tailnet
    pub iat: Ms,
    pub exp: Ms,
}

/// a browser's P-256 public key, written as its uncompressed SEC1 point
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceKey(VerifyingKey);

/// a request's `Phoebe-Proof`: when the browser signed it, and the signature
pub struct Proof {
    pub ts: Ms,
    pub sig: Signature,
}

/// a session [Sessions::issue] made
pub struct Session {
    pub token: String,
    pub claims: SessionClaims,
}

/// why a request is not let in. every one answers the same 401
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// no session, or no proof
    Missing,
    /// a token or proof that does not parse
    Malformed,
    /// signed with a key this node does not hold
    UnknownKey,
    /// the MAC does not match what the token says
    Forged,
    Expired,
    /// signed too far from now
    Stale,
    /// the signature is not the session key's, or not over this request
    Unproven,
}

impl Key {
    pub fn generate() -> Self {
        let mut bytes = [0; 32];
        getrandom::fill(&mut bytes).expect("the system has randomness");
        Self(bytes)
    }

    #[cfg(test)]
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

impl FromStr for Key {
    type Err = eyre::Report;

    fn from_str(text: &str) -> Result<Self> {
        if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("a session key is 64 hex digits");
        }
        let mut bytes = [0; 32];
        for (at, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[at * 2..at * 2 + 2], 16)?;
        }
        Ok(Self(bytes))
    }
}

impl Sessions {
    pub fn new(keys: Vec<Key>) -> Result<Self> {
        if keys.is_empty() {
            bail!("no session key");
        }
        Ok(Self {
            keys: keys.into_iter().map(|key| (Kid::of(&key), key)).collect(),
            lifetime: LIFETIME,
        })
    }

    /// the keys in `path`, 64 hex digits each, one per line, the signing
    /// one first
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        let keys = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(Key::from_str)
            .collect::<Result<Vec<_>>>()
            .wrap_err_with(|| format!("invalid session key in {}", path.display()))?;
        Self::new(keys)
    }

    /// a session for `sub` on `device`, bound to `key`, from `now`
    pub fn issue(&self, sub: String, device: String, key: DeviceKey, now: Ms) -> Session {
        let (kid, signer) = &self.keys[0];
        let claims = SessionClaims {
            v: 1,
            kid: *kid,
            sub,
            device,
            key,
            iat: now,
            exp: Ms(now.0 + self.lifetime.as_millis() as i64),
        };
        let json = serde_json::to_vec(&claims).expect("claims always serialize");
        let signed = format!("{VERSION}.{}", B64.encode(json));
        let mac = mac(signer, signed.as_bytes()).finalize().into_bytes();
        Session {
            token: format!("{signed}.{}", B64.encode(mac)),
            claims,
        }
    }

    /// what `token` says, if one of these keys signed it and it has not
    /// expired at `now`
    pub fn check(&self, token: &str, now: Ms) -> Result<SessionClaims, Refused> {
        let (signed, tag) = token.rsplit_once('.').ok_or(Refused::Malformed)?;
        let (version, claims) = signed.split_once('.').ok_or(Refused::Malformed)?;
        if version != VERSION {
            return Err(Refused::Malformed);
        }
        let tag = B64.decode(tag).map_err(|_| Refused::Malformed)?;
        let claims = B64.decode(claims).map_err(|_| Refused::Malformed)?;
        let claims: SessionClaims =
            serde_json::from_slice(&claims).map_err(|_| Refused::Malformed)?;

        let (_, key) = self
            .keys
            .iter()
            .find(|(kid, _)| *kid == claims.kid)
            .ok_or(Refused::UnknownKey)?;
        mac(key, signed.as_bytes())
            .verify_slice(&tag)
            .map_err(|_| Refused::Forged)?;
        if claims.v != 1 {
            return Err(Refused::Malformed);
        }
        if now >= claims.exp {
            return Err(Refused::Expired);
        }
        Ok(claims)
    }

    /// the session a request to `path_and_query` carries, if its token
    /// checks and its proof is the session key's signature over this request,
    /// made within [WINDOW] of `now`
    pub fn authenticate(
        &self,
        method: &Method,
        path_and_query: &str,
        headers: &HeaderMap,
        now: Ms,
    ) -> Result<SessionClaims, Refused> {
        let token = headers
            .get(header::AUTHORIZATION)
            .ok_or(Refused::Missing)?
            .to_str()
            .map_err(|_| Refused::Malformed)?
            .strip_prefix(SCHEME)
            .ok_or(Refused::Malformed)?;
        let proof: Proof = headers
            .get(PROOF)
            .ok_or(Refused::Missing)?
            .to_str()
            .map_err(|_| Refused::Malformed)?
            .parse()
            .map_err(|_| Refused::Malformed)?;

        let claims = self.check(token, now)?;
        fresh(proof.ts, now)?;
        let message = Proof::message(method, path_and_query, proof.ts, token);
        match claims.key.verifies(&message, &proof.sig) {
            true => Ok(claims),
            false => Err(Refused::Unproven),
        }
    }
}

/// whether a moment a browser signed is within [WINDOW] of `now`
pub fn fresh(ts: Ms, now: Ms) -> Result<(), Refused> {
    match ts.0.abs_diff(now.0) <= WINDOW.as_millis() as u64 {
        true => Ok(()),
        false => Err(Refused::Stale),
    }
}

fn mac(key: &Key, signed: &[u8]) -> Hmac<Sha256> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&key.0).expect("hmac takes any key");
    mac.update(signed);
    mac
}

impl Kid {
    fn of(key: &Key) -> Self {
        let digest = Sha256::digest(key.0);
        Self([digest[0], digest[1], digest[2], digest[3]])
    }
}

impl Serialize for Kid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for Kid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let invalid = || serde::de::Error::custom("a kid is 8 hex digits");
        if text.len() != 8 {
            return Err(invalid());
        }
        let mut kid = [0; 4];
        for (at, byte) in kid.iter_mut().enumerate() {
            *byte = u8::from_str_radix(text.get(at * 2..at * 2 + 2).ok_or_else(invalid)?, 16)
                .map_err(|_| invalid())?;
        }
        Ok(Self(kid))
    }
}

impl DeviceKey {
    /// a public key as SEC1 bytes, refused unless a point on P-256
    pub fn from_sec1(bytes: &[u8]) -> Result<Self> {
        VerifyingKey::from_sec1_bytes(bytes)
            .map(Self)
            .map_err(|_| eyre!("not a P-256 public key"))
    }

    /// a public key as WebCrypto exports it: `{ kty: "EC", crv: "P-256", x, y }`
    pub fn from_jwk(jwk: &Jwk) -> Result<Self> {
        if jwk.kty != "EC" || jwk.crv != "P-256" {
            bail!("not a P-256 key");
        }
        let x = B64.decode(&jwk.x).wrap_err("x is not base64url")?;
        let y = B64.decode(&jwk.y).wrap_err("y is not base64url")?;
        if x.len() != 32 || y.len() != 32 {
            bail!("a P-256 coordinate is 32 bytes");
        }
        let mut point = vec![4];
        point.extend(x);
        point.extend(y);
        Self::from_sec1(&point)
    }

    /// whether `sig` is this key's signature over `message`
    pub fn verifies(&self, message: &[u8], sig: &Signature) -> bool {
        self.0.verify(message, sig).is_ok()
    }
}

impl Serialize for DeviceKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&B64.encode(self.0.to_sec1_bytes()))
    }
}

impl<'de> Deserialize<'de> for DeviceKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let bytes = B64.decode(text).map_err(serde::de::Error::custom)?;
        Self::from_sec1(&bytes).map_err(serde::de::Error::custom)
    }
}

impl Proof {
    /// what a browser signs for a request
    pub fn message(method: &Method, path_and_query: &str, ts: Ms, token: &str) -> Vec<u8> {
        let token = B64.encode(Sha256::digest(token.as_bytes()));
        format!("{REQUEST}\n{method}\n{path_and_query}\n{}\n{token}", ts.0).into_bytes()
    }

    /// what a browser signs to sign in to the app at `origin`
    pub fn signin(origin: &str, ts: Ms) -> Vec<u8> {
        format!("{SIGNIN}\n{origin}\n{}", ts.0).into_bytes()
    }
}

/// a signature as WebCrypto writes it, `r || s`, in base64url
pub fn signature(text: &str) -> Result<Signature> {
    let bytes = B64.decode(text).wrap_err("not base64url")?;
    Signature::from_slice(&bytes).map_err(|_| eyre!("not a P-256 signature"))
}

impl FromStr for Proof {
    type Err = eyre::Report;

    fn from_str(value: &str) -> Result<Self> {
        let (ts, sig) = value
            .split_once('.')
            .ok_or_else(|| eyre!("a proof is <ms>.<signature>"))?;
        Ok(Self {
            ts: Ms(ts.parse().wrap_err("not a moment")?),
            sig: signature(sig)?,
        })
    }
}

impl fmt::Display for Proof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.ts.0, B64.encode(self.sig.to_bytes()))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
