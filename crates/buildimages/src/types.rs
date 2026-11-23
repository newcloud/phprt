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
    /// 运行时实例 id，也是 libcontainer / systemd scope 名。
    pub id: String,
    /// 用户可见的实例名；全局唯一。
    pub name: String,
    pub image: String,
    pub image_ref: String,
    pub project_root: PathBuf,
    pub mounts: Vec<MountSpec>,
    pub executable: PathBuf,
}

/// 一个已经准备好的构建实例。
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub image_ref: String,
    /// 创建实例的项目根，只用于显示。
    pub owner: String,
    /// 用过这个实例的所有项目根。
    pub projects: Vec<String>,
    pub created: String,
    pub bundle: PathBuf,
    pub rootfs: PathBuf,
}

/// 实例目录里的元数据，用来判断能否复用。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct InstanceMeta {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub image_ref: String,
    /// 旧版本字段名是 `project_root`，读取时兼容。
    #[serde(default, alias = "project_root")]
    pub owner: String,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub mounts: Vec<String>,
}

impl InstanceMeta {
    /// 用户可见名字。旧实例没有 `name` 时从旧 id 推导，如
    /// `phprt-4add57aa-php-8.5-bfc36aecdece` → `php-8.5`。
    pub(crate) fn display_name(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        crate::legacy_instance_name(&self.id).unwrap_or_else(|| self.id.clone())
    }

    /// 所有用过这个实例的项目根；旧实例没有列表时退回 owner。
    pub(crate) fn project_roots(&self) -> Vec<String> {
        if !self.projects.is_empty() {
            self.projects.clone()
        } else if !self.owner.is_empty() {
            vec![self.owner.clone()]
        } else {
            Vec::new()
        }
    }
}
