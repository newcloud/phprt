use std::path::PathBuf;

/// 一个已经缓存到本地的 OCI 镜像。
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub digest: String,
    pub reference: String,
    pub refs: Vec<String>,
    pub aliases: Vec<String>,
    pub platform: String,
    pub created: String,
    pub size: u64,
    pub dir: PathBuf,
}

impl ImageInfo {
    pub fn rootfs(&self) -> PathBuf {
        self.dir.join("rootfs")
    }
}

/// 要挂进构建容器的一个目录或文件。
#[derive(Debug, Clone)]
pub struct MountSpec {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub readonly: bool,
}

/// 创建 / 复用一个构建实例需要的全部信息。
#[derive(Debug, Clone)]
pub struct InstanceRequest {
    pub id: String,
    pub image: String,
    pub project_root: PathBuf,
    pub mounts: Vec<MountSpec>,
    pub executable: PathBuf,
}

/// 一个已经准备好的构建实例。
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub id: String,
    pub image: String,
    pub project_root: PathBuf,
    pub bundle: PathBuf,
    pub rootfs: PathBuf,
}

/// 实例目录里的元数据，用来判断能否复用。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct InstanceMeta {
    pub id: String,
    pub image: String,
    pub project_root: String,
    pub created: String,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub mounts: Vec<String>,
}
