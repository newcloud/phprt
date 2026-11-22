use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use oci_client::Reference;
use oci_client::client::{Client, ClientConfig, ClientProtocol};
use oci_client::secrets::RegistryAuth;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::store::{Store, digest_hex, dir_size, short_digest, write_atomic};
use crate::types::ImageInfo;
use crate::unpack::{LayerFile, detect_pm, unpack};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LayerMeta {
    pub media_type: String,
    pub digest: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ImageFile {
    pub reference: String,
    pub digest: String,
    pub list_digest: Option<String>,
    pub platform: String,
    pub created: String,
    pub layers: Vec<LayerMeta>,
}

pub fn add(
    store: &Store,
    reference: &str,
    alias: Option<&str>,
    offline: bool,
) -> Result<ImageInfo> {
    if offline {
        return match store.resolve(alias.unwrap_or(reference)) {
            Ok(image) => Ok(image),
            Err(_) => bail!("--offline 下镜像不在本地缓存：{reference}"),
        };
    }
    let reference: Reference = reference
        .parse()
        .with_context(|| format!("镜像引用不合法：{reference}"))?;
    let store = store.clone();
    let alias = alias.map(str::to_owned);
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move { pull(&store, &reference, alias.as_deref()).await })
}

async fn pull(store: &Store, reference: &Reference, alias: Option<&str>) -> Result<ImageInfo> {
    let client = Client::new(ClientConfig {
        protocol: ClientProtocol::Https,
        ..Default::default()
    });
    let auth = RegistryAuth::Anonymous;
    println!("拉取镜像 {reference}");
    let (manifest, digest, config, list_digest) = client
        .pull_manifest_and_config_and_list_digest(reference, &auth)
        .await
        .with_context(|| format!("拉取 {reference} 的 manifest 失败"))?;

    let dir = store.image_dir(&digest);
    if dir.join("rootfs").is_dir() && dir.join("image.json").is_file() {
        println!("镜像 {} 已缓存（{}）", short_digest(&digest), dir.display());
        let meta = read_image_file(&dir)?;
        store.record(
            &digest,
            &reference.to_string(),
            alias,
            &meta.platform,
            &meta.created,
            dir_size(&dir),
        )?;
        return store.resolve(&digest);
    }

    let config_json: serde_json::Value =
        serde_json::from_str(&config).context("镜像 config 不是合法 JSON")?;
    let os = config_json
        .get("os")
        .and_then(|v| v.as_str())
        .unwrap_or("linux");
    let arch = config_json
        .get("architecture")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if os != "linux" {
        bail!("只支持 linux 镜像，当前镜像是 {os}");
    }
    let host_arch = std::env::consts::ARCH;
    let arch_matches = arch == host_arch || (host_arch == "x86_64" && arch == "amd64");
    if !arch_matches {
        bail!("镜像架构是 {arch}，当前宿主是 {host_arch}，不支持跨架构");
    }
    if manifest.layers.is_empty() {
        bail!("镜像没有 rootfs 层");
    }

    let platform = format!("{os}/{arch}");
    let created = config_json
        .get("created")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    fs::create_dir_all(dir.join("layers"))?;
    write_atomic(&dir.join("config.json"), config.as_bytes())?;
    write_atomic(
        &dir.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;

    let mut layers = Vec::new();
    for (index, layer) in manifest.layers.iter().enumerate() {
        let name = format!("{index:02}-{}.blob", digest_hex(&layer.digest));
        let path = dir.join("layers").join(&name);
        if !path.exists() {
            print!(
                "下载层 {index} {} ({:.1} MB) ... ",
                short_digest(&layer.digest),
                layer.size as f64 / 1024.0 / 1024.0
            );
            std::io::stdout().flush().ok();
            let tmp = path.with_extension("part");
            {
                let file = tokio::fs::File::create(&tmp).await?;
                client
                    .pull_blob(reference, layer, file)
                    .await
                    .with_context(|| format!("下载层 {} 失败", layer.digest))?;
            }
            let got = sha256_file(&tmp)?;
            if got != layer.digest {
                let _ = fs::remove_file(&tmp);
                bail!(
                    "层 {} 校验失败：期望 {}，实际 {}",
                    layer.digest,
                    layer.digest,
                    got
                );
            }
            fs::rename(&tmp, &path)?;
            println!("完成");
        }
        layers.push(LayerMeta {
            media_type: layer.media_type.clone(),
            digest: layer.digest.clone(),
            path: format!("layers/{name}"),
        });
    }

    let image_file = ImageFile {
        reference: reference.to_string(),
        digest: digest.clone(),
        list_digest,
        platform: platform.clone(),
        created: created.clone(),
        layers: layers.clone(),
    };
    write_atomic(
        &dir.join("image.json"),
        &serde_json::to_vec_pretty(&image_file)?,
    )?;

    println!("解包 rootfs ...");
    let layer_files: Vec<LayerFile> = layers
        .iter()
        .map(|layer| LayerFile {
            media_type: layer.media_type.clone(),
            digest: layer.digest.clone(),
            path: dir.join(&layer.path),
        })
        .collect();
    unpack(&dir, &layer_files)?;
    let pm = detect_pm(&dir.join("rootfs"))?;
    let size = dir_size(&dir);
    store.record(
        &digest,
        &reference.to_string(),
        alias,
        &platform,
        &created,
        size,
    )?;
    println!(
        "完成：{}  {}  {}  {:.1} MB",
        short_digest(&digest),
        platform,
        pm,
        size as f64 / 1024.0 / 1024.0
    );
    store.resolve(&digest)
}

pub(crate) fn read_image_file(dir: &Path) -> Result<ImageFile> {
    let path = dir.join("image.json");
    let text = fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} 不是合法元数据", path.display()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
