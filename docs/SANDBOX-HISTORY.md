# 沙箱记录

记录 phprt 曾经在 `./myphp` 里做过 bwrap 沙箱，后来又全部删除的经过。

## 时间线

| 提交 | 动作 | 规模 |
| --- | --- | --- |
| `9b86707` | `feat: myphp 支持 bwrap 沙箱` | +420 行 / 8 个文件 |
| `14333b6` | `refactor: 移除 bwrap 沙箱支持` | −504 行 / +36 行 / 8 个文件 |

## 加的内容

`./myphp` 在启动 PHP 之前把自己 `exec` 进 bwrap，参数大致是：

```
bwrap --die-with-parent --unshare-all \
      --tmpfs / --ro-bind /usr /usr --share-net \
      --symlink usr/bin /bin --symlink usr/lib64 /lib64 \
      --tmpfs /usr/lib64 \
      --ro-bind <白名单文件> … \
      --dev /dev --proc /proc --tmpfs /tmp --tmpfs /home \
      --ro-bind <项目> <项目> --ro-bind <安装根> <安装根> \
      --bind <PHPRT_RW_DIRS> … \
      --chdir <项目> -- <安装根>/php-<版本>/bin/php …
```

白名单由 `phprt build` 生成到 `php-<版本>/sandbox.libs`：`ldd` 扫 PHP 和全部扩展，
取安装根之外的路径，再补 `ldd` 看不到的 `dlopen` 依赖（NSS / gconv / locale）。
8.6 最小档 10 项，8.5 完整档 11 项。

新增的入口：

| 入口 | 参数 |
| --- | --- |
| `./myphp` | `--sandbox` / `--no-sandbox` / `--sandbox-check` / `--no-net` |
| `phprt init` | `--sandbox --rw var` |
| `install.sh` | `--sandbox --rw var,var/log` |
| 环境变量 | `PHPRT_SANDBOX` / `PHPRT_RW_DIRS` / `PHPRT_SANDBOX_OPTS` / `PHPRT_SANDBOX_LIBS` |

涉及的 8 个文件：`README.md`、`build.sh`、`docs/SANDBOX.md`、`install.sh`、
`lib/common.sh`、`lib/myphp-project.tmpl`、`lib/verify.sh`、`phprt`。

## 实测结果

在 `blog`（RoadRunner 3 + Slim 4 + SQLite，4 个 worker）上跑过：

```
$ curl -s localhost:8080/health
{"status":"ok","php":"8.6.0-dev","pid":2,"worker":"c012d2","sandbox":true,"posts":2}
```

| 项 | 结果 |
| --- | --- |
| 沙箱内模块数 | 33（与宿主一致） |
| `GET /` / 中文渲染 | 200 |
| `POST /post` | 303（SQLite 写入正常） |
| `GET /nope` | 404 |
| `kill -9` 一个 worker | RR 自动拉起 |
| 系统库可见性 | `libcurl` / `libssl` / `libicu` / `libxml2` / `libz` 均不可见 |
| `PHPRT_RW_DIRS` 只留 `var/log` | `GET /` 200，`POST /post` 500（SQLite 只读） |
| `PHPRT_SANDBOX=off` | `pid` 回到宿主 pid，`sandbox:false` |
| `phprt verify` 的 8b 段 | 通过 |

沙箱内 `pid 1` 是 bwrap，PHP 是 `pid 2`；进程树是 `rr → bwrap → bwrap → php`。

## 删除的原因

沙箱挂在 PHP worker 上，而服务是用 `rr serve` 启动的，沙箱边界和启动链不一致。
把边界挪到 `rr` 上时遇到的情况（实测）：

- `bin/rr` 是静态链接的 Go 二进制，可以进沙箱；
- 但 RR 要 exec `./myphp`，那是 bash 脚本，沙箱里起不来：
  `/bin/bash: error while loading shared libraries: libtinfo.so.6`；
- 内层再套 bwrap 也起不来：
  `/usr/bin/bwrap: error while loading shared libraries: libselinux.so.1`；
- RR 必须 bind 8080，网络只能 `--share-net`；
- 项目是只读但仍然可读（`var/blog.sqlite`、`.env` 等）；
- `PHPRT_SANDBOX_OPTS` 一行就能把系统库挂回去。

## 删掉的内容

```
README.md              | 140 ++++++--------------------------
build.sh               |   2 -
docs/SANDBOX.md        | 128 --------------------------------
install.sh             |  17 -----
lib/common.sh          |  50 ----------
lib/myphp-project.tmpl | 163 +--------------------------------
lib/verify.sh          |  16 ----
phprt                  |  24 +-------
8 files changed, 36 insertions(+), 504 deletions(-)
```

- `lib/myphp-project.tmpl`：`sandbox_exec()` 整段、4 个 `PHPRT_SANDBOX*` 变量、4 个开关、自检脚本
- `phprt`：`render_app` 的 `@SANDBOX@` / `@RW_DIRS@`，`init --sandbox/--rw`
- `install.sh`：`--sandbox` / `--rw` 解析与模板替换
- `lib/common.sh`：`sandbox_libs_file()`、`gen_sandbox_libs()`
- `build.sh`：白名单生成调用
- `lib/verify.sh`：`8b) 沙箱` 自检段
- `docs/SANDBOX.md`：整个文件
- `README.md`：沙箱章节、特性、FAQ、命令参考、布局
- 安装根里的 `php-<版本>/sandbox.libs`

## 现状

`./myphp` 103 行：设 `PHPRC` / `PHP_INI_SCAN_DIR` / `LD_LIBRARY_PATH` / `OPENSSL_CONF` /
`EXT_DIR`，然后 `exec php`。没有任何 bwrap 相关代码。
