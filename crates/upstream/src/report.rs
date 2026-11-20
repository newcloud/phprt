//! 结果报告：文本输出和退出状态。

use serde::Serialize;

/// 一个组件一条线的处理结果。
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub component: String,
    pub line: String,
    pub status: Status,
    /// 新增 / 回填的版本；无更新时为空。
    pub version: String,
    /// 写进 catalog 的最终地址。
    pub url: String,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// 线内发现更大的补丁，已写入（或 dry-run 计划写入）。
    NewPatch,
    /// 已有条目缺 sha256，已回填。
    Filled,
    /// 线内没有更大的补丁。
    Current,
    /// 组件没有 upstream 脚本，跳过。
    Skipped,
    /// 查询、解析或下载失败。
    Failed,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub rows: Vec<Row>,
    /// 有组件失败但其余成功（退出码 3 的依据）。
    pub partial_failure: bool,
}

impl Report {
    pub fn push(
        &mut self,
        component: &str,
        line: &str,
        status: Status,
        version: &str,
        url: &str,
        note: String,
    ) {
        if status == Status::Failed {
            self.partial_failure = true;
        }
        self.rows.push(Row {
            component: component.to_owned(),
            line: line.to_owned(),
            status,
            version: version.to_owned(),
            url: url.to_owned(),
            note,
        });
    }

    /// 本轮有没有对 catalog 做改动（新增或回填）。
    pub fn changed(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.status, Status::NewPatch | Status::Filled))
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        for row in &self.rows {
            let line = if row.line.is_empty() {
                String::new()
            } else {
                format!(" {}", row.line)
            };
            let version = if row.version.is_empty() {
                String::new()
            } else {
                format!(" {}", row.version)
            };
            out.push_str(&format!(
                "{}{}: {}{}\n",
                row.component,
                line,
                status_label(row.status),
                version
            ));
            if !row.url.is_empty() {
                out.push_str(&format!("  url: {}\n", row.url));
            }
            if !row.note.is_empty() {
                out.push_str(&format!("  {}\n", row.note));
            }
        }
        if self.rows.is_empty() {
            out.push_str("没有组件可更新（catalog 为空，或全部组件没有 upstream 脚本）\n");
        }
        out
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::NewPatch => "新增补丁",
        Status::Filled => "回填 sha256",
        Status::Current => "无更新",
        Status::Skipped => "跳过",
        Status::Failed => "失败",
    }
}
