use std::fmt::Debug;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types;
use rustls::{
    DigitallySignedStruct, Error as TlsError, OtherError, SignatureScheme,
};

fn log_server_cert(_end_entity: &rustls::pki_types::CertificateDer<'_>) {}

#[derive(Debug)]
struct EkuError;

impl std::fmt::Display for EkuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("certificate had invalid extensions")
    }
}

impl std::error::Error for EkuError {}

/// A TLS certificate verifier that uses bundled Mozilla root certs (webpki-roots).
///
/// This is used on Android where the platform verifier requires JNI which
/// may not be available on background threads.
pub struct Verifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Verifier").finish()
    }
}

impl Verifier {
    pub fn new(crypto_provider: Arc<CryptoProvider>) -> Result<Self, TlsError> {
        Self::new_inner([], crypto_provider)
    }

    pub fn new_with_extra_roots(
        extra_roots: impl IntoIterator<Item = pki_types::CertificateDer<'static>>,
        crypto_provider: Arc<CryptoProvider>,
    ) -> Result<Self, TlsError> {
        Self::new_inner(extra_roots, crypto_provider)
    }

    fn new_inner(
        extra_roots: impl IntoIterator<Item = pki_types::CertificateDer<'static>>,
        crypto_provider: Arc<CryptoProvider>,
    ) -> Result<Self, TlsError> {
        let mut root_store = rustls::RootCertStore::empty();

        for cert in extra_roots {
            root_store.add(cert)?;
        }

        let (_added, ignored) = root_store.add_parsable_certificates(
            webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned(),
        );
        if ignored > 0 {
            log::warn!("{ignored} bundled CA root certificates were ignored due to errors");
        }

        if root_store.is_empty() {
            return Err(TlsError::General(
                "No bundled CA certificates found".to_owned(),
            ));
        }

        log::debug!("Loaded {} bundled CA root certificates", root_store.len());

        Ok(Self {
            inner: WebPkiServerVerifier::builder_with_provider(
                root_store.into(),
                crypto_provider.clone(),
            )
            .build()
            .map_err(|e| TlsError::Other(OtherError(Arc::new(e))))?,
        })
    }
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &pki_types::CertificateDer<'_>,
        intermediates: &[pki_types::CertificateDer<'_>],
        server_name: &pki_types::ServerName,
        ocsp_response: &[u8],
        _now: pki_types::UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        log_server_cert(end_entity);

        self.inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, _now)
            .map_err(|e| {
                log::error!("failed to verify TLS certificate: {}", e);
                e
            })
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}
