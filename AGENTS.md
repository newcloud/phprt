# phprt 项目约定

1. 一棵版本树就是一个标准 PHP 安装：`.phprt/php-<版本>/` 下是 `bin/`、`etc/`、`lib/`、`deps/`。

2. 配置只用 PHP 官方默认路径：主配置 `etc/php.ini`，扫描目录 `etc/conf.d/`，CA 文件 `etc/ssl/cert.pem`。

3. `.phprt/` 根下不放 `php.ini` 和 `conf.d`；扩展开关和 `00-ssl.ini` 都写进版本树的 `etc/conf.d/`。

4. 版本树必须可移动：整体改名或搬到别处后，项目里的 `./myphp` 照常可用，不重编、不改配置。

5. 编译完成后的产物里不得依赖构建树的绝对路径；`--prefix` 只决定安装位置，不参与运行时查找。

6. 会写死路径的库单独处理：OpenSSL 的 `--openssldir` 指向系统目录，不执行会写系统目录的安装目标。

7. 运行时路径全部由 `myphp` 在启动时按自身位置算出，再用环境变量或 `-d` 传给 php。

8. 需要环境变量才能表达的配置只写在 `myphp`，不散落到脚本、文档或用户配置里。

9. CA 只有一份，放在版本树的 `etc/ssl/cert.pem`；OpenSSL 用 `SSL_CERT_FILE`，PHP 的 openssl / curl 扩展用 `conf.d/00-ssl.ini` 里的 `${SSL_CERT_FILE}`。

10. 不再引入第二份 CA、第二个证书目录，也不使用 `CURL_CA_BUNDLE` 改 PHP 的 CA。

11. `php.ini` 里不写绝对路径；需要路径时引用 `myphp` 导出的环境变量。

12. 工具链默认值与用户改动分开：工具链写 `00-ssl.ini`，用户覆盖写编号更大的 ini。

13. 用户只需理解「项目里有一个标准 PHP 安装」：配置去 `etc/php.ini`，扩展去 `etc/conf.d/`，不记 phprt 私有目录。

14. 新增或修改路径、环境变量、ini 约定时，同步更新生成入口、构建逻辑、校验逻辑和本文档。

15. 验收以改名/搬移测试和 `phprt verify` 通过为准。

16. 扩展状态不落盘：不保存 `project.conf` 或每版本清单，统一按 `myphp` 相同环境跑
    `php -m` 推导；用户手改 `php.ini`、删 `.so` 时结果必须仍然准确。

17. 构建目录按版本分：`.phprt/build-php-<版本>/`，删掉只影响该版本的增量重建；
    `.phprt` 顶层除 `current`、`php-*`、`build-php-*` 外不放别的东西。

18. 契约检查是开发门禁，只放在 `scripts/recipe-check.sh`；不提供 `phprt recipe`
    子命令。新增或修改 recipe 后必须跑通，才算添加成功。

19. 扩展覆盖的进度、约束、问题台账和优化提案只记在 `docs/EXTENSIONS.md`。
    新增扩展必须按该文件的流程做两轮验证（基线 → 安全版）并更新进度。

20. 开发是发现驱动的：遇到框架/流程问题、新的优化方向、需要改 driver 才能通过
    的情况，停下报告等决定，不擅自实施；扩展内部问题自行处理并记台账。

21. README 只给普通用户看，结构固定，不新增章节、全文控制在 80 行内：
    简介 → 安装依赖 → 快速开始（`install` / `install --downloads` + 两张截图 +
    `./myphp -r` 示例）→ 使用说明（只放 `docs/GUIDE.md` 目录）→ 扩展支持
    （按分类一行列出）→ 项目特点（只列 5 条）→ 注意事项与说明 → 协议一行。
    参数表、扩展表、目录树、下载/代理细节、菜单键位一律写 `docs/GUIDE.md`；
    开发与契约细节写 `docs/DEVELOPMENT.md` / `docs/RECIPE.md`。
    新增功能先改 GUIDE.md，README 只允许修正已有段落里的事实错误。

22. 扩展分类的唯一定义在 `runner/common.sh` 的 `PHPRT_EXT_CATEGORIES` 和
    `PHPRT_CATEGORY_LABELS`；分类键不能带空格，显示名可以。改分类必须同时改
    recipe 的 `category=`、`docs/RECIPE.md` 的值域、`docs/DEVELOPMENT.md` 和
    `docs/EXTENSIONS.md` 的顺序说明、`docs/GUIDE.md` 的菜单顺序、
    `docs/TEST-V2.md` 的选择器预期和 `README.md` 的扩展支持行，然后跑
    `scripts/recipe-check.sh`。分类顺序就是菜单顺序，不要另写一份顺序表。

23. 改了命令相关的行为（子命令、参数、默认值、输出提示、退出码、示例命令），
    必须同步更新 `docs/GUIDE.md` 对应章节：参数表、示例输出、「命令速查」都要
    与实际一致。改完至少实跑一次该命令并核对输出；`--help` 文本在
    `runner/commands.sh` 的 `usage()`，与 GUIDE 一起改。README 只在受影响的
    事实描述属于第 21 条允许的范围内才动。

## 契约读取顺序（AI）

1. 先跑 `phprt ext list <系列> --json --name <扩展>`，拿完整契约；
   不要先读 `runner/common.sh` 或逐个 `bash recipe.sh --flags`。
2. 读 JSON 里的 `hooks`、`needs_detail`、`opts`、`flags_on/off`、`notes`；
   需要背景时再读目标 recipe 的头部。
3. 实跑 `--check`、`--flags`（builtin 的 switch=0/1 各一次）、`--needs`、`--opts`。
4. 只把 `docs/RECIPE.md` 的 normative 段当契约；历史/审计段和
   `docs/EXTENSIONS.md` 的台账只作背景。
5. 改完跑 `scripts/recipe-check.sh`；构建后跑 `phprt verify`；数字类文档由
   `scripts/doc-check.sh` 兜底（Phase 5 引入）。
