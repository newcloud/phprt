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
