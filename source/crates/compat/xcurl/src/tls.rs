use crate::net::{CURLE_PEER_FAILED_VERIFICATION, CURLE_SSL_CERTPROBLEM, CURLE_SSL_CONNECT_ERROR};
use crate::sys::*;
use core::ffi::c_void;
use std::sync::Arc;

pub const CURLE_SSL_CIPHER: i32 = 59;
pub const CURLE_SSL_CACERT_BADFILE: i32 = 77;

const ENC: u32 = 0x0001_0001;
const CERT_STORE_PROV_MEMORY: usize = 2;
const CERT_STORE_PROV_SYSTEM_W: usize = 10;
const CERT_STORE_ADD_ALWAYS: u32 = 4;
const CERT_STORE_OPEN_EXISTING_FLAG: u32 = 0x4000;
const CERT_STORE_READONLY_FLAG: u32 = 0x8000;
const CERT_FIND_HASH: u32 = 0x10000;
const CERT_HASH_PROP_ID: u32 = 3;
const CERT_KEY_PROV_INFO_PROP_ID: u32 = 2;
const CERT_KEY_CONTEXT_PROP_ID: u32 = 5;
const CERT_NCRYPT_KEY_HANDLE_PROP_ID: u32 = 78;
const CERT_NCRYPT_KEY_HANDLE_TRANSFER_PROP_ID: u32 = 99;
const PKCS12_NO_PERSIST_KEY: u32 = 0x8000;
const CRYPT_USER_KEYSET: u32 = 0x1000;
const CRYPT_DECODE_ALLOC_FLAG: u32 = 0x8000;
const PKCS_RSA_PRIVATE_KEY: usize = 43;
const NCRYPTBUFFER_PKCS_SECRET: u32 = 46;
const CERT_TRUST_IS_UNTRUSTED_ROOT: u32 = 0x20;
const CERT_TRUST_REVOCATION_STATUS_UNKNOWN: u32 = 0x40;
const CERT_TRUST_IS_OFFLINE_REVOCATION: u32 = 0x0100_0000;
const CERT_TRUST_IS_PARTIAL_CHAIN: u32 = 0x0001_0000;

pub const WINHTTP_FLAG_SECURE_PROTOCOL_TLS1: u32 = 0x80;
pub const WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_1: u32 = 0x200;
pub const WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2: u32 = 0x800;
pub const WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3: u32 = 0x2000;

#[repr(C)]
struct Blob {
    n: u32,
    p: *const u8,
}

#[repr(C)]
struct NBuffer {
    cb: u32,
    ty: u32,
    pv: *const c_void,
}

#[repr(C)]
struct NBufferDesc {
    version: u32,
    count: u32,
    buffers: *const NBuffer,
}

#[repr(C)]
struct ChainPara {
    cb: u32,
    usage_type: u32,
    usage_count: u32,
    usage: *const c_void,
}

#[link(name = "crypt32")]
extern "system" {
    fn PFXImportCertStore(blob: *const Blob, password: *const u16, flags: u32) -> *mut c_void;
    fn CertOpenStore(provider: usize, enc: u32, prov: usize, flags: u32, para: *const c_void) -> *mut c_void;
    fn CertCloseStore(store: *mut c_void, flags: u32) -> i32;
    fn CertEnumCertificatesInStore(store: *mut c_void, prev: *const c_void) -> *const c_void;
    fn CertFindCertificateInStore(store: *mut c_void, enc: u32, find_flags: u32, find_type: u32, para: *const c_void, prev: *const c_void) -> *const c_void;
    fn CertGetCertificateContextProperty(ctx: *const c_void, prop: u32, data: *mut c_void, len: *mut u32) -> i32;
    fn CertSetCertificateContextProperty(ctx: *const c_void, prop: u32, flags: u32, data: *const c_void) -> i32;
    fn CertDuplicateCertificateContext(ctx: *const c_void) -> *const c_void;
    fn CertFreeCertificateContext(ctx: *const c_void) -> i32;
    fn CertCreateCertificateContext(enc: u32, data: *const u8, len: u32) -> *const c_void;
    fn CertAddEncodedCertificateToStore(store: *mut c_void, enc: u32, data: *const u8, len: u32, disp: u32, out: *mut *const c_void) -> i32;
    fn CertGetCertificateChain(engine: *mut c_void, ctx: *const c_void, time: *const c_void, store: *mut c_void, para: *const ChainPara, flags: u32, reserved: *mut c_void, out: *mut *const c_void) -> i32;
    fn CertFreeCertificateChain(chain: *const c_void);
    fn CryptDecodeObjectEx(enc: u32, ty: usize, data: *const u8, len: u32, flags: u32, para: *const c_void, out: *mut c_void, outlen: *mut u32) -> i32;
}

#[link(name = "ncrypt")]
extern "system" {
    fn NCryptOpenStorageProvider(ph: *mut usize, name: *const u16, flags: u32) -> i32;
    fn NCryptImportKey(prov: usize, import_key: usize, blob_type: *const u16, params: *const NBufferDesc, key: *mut usize, data: *const u8, len: u32, flags: u32) -> i32;
    fn NCryptFreeObject(h: usize) -> i32;
}

extern "system" {
    fn LocalFree(p: *mut c_void) -> *mut c_void;
}

pub struct CertCtx(usize);

unsafe impl Send for CertCtx {}
unsafe impl Sync for CertCtx {}

impl CertCtx {
    pub fn ptr(&self) -> *const c_void {
        self.0 as *const c_void
    }

    pub fn thumbprint(&self) -> String {
        unsafe { thumb(self.ptr()) }
    }
}

impl Drop for CertCtx {
    fn drop(&mut self) {
        unsafe { CertFreeCertificateContext(self.0 as *const c_void) };
    }
}

pub struct CaStore {
    store: usize,
    certs: Vec<Vec<u8>>,
}

unsafe impl Send for CaStore {}
unsafe impl Sync for CaStore {}

impl Drop for CaStore {
    fn drop(&mut self) {
        unsafe { CertCloseStore(self.store as *mut c_void, 0) };
    }
}

impl CaStore {
    pub fn len(&self) -> usize {
        self.certs.len()
    }
}

#[derive(Clone, Default)]
pub struct Tls {
    pub client: Option<Arc<CertCtx>>,
    pub protocols: u32,
    pub ca: Option<Arc<CaStore>>,
    pub revocation: bool,
    pub revoke_best_effort: bool,
}

unsafe fn encoded<'a>(ctx: *const c_void) -> &'a [u8] {
    let p = *((ctx as *const u8).add(8) as *const *const u8);
    let n = *((ctx as *const u8).add(16) as *const u32);
    core::slice::from_raw_parts(p, n as usize)
}

unsafe fn thumb(ctx: *const c_void) -> String {
    let mut h = [0u8; 20];
    let mut n = 20u32;
    if CertGetCertificateContextProperty(ctx, CERT_HASH_PROP_ID, h.as_mut_ptr() as *mut c_void, &mut n) == 0 {
        return String::new();
    }
    h[..n as usize].iter().map(|b| format!("{b:02x}")).collect()
}

unsafe fn has_key(ctx: *const c_void) -> bool {
    [CERT_KEY_PROV_INFO_PROP_ID, CERT_KEY_CONTEXT_PROP_ID, CERT_NCRYPT_KEY_HANDLE_PROP_ID].iter().any(|p| {
        let mut n = 0u32;
        CertGetCertificateContextProperty(ctx, *p, core::ptr::null_mut(), &mut n) != 0
    })
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\r' | b'\n' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub fn pem_blocks(text: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(b) = rest.find("-----BEGIN ") {
        let after = &rest[b + 11..];
        let Some(le) = after.find("-----") else { break };
        let label = after[..le].to_string();
        let body = &after[le + 5..];
        let end_tag = format!("-----END {label}-----");
        let Some(e) = body.find(&end_tag) else { break };
        let inner: String = body[..e].lines().filter(|l| !l.contains(':')).collect::<Vec<_>>().join("");
        if let Some(der) = b64_decode(&inner) {
            out.push((label, der));
        }
        rest = &body[e + end_tag.len()..];
    }
    out
}

fn source(v: &str) -> Result<Vec<u8>, String> {
    if v.trim_start().starts_with("-----BEGIN") {
        return Ok(v.as_bytes().to_vec());
    }
    std::fs::read(v).map_err(|e| format!("could not read '{v}': {e}"))
}

pub fn ssl_version(v: i64) -> Result<u32, String> {
    let min = (v & 0xffff) as u32;
    let max = ((v >> 16) & 0xffff) as u32;
    let all = [
        (4u32, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1),
        (5, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_1),
        (6, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2),
        (7, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3),
    ];
    let lo = match min {
        0 => {
            if max == 0 || max == 1 {
                return Ok(0);
            }
            6
        }
        1 | 4 => 4,
        5 | 6 | 7 => min,
        2 | 3 => return Err("SSLv2 and SSLv3 are not supported by Schannel".into()),
        _ => return Err(format!("unknown CURLOPT_SSLVERSION minimum {min}")),
    };
    let hi = match max {
        0 | 1 => 7,
        4..=7 => max,
        _ => return Err(format!("unknown CURLOPT_SSLVERSION maximum {max}")),
    };
    if hi < lo {
        return Err(format!("CURLOPT_SSLVERSION maximum TLS 1.{} is below the minimum TLS 1.{}", hi - 4, lo - 4));
    }
    Ok(all.iter().filter(|(k, _)| *k >= lo && *k <= hi).fold(0, |a, (_, f)| a | f))
}

const KNOWN_KEYWORDS: &[&str] = &["DEFAULT", "ALL", "HIGH", "MEDIUM", "LOW", "COMPLEMENTOFDEFAULT", "COMPLEMENTOFALL", "SECURE128", "SECURE192", "SUITEB128", "SUITEB192", "AES", "AESGCM", "CHACHA20", "ECDHE", "EECDH", "DHE", "EDH", "RSA", "ECDSA", "AESCCM", "kRSA", "aRSA", "aECDSA", "TLSv1.2", "TLSv1", "SSLv3", "SHA256", "SHA384", "SHA1", "SHA", "eNULL", "aNULL", "NULL", "MD5", "RC4", "DES", "3DES", "EXPORT", "PSK", "SRP", "CAMELLIA", "SEED", "IDEA"];

fn cipher_token_ok(t: &str) -> bool {
    let t = t.trim_start_matches(['!', '-', '+', '@']);
    if t.is_empty() {
        return true;
    }
    if t.starts_with("STRENGTH") || t.starts_with("SECLEVEL=") {
        return true;
    }
    if let Some(c) = t.strip_prefix("CALG_") {
        return !c.is_empty() && c.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    }
    if t.starts_with("TLS_") || t.starts_with("SSL_") {
        return t.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    }
    if KNOWN_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(t)) {
        return true;
    }
    t.contains('-') && t.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

pub struct CipherPlan {
    pub tls13_only: bool,
    pub names: Vec<String>,
}

pub fn ciphers(list: &str) -> Result<CipherPlan, String> {
    let names: Vec<String> = list.split([':', ',', ' ', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if names.is_empty() {
        return Err("empty cipher list".into());
    }
    if let Some(bad) = names.iter().find(|n| !cipher_token_ok(n)) {
        return Err(format!("failed setting cipher list: unknown cipher '{bad}'"));
    }
    let positive: Vec<&String> = names.iter().filter(|n| !n.starts_with('!') && !n.starts_with('-')).collect();
    let tls13 = |n: &str| n.starts_with("TLS_AES_") || n.starts_with("TLS_CHACHA20_");
    let tls13_only = !positive.is_empty() && positive.iter().all(|n| tls13(n));
    Ok(CipherPlan { tls13_only, names })
}

unsafe fn import_key(der: &[u8], label: &str, pass: Option<&str>) -> Result<usize, String> {
    let mut prov = 0usize;
    let kprov = wide("Microsoft Software Key Storage Provider");
    let st = NCryptOpenStorageProvider(&mut prov, kprov.as_ptr(), 0);
    if st != 0 {
        return Err(format!("NCryptOpenStorageProvider failed 0x{:08x}", st as u32));
    }
    let mut key = 0usize;
    let result = match label {
        "PRIVATE KEY" | "ENCRYPTED PRIVATE KEY" => {
            let ty = wide("PKCS8_PRIVATEKEY");
            let pw = pass.map(wide);
            let buf = pw.as_ref().map(|p| NBuffer { cb: (p.len() * 2) as u32, ty: NCRYPTBUFFER_PKCS_SECRET, pv: p.as_ptr() as *const c_void });
            let desc = buf.as_ref().map(|b| NBufferDesc { version: 0, count: 1, buffers: b as *const NBuffer });
            let dp = desc.as_ref().map_or(core::ptr::null(), |d| d as *const NBufferDesc);
            if label == "ENCRYPTED PRIVATE KEY" && pass.is_none() {
                Err("the private key is encrypted and CURLOPT_KEYPASSWD is not set".to_string())
            } else {
                let st = NCryptImportKey(prov, 0, ty.as_ptr(), dp, &mut key, der.as_ptr(), der.len() as u32, 0);
                if st == 0 { Ok(key) } else { Err(format!("could not import the PKCS#8 private key (0x{:08x})", st as u32)) }
            }
        }
        "RSA PRIVATE KEY" => {
            let mut out: *mut u8 = core::ptr::null_mut();
            let mut n = 0u32;
            if CryptDecodeObjectEx(ENC, PKCS_RSA_PRIVATE_KEY, der.as_ptr(), der.len() as u32, CRYPT_DECODE_ALLOC_FLAG, core::ptr::null(), &mut out as *mut *mut u8 as *mut c_void, &mut n) == 0 {
                Err(format!("could not decode the PKCS#1 RSA private key (error {})", GetLastError()))
            } else {
                let ty = wide("CAPIPRIVATEBLOB");
                let st = NCryptImportKey(prov, 0, ty.as_ptr(), core::ptr::null(), &mut key, out, n, 0);
                LocalFree(out as *mut c_void);
                if st == 0 { Ok(key) } else { Err(format!("could not import the RSA private key (0x{:08x})", st as u32)) }
            }
        }
        other => Err(format!("unsupported private key type '{other}'; Schannel needs PKCS#8 or PKCS#1 RSA keys")),
    };
    NCryptFreeObject(prov);
    result
}

unsafe fn from_pfx(data: &[u8], pass: Option<&str>) -> Result<CertCtx, String> {
    let blob = Blob { n: data.len() as u32, p: data.as_ptr() };
    let pw = wide(pass.unwrap_or(""));
    let store = PFXImportCertStore(&blob, pw.as_ptr(), CRYPT_USER_KEYSET | PKCS12_NO_PERSIST_KEY);
    if store.is_null() {
        return Err(format!("could not import the PKCS#12 certificate (error {}); check CURLOPT_KEYPASSWD", GetLastError()));
    }
    let mut first: *const c_void = core::ptr::null();
    let mut chosen: *const c_void = core::ptr::null();
    let mut c = CertEnumCertificatesInStore(store, core::ptr::null());
    while !c.is_null() {
        if first.is_null() {
            first = CertDuplicateCertificateContext(c);
        }
        if has_key(c) {
            chosen = CertDuplicateCertificateContext(c);
            CertFreeCertificateContext(c);
            break;
        }
        c = CertEnumCertificatesInStore(store, c);
    }
    CertCloseStore(store, 0);
    if chosen.is_null() {
        if !first.is_null() {
            CertFreeCertificateContext(first);
        }
        return Err("the PKCS#12 file holds no certificate with a private key".into());
    }
    if !first.is_null() {
        CertFreeCertificateContext(first);
    }
    Ok(CertCtx(chosen as usize))
}

unsafe fn from_store(path: &str) -> Result<CertCtx, String> {
    let parts: Vec<&str> = path.splitn(3, ['\\', '/']).collect();
    if parts.len() != 3 {
        return Err(format!("'{path}' is neither a readable file nor a Schannel store path like CurrentUser\\MY\\<thumbprint>"));
    }
    let loc = match parts[0].to_ascii_lowercase().as_str() {
        "currentuser" => 1u32,
        "localmachine" => 2,
        "currentservice" => 4,
        "services" => 5,
        "users" => 6,
        "currentusergrouppolicy" => 7,
        "localmachinegrouppolicy" => 8,
        "localmachineenterprise" => 9,
        other => return Err(format!("unknown certificate store location '{other}'")),
    };
    let hex: String = parts[2].chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 40 {
        return Err(format!("certificate thumbprint '{}' is not 40 hex digits", parts[2]));
    }
    let hash: Vec<u8> = (0..20).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0)).collect();
    let name = wide(parts[1]);
    let store = CertOpenStore(CERT_STORE_PROV_SYSTEM_W, 0, 0, (loc << 16) | CERT_STORE_OPEN_EXISTING_FLAG | CERT_STORE_READONLY_FLAG, name.as_ptr() as *const c_void);
    if store.is_null() {
        return Err(format!("could not open certificate store {}\\{} (error {})", parts[0], parts[1], GetLastError()));
    }
    let blob = Blob { n: 20, p: hash.as_ptr() };
    let c = CertFindCertificateInStore(store, ENC, 0, CERT_FIND_HASH, &blob as *const Blob as *const c_void, core::ptr::null());
    CertCloseStore(store, 0);
    if c.is_null() {
        return Err(format!("no certificate with thumbprint {hex} in {}\\{}", parts[0], parts[1]));
    }
    Ok(CertCtx(c as usize))
}

unsafe fn from_pem(cert: &[u8], key: Option<&[u8]>, pass: Option<&str>) -> Result<CertCtx, String> {
    let text = String::from_utf8_lossy(cert).into_owned();
    let blocks = pem_blocks(&text);
    let Some((_, der)) = blocks.iter().find(|(l, _)| l == "CERTIFICATE" || l == "X509 CERTIFICATE") else {
        return Err("no PEM CERTIFICATE block found in CURLOPT_SSLCERT".into());
    };
    let ctx = CertCreateCertificateContext(ENC, der.as_ptr(), der.len() as u32);
    if ctx.is_null() {
        return Err(format!("could not parse the client certificate (error {})", GetLastError()));
    }
    let ctx = CertCtx(ctx as usize);
    let key_text = match key {
        Some(k) => String::from_utf8_lossy(k).into_owned(),
        None => text.clone(),
    };
    let kb = pem_blocks(&key_text);
    let Some((label, kder)) = kb.iter().find(|(l, _)| l.ends_with("PRIVATE KEY")) else {
        return Err("no PEM private key found; set CURLOPT_SSLKEY".into());
    };
    let h = import_key(kder, label, pass)?;
    if CertSetCertificateContextProperty(ctx.ptr(), CERT_NCRYPT_KEY_HANDLE_PROP_ID, 0, &h as *const usize as *const c_void) == 0 {
        NCryptFreeObject(h);
        return Err(format!("could not attach the private key to the certificate (error {})", GetLastError()));
    }
    CertSetCertificateContextProperty(ctx.ptr(), CERT_NCRYPT_KEY_HANDLE_TRANSFER_PROP_ID, 0, &h as *const usize as *const c_void);
    Ok(ctx)
}

pub struct ClientCertSpec<'a> {
    pub cert: Option<&'a str>,
    pub cert_blob: Option<&'a [u8]>,
    pub cert_type: Option<&'a str>,
    pub key: Option<&'a str>,
    pub key_blob: Option<&'a [u8]>,
    pub key_type: Option<&'a str>,
    pub pass: Option<&'a str>,
}

pub fn client_cert(s: &ClientCertSpec) -> Result<Option<CertCtx>, (i32, String)> {
    let err = |m: String| (CURLE_SSL_CERTPROBLEM, format!("unable to use client certificate: {m}"));
    let ty = s.cert_type.map(|t| t.to_ascii_uppercase());
    if let Some(kt) = s.key_type.map(|t| t.to_ascii_uppercase()) {
        if kt == "ENG" {
            return Err(err("crypto engines are not available with Schannel".into()));
        }
        if kt != "PEM" && kt != "DER" {
            return Err(err(format!("unknown CURLOPT_SSLKEYTYPE '{kt}'")));
        }
    }
    let data = match (s.cert_blob, s.cert) {
        (Some(b), _) => b.to_vec(),
        (None, Some(c)) if !c.is_empty() => {
            if ty.as_deref() != Some("PEM") && ty.as_deref() != Some("DER") && !std::path::Path::new(c).exists() && !c.trim_start().starts_with("-----BEGIN") {
                return unsafe { from_store(c) }.map(Some).map_err(err);
            }
            source(c).map_err(err)?
        }
        _ => return Ok(None),
    };
    let key = match (s.key_blob, s.key) {
        (Some(b), _) => Some(b.to_vec()),
        (None, Some(k)) if !k.is_empty() => Some(source(k).map_err(err)?),
        _ => None,
    };
    let looks_pem = data.windows(11).any(|w| w == b"-----BEGIN ");
    unsafe {
        match ty.as_deref() {
            Some("P12") | Some("PFX") => from_pfx(&data, s.pass),
            Some("PEM") => from_pem(&data, key.as_deref(), s.pass),
            Some("DER") => {
                let ctx = CertCreateCertificateContext(ENC, data.as_ptr(), data.len() as u32);
                if ctx.is_null() {
                    Err(format!("could not parse the DER client certificate (error {})", GetLastError()))
                } else {
                    let ctx = CertCtx(ctx as usize);
                    match key {
                        Some(k) => {
                            let kt = String::from_utf8_lossy(&k).into_owned();
                            let (label, der) = pem_blocks(&kt).into_iter().find(|(l, _)| l.ends_with("PRIVATE KEY")).unwrap_or(("PRIVATE KEY".into(), k));
                            let h = match import_key(&der, &label, s.pass) {
                                Ok(h) => h,
                                Err(m) => return Err(err(m)),
                            };
                            CertSetCertificateContextProperty(ctx.ptr(), CERT_NCRYPT_KEY_HANDLE_PROP_ID, 0, &h as *const usize as *const c_void);
                            CertSetCertificateContextProperty(ctx.ptr(), CERT_NCRYPT_KEY_HANDLE_TRANSFER_PROP_ID, 0, &h as *const usize as *const c_void);
                            Ok(ctx)
                        }
                        None => Err("a DER certificate needs CURLOPT_SSLKEY".to_string()),
                    }
                }
            }
            Some("ENG") => Err("crypto engines are not available with Schannel".into()),
            Some(other) => Err(format!("unknown CURLOPT_SSLCERTTYPE '{other}'")),
            None if looks_pem => from_pem(&data, key.as_deref(), s.pass),
            None => from_pfx(&data, s.pass),
        }
    }
    .map(Some)
    .map_err(err)
}

pub fn ca_store(path: Option<&str>, blob: Option<&[u8]>) -> Result<Option<CaStore>, (i32, String)> {
    let data = match (blob, path) {
        (Some(b), _) => b.to_vec(),
        (None, Some(p)) if !p.is_empty() => std::fs::read(p).map_err(|e| (CURLE_SSL_CACERT_BADFILE, format!("error setting certificate verify locations: CAfile: {p}: {e}")))?,
        _ => return Ok(None),
    };
    let text = String::from_utf8_lossy(&data).into_owned();
    let mut ders: Vec<Vec<u8>> = pem_blocks(&text).into_iter().filter(|(l, _)| l == "CERTIFICATE" || l == "TRUSTED CERTIFICATE").map(|(_, d)| d).collect();
    if ders.is_empty() && data.first() == Some(&0x30) {
        ders.push(data.clone());
    }
    if ders.is_empty() {
        return Err((CURLE_SSL_CACERT_BADFILE, "error setting certificate verify locations: no certificates in the CA bundle".into()));
    }
    unsafe {
        let store = CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, core::ptr::null());
        if store.is_null() {
            return Err((CURLE_SSL_CACERT_BADFILE, "could not create a certificate store for the CA bundle".into()));
        }
        let mut kept = Vec::new();
        for d in ders {
            if CertAddEncodedCertificateToStore(store, ENC, d.as_ptr(), d.len() as u32, CERT_STORE_ADD_ALWAYS, core::ptr::null_mut()) != 0 {
                kept.push(d);
            }
        }
        if kept.is_empty() {
            CertCloseStore(store, 0);
            return Err((CURLE_SSL_CACERT_BADFILE, "no parseable certificates in the CA bundle".into()));
        }
        Ok(Some(CaStore { store: store as usize, certs: kept }))
    }
}

pub unsafe fn verify_with_ca(server: *const c_void, ca: &CaStore, revocation: bool) -> Result<(), (i32, String)> {
    let para = ChainPara { cb: core::mem::size_of::<ChainPara>() as u32, usage_type: 0, usage_count: 0, usage: core::ptr::null() };
    let mut chain: *const c_void = core::ptr::null();
    let flags = if revocation { 0x2000_0000 } else { 0 };
    if CertGetCertificateChain(core::ptr::null_mut(), server, core::ptr::null(), ca.store as *mut c_void, &para, flags, core::ptr::null_mut(), &mut chain) == 0 || chain.is_null() {
        return Err((CURLE_PEER_FAILED_VERIFICATION, format!("could not build the server certificate chain (error {})", GetLastError())));
    }
    let status = *((chain as *const u8).add(4) as *const u32);
    let n = *((chain as *const u8).add(12) as *const u32);
    let mut anchored = false;
    if n > 0 {
        let simple = **((chain as *const u8).add(16) as *const *const *const u8);
        let ne = *(simple.add(12) as *const u32);
        let elems = *(simple.add(16) as *const *const *const u8);
        for i in 0..ne as usize {
            let el = *elems.add(i);
            let cert = *(el.add(8) as *const *const c_void);
            let der = encoded(cert);
            if ca.certs.iter().any(|c| c.as_slice() == der) {
                anchored = true;
                break;
            }
        }
    }
    CertFreeCertificateChain(chain);
    let tolerated = CERT_TRUST_IS_UNTRUSTED_ROOT | CERT_TRUST_IS_PARTIAL_CHAIN | if revocation { 0 } else { CERT_TRUST_REVOCATION_STATUS_UNKNOWN | CERT_TRUST_IS_OFFLINE_REVOCATION };
    if !anchored {
        return Err((CURLE_PEER_FAILED_VERIFICATION, "SSL certificate problem: unable to get local issuer certificate (chain does not reach any CURLOPT_CAINFO certificate)".into()));
    }
    if status & !tolerated != 0 {
        return Err((CURLE_PEER_FAILED_VERIFICATION, format!("SSL certificate problem: chain status 0x{status:08x}")));
    }
    Ok(())
}

pub unsafe fn free_cert(ctx: *const c_void) {
    CertFreeCertificateContext(ctx);
}

pub fn connect_error(msg: String) -> (i32, String) {
    (CURLE_SSL_CONNECT_ERROR, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ranges() {
        assert_eq!(ssl_version(0).unwrap(), 0);
        assert_eq!(ssl_version(6).unwrap(), WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3);
        assert_eq!(ssl_version(6 | (6 << 16)).unwrap(), WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2);
        assert!(ssl_version(3).is_err());
        assert!(ssl_version(7 | (6 << 16)).is_err());
    }

    #[test]
    fn cipher_lists() {
        assert!(ciphers("ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:!aNULL").is_ok());
        assert!(ciphers("TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384").unwrap().tls13_only);
        assert!(ciphers("CALG_AES_256:CALG_SHA_384").is_ok());
        assert!(ciphers("x").is_err());
        assert!(ciphers("").is_err());
    }

    #[test]
    fn pem_and_base64() {
        let p = "junk\n-----BEGIN CERTIFICATE-----\nAAEC\nAw==\n-----END CERTIFICATE-----\n";
        let b = pem_blocks(p);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].1, vec![0, 1, 2, 3]);
    }

    #[test]
    fn missing_files_are_cert_problems() {
        let r = client_cert(&ClientCertSpec { cert: Some("Z:\\nope\\client.pem"), cert_blob: None, cert_type: Some("PEM"), key: None, key_blob: None, key_type: None, pass: None });
        assert_eq!(r.err().unwrap().0, CURLE_SSL_CERTPROBLEM);
        let r = client_cert(&ClientCertSpec { cert: Some("x"), cert_blob: None, cert_type: None, key: None, key_blob: None, key_type: None, pass: None });
        assert_eq!(r.err().unwrap().0, CURLE_SSL_CERTPROBLEM);
        assert_eq!(ca_store(Some("Z:\\nope\\ca.pem"), None).err().unwrap().0, CURLE_SSL_CACERT_BADFILE);
    }
}
