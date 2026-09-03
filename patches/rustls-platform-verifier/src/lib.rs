//! Patched rustls-platform-verifier for Rivulet.
//!
//! On Android, the original crate requires JNI initialization which panics
//! when reqwest builds a client on a background thread before JNI is ready.
//! This patch uses bundled Mozilla root certificates (webpki-roots) instead,
//! identical to what `ureq` already does successfully.

use std::sync::Arc;

mod verification;
pub use verification::Verifier;

/// Extension trait to help configure `ClientConfig`s with the platform verifier.
pub trait BuilderVerifierExt {
    fn with_platform_verifier(
        self,
    ) -> Result<rustls::ConfigBuilder<rustls::ClientConfig, rustls::client::WantsClientCert>, rustls::Error>;
}

impl BuilderVerifierExt for rustls::ConfigBuilder<rustls::ClientConfig, rustls::WantsVerifier> {
    fn with_platform_verifier(
        self,
    ) -> Result<rustls::ConfigBuilder<rustls::ClientConfig, rustls::client::WantsClientCert>, rustls::Error> {
        let verifier = verification::Verifier::new(self.crypto_provider().clone())?;
        Ok(self
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier)))
    }
}

/// Extension trait to help build a `ClientConfig` with the platform verifier.
pub trait ConfigVerifierExt {
    fn with_platform_verifier() -> Result<rustls::ClientConfig, rustls::Error>;
}

impl ConfigVerifierExt for rustls::ClientConfig {
    fn with_platform_verifier() -> Result<rustls::ClientConfig, rustls::Error> {
        Ok(rustls::ClientConfig::builder()
            .with_platform_verifier()?
            .with_no_client_auth())
    }
}
