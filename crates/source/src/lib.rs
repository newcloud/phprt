use std::path::PathBuf;

/// 方案里的一个源码包：下载和解压两个阶段都消费这一份清单。
#[derive(Debug, Clone)]
pub struct Source {
    /// php / deps / ext
    pub label: String,
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// 缓存里的压缩包：<root>/.phprt/downloads/<文件名>
    pub archive: PathBuf,
    /// 解压后的源码根：<root>/build/<组件>/<stem>
    pub unpack: PathBuf,
    /// 相对项目根的解压目录，用于输出
    pub unpack_display: String,
}
