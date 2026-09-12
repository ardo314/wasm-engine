//! Fetching a component artifact from an OCI registry.
//!
//! `ArtifactRef.sha256` is the registry's own identity for the blob, so this
//! only has to produce the bytes — the resolver verifies them.

use oci_client::client::{ClientConfig, ClientProtocol};
use oci_client::secrets::RegistryAuth;
use oci_wasm::WasmClient;
use wasm_host::{Fetch, FetchError};
use wasm_registry::ArtifactRef;

/// Pulls components from OCI registries.
pub struct OciFetcher {
    client: WasmClient,
    auth: RegistryAuth,
}

impl OciFetcher {
    /// Pulls anonymously over HTTPS, which is what a public registry needs.
    pub fn new() -> Self {
        Self::with_config(ClientConfig::default(), RegistryAuth::Anonymous)
    }

    pub fn with_auth(auth: RegistryAuth) -> Self {
        Self::with_config(ClientConfig::default(), auth)
    }

    /// Talks plain HTTP to `registries`, for a local registry that has no
    /// certificate. Everything else stays on HTTPS.
    pub fn insecure(registries: Vec<String>, auth: RegistryAuth) -> Self {
        Self::with_config(
            ClientConfig {
                protocol: ClientProtocol::HttpsExcept(registries),
                ..ClientConfig::default()
            },
            auth,
        )
    }

    pub fn with_config(config: ClientConfig, auth: RegistryAuth) -> Self {
        Self {
            client: WasmClient::new(oci_client::Client::new(config)),
            auth,
        }
    }
}

impl Default for OciFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for OciFetcher {
    async fn fetch(&self, artifact: &ArtifactRef) -> Result<Vec<u8>, FetchError> {
        let reference = artifact
            .uri
            .strip_prefix("oci://")
            .unwrap_or(&artifact.uri)
            .parse()
            .map_err(|e| {
                FetchError::new(format!("`{}` is not an image reference: {e}", artifact.uri))
            })?;

        let image = self
            .client
            .pull(&reference, &self.auth)
            .await
            .map_err(|e| FetchError::new(format!("{e:#}")))?;

        // `WasmClient::pull` has already rejected any layer that is not wasm,
        // so more than one means the artifact is not a single component.
        let [layer] = image.layers.as_slice() else {
            return Err(FetchError::new(format!(
                "expected one wasm layer, found {}",
                image.layers.len()
            )));
        };
        Ok(layer.data.to_vec())
    }
}
