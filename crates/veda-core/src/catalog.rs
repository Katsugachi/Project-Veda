use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelQuant {
    Q5,
    Q8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    ChatModel,
    EmbeddingModel,
    LlamaRuntime,
    RuntimeDependency,
    Docset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub kind: AssetKind,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub license: String,
    #[serde(default)]
    pub quant: Option<ModelQuant>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub docset: Option<crate::DocsetId>,
    #[serde(default)]
    pub source_subdir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    pub generated_at: String,
    pub llama_cpp_release: String,
    pub assets: Vec<Asset>,
}

pub fn default_catalog() -> Catalog {
    let model_base = "https://huggingface.co/Abiray/MiniCPM5-1B-GGUF/resolve/main";
    let runtime_base = "https://github.com/ggml-org/llama.cpp/releases/download/b10369";
    Catalog {
        schema_version: 1,
        generated_at: "2026-08-12T00:00:00Z".into(),
        llama_cpp_release: "b10369".into(),
        assets: vec![
            Asset {
                id: "minicpm5-q5".into(),
                name: "MiniCPM 5".into(),
                kind: AssetKind::ChatModel,
                url: format!("{model_base}/minicpm5-1b-Q5_K_M.gguf"),
                sha256: "a9408d2e911e3b29ef40a7d9bf5d25e480d770733e30958df018c7be65a77e30".into(),
                bytes: 786_862_688,
                license: "Apache-2.0".into(),
                quant: Some(ModelQuant::Q5),
                platform: None,
                backend: None,
                docset: None,
                source_subdir: None,
            },
            Asset {
                id: "minicpm5-q8".into(),
                name: "MiniCPM 5".into(),
                kind: AssetKind::ChatModel,
                url: format!("{model_base}/minicpm5-1b-Q8_0.gguf"),
                sha256: "60b7e21be12abb44725e18ff4feecfbba53e216e8ea52e49112c40252a839f5d".into(),
                bytes: 1_153_529_261,
                license: "Apache-2.0".into(),
                quant: Some(ModelQuant::Q8),
                platform: None,
                backend: None,
                docset: None,
                source_subdir: None,
            },
            Asset {
                id: "bge-small-q8".into(),
                name: "Offline search support".into(),
                kind: AssetKind::EmbeddingModel,
                url: "https://huggingface.co/CompendiumLabs/bge-small-en-v1.5-gguf/resolve/main/bge-small-en-v1.5-q8_0.gguf".into(),
                sha256: "ec38e8da142596baa913124ae50550de284b6916bf59577ef2f0cb9660c2f514".into(),
                bytes: 36_806_944,
                license: "MIT".into(),
                quant: Some(ModelQuant::Q8),
                platform: None,
                backend: Some("embeddings".into()),
                docset: None,
                source_subdir: None,
            },
            docset_source("python-source-3.14.7", "Python 3.14.7 documentation", crate::DocsetId::Python, "https://docs.python.org/3/archives/python-3.14-docs-html.zip", "9306da398ae5a9142deb22d5c7865994fe0ada961022c8dea8ee341348e14181", 16_740_440, "python-3.14-docs-html", "PSF-2.0"),
            docset_source("cppreference-source-20250209", "cppreference 2025-02-09", crate::DocsetId::Cpp, "https://github.com/PeterFeicht/cppreference-doc/releases/download/v20250209/html-book-20250209.zip", "5389f2635f1417f05319b11f065ceecd1a8442c9143e2c127409137396b0c81c", 55_740_889, "reference/en", "CC-BY-SA-3.0 AND GFDL-1.3-no-invariants-or-later"),
            docset_source("mdn-html-source-20260812", "MDN HTML 2026-08-12", crate::DocsetId::Html, "https://github.com/mdn/content/archive/83cd10f1d5850fdde087ae7e399810723bead4d3.zip", "1a5d2d615980a4b124f7898d65eabfa8884521d5f4b1d2a0f1ab372f3779d7ef", 73_684_713, "content-83cd10f1d5850fdde087ae7e399810723bead4d3/files/en-us/web/html", "CC-BY-SA-2.5"),
            docset_source("mdn-css-source-20260812", "MDN CSS 2026-08-12", crate::DocsetId::Css, "https://github.com/mdn/content/archive/83cd10f1d5850fdde087ae7e399810723bead4d3.zip", "1a5d2d615980a4b124f7898d65eabfa8884521d5f4b1d2a0f1ab372f3779d7ef", 73_684_713, "content-83cd10f1d5850fdde087ae7e399810723bead4d3/files/en-us/web/css", "CC-BY-SA-2.5"),
            docset_source("mdn-javascript-source-20260812", "MDN JavaScript 2026-08-12", crate::DocsetId::Javascript, "https://github.com/mdn/content/archive/83cd10f1d5850fdde087ae7e399810723bead4d3.zip", "1a5d2d615980a4b124f7898d65eabfa8884521d5f4b1d2a0f1ab372f3779d7ef", 73_684_713, "content-83cd10f1d5850fdde087ae7e399810723bead4d3/files/en-us/web/javascript", "CC-BY-SA-2.5"),
            runtime("llama-macos-arm64", "macos-arm64", "metal", format!("{runtime_base}/llama-b10369-bin-macos-arm64.tar.gz"), "de2ac2c0a7cc245bce2411393658ff19c9c00d9d1fe37c5dfe94668c0d7bc01f", 11_069_404),
            runtime("llama-macos-x64", "macos-x64", "metal", format!("{runtime_base}/llama-b10369-bin-macos-x64.tar.gz"), "3cd137ae474fe4a55dcbcf319b94b36fe136571e6ca680d3eb8a175aa6ff1717", 11_345_911),
            runtime("llama-win-cpu-x64", "windows-x64", "cpu", format!("{runtime_base}/llama-b10369-bin-win-cpu-x64.zip"), "d6f606412f2335bc4a2324750306e8b5b027e8327f183990b2dbe3671f7f9dbd", 18_458_753),
            runtime("llama-win-vulkan-x64", "windows-x64", "vulkan", format!("{runtime_base}/llama-b10369-bin-win-vulkan-x64.zip"), "862d0c017b6fcc3d8541bad0da051f535550b16fba162a5d94dadd754c5a07b7", 34_201_860),
            runtime("llama-win-cuda-x64", "windows-x64", "cuda", format!("{runtime_base}/llama-b10369-bin-win-cuda-12.4-x64.zip"), "5eca96bb069281deda8e882843193a605a7b0687c659c6b4123715a4f1642642", 250_748_190),
            runtime_dependency("cudart-win-cuda-x64", "windows-x64", "cuda", format!("{runtime_base}/cudart-llama-bin-win-cuda-12.4-x64.zip"), "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6", 391_443_627),
            runtime("llama-win-cpu-arm64", "windows-arm64", "cpu", format!("{runtime_base}/llama-b10369-bin-win-cpu-arm64.zip"), "e4654a6832ae05503962d7711821d025b13d7b410ece846f6db751158fb6fbb8", 12_290_971),
        ],
    }
}

#[allow(clippy::too_many_arguments)]
fn docset_source(
    id: &str,
    name: &str,
    docset: crate::DocsetId,
    url: &str,
    sha256: &str,
    bytes: u64,
    source_subdir: &str,
    license: &str,
) -> Asset {
    Asset {
        id: id.into(),
        name: name.into(),
        kind: AssetKind::Docset,
        url: url.into(),
        sha256: sha256.into(),
        bytes,
        license: license.into(),
        quant: None,
        platform: None,
        backend: None,
        docset: Some(docset),
        source_subdir: Some(source_subdir.into()),
    }
}

fn runtime_dependency(
    id: &str,
    platform: &str,
    backend: &str,
    url: String,
    sha256: &str,
    bytes: u64,
) -> Asset {
    let mut asset = runtime(id, platform, backend, url, sha256, bytes);
    asset.name = format!("llama.cpp {backend} libraries");
    asset.kind = AssetKind::RuntimeDependency;
    asset
}

fn runtime(
    id: &str,
    platform: &str,
    backend: &str,
    url: String,
    sha256: &str,
    bytes: u64,
) -> Asset {
    Asset {
        id: id.into(),
        name: format!("llama.cpp {backend}"),
        kind: AssetKind::LlamaRuntime,
        url,
        sha256: sha256.into(),
        bytes,
        license: "MIT".into(),
        quant: None,
        platform: Some(platform.into()),
        backend: Some(backend.into()),
        docset: None,
        source_subdir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplied_q5_is_pinned() {
        let catalog = default_catalog();
        let q5 = catalog
            .assets
            .iter()
            .find(|asset| asset.id == "minicpm5-q5")
            .unwrap();
        assert_eq!(q5.bytes, 786_862_688);
        assert_eq!(q5.sha256.len(), 64);
        assert!(q5.url.ends_with("minicpm5-1b-Q5_K_M.gguf"));
    }
}
