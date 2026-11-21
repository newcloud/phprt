//! 结果报告：文本输出和退出状态。

use serde::Serialize;

/// 一个组件一条线的处理结果。
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub component: String,
    pub line: String,
    pub status: Status,
    /// 处理前的最大版本；线不存在时为空。
    pub current: String,
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
    /// 上游返回里没有属于这条线的版本（可能超出接口返回范围）；不是失败，但要人看。
    NoCandidate,
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
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        component: &str,
        line: &str,
        status: Status,
        current: &str,
        version: &str,
        url: &str,
        note: String,
    ) {
        self.push_row(Row {
            component: component.to_owned(),
            line: line.to_owned(),
            status,
            current: current.to_owned(),
            version: version.to_owned(),
            url: url.to_owned(),
            note,
        });
    }

    /// 直接放一条已经拼好的结果。
    pub fn push_row(&mut self, row: Row) {
        if row.status == Status::Failed {
            self.partial_failure = true;
        }
        self.rows.push(row);
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
            out.push_str(&format!(
                "{}{}: {}\n",
                row.component,
                line,
                detail(row.status, &row.current, &row.version)
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

/// 一条版本线的结果文字，报告和进度行共用。
pub(crate) fn detail(status: Status, current: &str, version: &str) -> String {
    match status {
        Status::NewPatch if current.is_empty() => format!("新增 {version}"),
        Status::NewPatch => format!("{current} → {version}"),
        Status::Filled => format!("回填 sha256 {version}"),
        Status::Current if current.is_empty() => "无更新".to_owned(),
        Status::Current => format!("无更新 {current}"),
        Status::NoCandidate if current.is_empty() => "无候选".to_owned(),
        Status::NoCandidate => format!("无候选（本地 {current}）"),
        Status::Skipped => "跳过".to_owned(),
        Status::Failed if version.is_empty() => "失败".to_owned(),
        Status::Failed => format!("失败 {version}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_each_line_on_one_row() {
        let mut report = Report::default();
        report.push(
            "sqlite",
            "3.54",
            Status::Current,
            "3.54.1",
            "",
            "",
            String::new(),
        );
        report.push(
            "php",
            "8.2",
            Status::NewPatch,
            "8.2.29",
            "8.2.36",
            "https://x/php-8.2.36.tar.gz",
            String::new(),
        );
        report.push(
            "luajit",
            "2.1",
            Status::Failed,
            "2.1",
            "2.1-20261117",
            "https://x/v2.1-20261117.tar.gz",
            "文件名版本号不一致".to_owned(),
        );
        assert_eq!(
            report.text(),
            concat!(
                "sqlite 3.54: 无更新 3.54.1\n",
                "php 8.2: 8.2.29 → 8.2.36\n",
                "  url: https://x/php-8.2.36.tar.gz\n",
                "luajit 2.1: 失败 2.1-20261117\n",
                "  url: https://x/v2.1-20261117.tar.gz\n",
                "  文件名版本号不一致\n",
            )
        );
    }

    #[test]
    fn detail_covers_every_state() {
        assert_eq!(
            detail(Status::NewPatch, "8.2.29", "8.2.36"),
            "8.2.29 → 8.2.36"
        );
        assert_eq!(detail(Status::NewPatch, "", "8.2.36"), "新增 8.2.36");
        assert_eq!(
            detail(Status::Filled, "7.4.33", "7.4.33"),
            "回填 sha256 7.4.33"
        );
        assert_eq!(detail(Status::Current, "3.54.1", ""), "无更新 3.54.1");
        assert_eq!(
            detail(Status::NoCandidate, "7.4.33", ""),
            "无候选（本地 7.4.33）"
        );
        assert_eq!(detail(Status::Skipped, "", ""), "跳过");
        assert_eq!(detail(Status::Failed, "2.1", ""), "失败");
    }
}
