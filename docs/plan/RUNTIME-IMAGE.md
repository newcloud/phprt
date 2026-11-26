# 运行镜像（OCI 产出）

状态：方案，未实现。**构建环境**（用 OCI 镜像里的工具编译）已经实现，见
[BUILD-IMAGES.md](../BUILD-IMAGES.md)；本文只描述把装好的安装树产出为能直接运行的
OCI 镜像。代码与本文冲突时以代码实际行为为准。

## 1. 目标

构建产物现在是 `.phprt/<产品>-<版本>/` 一棵自包含的安装树，能跑，但不能直接交付：
用户要手写 Dockerfile、自己拷库、自己试缺哪个 `.so`。

目标只有一条：**从一份已经装好的树，一条命令得到能跑的 OCI 镜像**，并告诉用户下一条
命令是什么。判据和 README 的目标一致：不手拷文件、不手写 Dockerfile、不手猜缺哪个
`.so`。

目标链路：

```text
Builder image
    ↓
phprt build
    ↓
artifact
    ↓
minimal runtime image
```

放进更大的价值链看，前四段已经成立，后两段是空的：

```text
源码
 ↓
Recipe
 ↓
依赖解析
 ↓
精确构建
 ↓
Runtime
 ↓
OCI Image
```

| 环节 | 现在 |
| --- | --- |
| Builder image | 已实现：`phprt buildimages add` + `--image`，用 OCI 镜像里的工具编译（[BUILD-IMAGES.md](../BUILD-IMAGES.md)） |
| artifact | 产物就是 `.phprt/php-8.5.12/` 整棵树；搬走要连库一起拷，没有清单、没有校验 |
| minimal runtime image | 用户手写 Dockerfile、裁库、试错缺哪个 `.so` |

当前状态离这条判据还很远。本文写的是终点，不是现状说明，也不是已实现用法。

现有的可搬移性是地基：运行时库已经收进 `lib/phprt`，rpath 全部是 `$ORIGIN` 相对路径
（[BUILD.md](../BUILD.md) §6），树整体平移不用重编。另一个 rpath 后端仍是方案，见
[plan/RPATH.md](RPATH.md)。

## 2. 边界：打包不需要 daemon

不需要用 `docker build` 来产镜像。用 Rust 的 `oci-spec` 出类型，`tar` + `flate2` 写层，
`sha2` 算 digest。产物是一个自包含的 OCI layout 目录。

首次出镜像仍要能读到 builder 镜像的内容，用来裁 runtime base。这一步读
[BUILD-IMAGES.md](../BUILD-IMAGES.md) 已经缓存的实例 rootfs，或按 digest 从 registry 取；
结果按 digest 缓存后，后续出镜像完全离线。所以准确的说法是：**不需要 daemon 来构建
镜像，但第一次要能访问 builder 镜像**。

镜像选择、实例和工具安装属于构建环境，本文不重复；只约定：runtime base 必须来自
同一次构建实际使用的 builder 镜像 digest。

## 3. 入口由 recipe 定

Rust 里不出现任何产品名。入口是产品声明，和 `tools` / `runtime` 一样写在
`product_profile(...)` 里：

```python
PROFILE = product_profile(
    ...,
    image = image(
        entrypoint = ["{tree}/sbin/nginx", "-p", "{tree}", "-c", "{tree}/etc/nginx.conf",
                      "-g", "daemon off;"],
        env = {"PATH": "{tree}/bin"},
        workdir = "/",
        ports = ["8080:80"],
    ),
)
```

- `entrypoint` 是 exec 数组。第一版只声明它，不声明 `cmd`、`stopsignal`、
  `healthcheck`。
- `ports` 写 `宿主:容器`，直接给 compose 用。写单个值时表示只暴露不映射。
  用 `8080:80` 而不是 `80:80`，是因为 rootless 运行绑不了 1024 以下的宿主端口。
- `env` 只放运行时需要的变量，构建期的 `CC` / `PHP_AUTOCONF` 不进来。
- 第一版镜像默认以 root 跑，和官方 PHP / nginx 镜像一致；`user` 字段先不做。
  非 root、`--read-only` 要处理属主和 tmpfs，是另一摊。

几个产品的 entrypoint：

- PHP CLI（选中 `cli` target）：`["{tree}/bin/php"]`，没有端口，不生成 compose；
- PHP-FPM（选中 `fpm` target；本方案支持，落地前按实际 init 脚本核对）：
  `["{tree}/sbin/php-fpm", "-F"]`，`ports = ["9000:9000"]`；
- nginx：`["{tree}/sbin/nginx", "-p", "{tree}", "-c", "{tree}/etc/nginx.conf",
  "-g", "daemon off;"]`；
- MySQL：`["{tree}/bin/mysqld"]`；
- CPython：`["{tree}/bin/python3"]`。

`entrypoint` 是数组不是脚本。要脚本就把脚本文件放进产品目录（和 `entry.sh`
一样），解释器由同一个 recipe 声明带进镜像。带不带 shell 是 recipe 的选择，
工具不替它决定。

注意区分两个「入口」：产品 profile 或选中 target 的 `entry` / `entry_template` 是
**宿主上的开发入口**（`myphp`），不进镜像；`image.entrypoint` 是镜像的入口。
镜像里的 `{tree}` 指安装树在镜像里的路径（`/opt/<产品>-<版本>/`，见 §6），不是宿主
安装树的路径。

## 4. 配置覆盖层

配置已经在树里，`post_install` 写的，直接进镜像，不另造机制。但树里的配置有两类
问题，必须覆盖一层：

- **开发向**：`php.ini` 里 `display_errors = On`、`error_reporting = E_ALL`，
  原样进镜像等于把开发配置发布到生产；
- **编译期绝对路径**：`extension_dir` 和 nginx 的 `--http-log-path` 都指向构建机的
  `.phprt/...`，树平移到 `/opt/...` 之后就失效了。

所以 `image` 声明多一个 `config`，值是「镜像内路径 → 产品目录里的文件名」，
只在打镜像时应用：同名覆盖，不同名新增。**宿主安装树一个字节都不变。**

```python
image = image(
    ...,
    config = {
        "etc/nginx.conf":          "nginx.image.conf",
        "etc/conf.d/00-image.ini": "image.ini",
    },
)
```

文件放在 `recipes/products/<产品>/` 下，和 `entry.sh` 同级，能 diff、能高亮。
模板支持 `{tree}`；PHP 的 `extension_dir` 用 `{extension_dir}`，由工具从安装树里
解析出实际目录后替换。

PHP 的 `image.ini`：

```ini
extension_dir = {extension_dir}
display_errors = Off
error_reporting = E_ALL & ~E_DEPRECATED
log_errors = On
```

nginx 的 `nginx.image.conf`：

```nginx
error_log stderr warn;
pid /tmp/nginx.pid;

events {
    worker_connections 1024;
}

http {
    include {tree}/etc/mime.types;
    access_log /dev/stdout;

    server {
        listen 80;
        root {tree}/html;
        index index.html;
    }
}
```

`include` 和 `root` 写绝对路径，不依赖 prefix 解析；`-p {tree}` 留给五个
`*_temp` 目录用。

覆盖方式就是挂载，路径固定：

```bash
-v ./php.ini:/opt/php-8.5.12/etc/php.ini:ro
-v ./conf.d:/opt/php-8.5.12/etc/conf.d:ro
-v ./nginx.conf:/opt/nginx-1.29.8/etc/nginx.conf:ro
```

不做「扫 `/config` 目录」这种 entrypoint 魔法，也不做 envsubst 模板渲染。
要模板就自己写个 entrypoint 脚本带进镜像，工具不掺和。

## 5. 日志与可写目录

**日志一律走 fd，不落文件。**

nginx 的覆盖配置里 `error_log stderr;` + `access_log /dev/stdout;`，nginx 原生
支持这两个写法。这里不用官方镜像那种软链，原因很实际：我们的日志路径是编译期
绝对路径（`{tree}/var/log/...`），指向构建机的目录，镜像里根本没有那个落点，
软链无处可建。

PHP CLI 没有日志文件：`log_errors = On` 且 `error_log` 为空时，错误直接走 SAPI
日志，CLI 就是 stderr。所以 `image.ini` 里不设 `error_log`。

php-fpm 加进来时，在它的 `php-fpm.image.conf` 里把默认的文件路径改掉：

```ini
error_log = /proc/self/fd/2
pid = /tmp/php-fpm.pid
```

pool 的 `access.log` 默认不开，要开就 `/proc/self/fd/1`；`slowlog` 默认关。
规则和上面一致，只是多一份配置。

`var/log/` 在镜像里**保留成空目录**，不删。它是给用户自己的配置写文件日志时用的，
挂载覆盖后路径不变。

**可写目录**：

- nginx 的五个缓冲目录：`client_body_temp/`、`proxy_temp/`、`fastcgi_temp/`、
  `uwsgi_temp/`、`scgi_temp/`。它们在树里已经存在，保留原位，不改到 `/tmp`——
  改路径要动 conf，上游默认就是 prefix 下。
- `var/`（pid 或用户自己的日志）、`/tmp`（session、上传）。
- 第一版默认 root，这些目录天然可写。`--read-only` 时把这几个目录挂 tmpfs，
  这是用户的事，镜像里不声明。

## 6. 文件清单：从入口反推

一棵装好默认组件的 PHP 树有一百多 M，其中 `include/`、`bin/phpize`、
`bin/php-config`、`phpdbg`、man、静态库，镜像都不需要。所以镜像文件集合要正向选，
不是 `cp -a`：

1. 入口二进制的 `NEEDED` 递归，得到必须的 `.so`；
2. `etc/conf.d/*.ini` 里的 `extension=` 指向的 `.so`，再递归它们的 `NEEDED`；
3. profile 声明的固定文件（`php.ini`、CA、时区、`/etc/passwd` 等）；
4. `image.config` 覆盖或新增的文件。

产出一份 manifest，每项是路径、sha256、来源、被谁需要：

```json
{ "path": "lib/phprt/libssl.so.3", "sha256": "...", "source": "elf", "needed_by": ["bin/php"] }
```

manifest 同时做三件事：决定镜像里放什么；校验（对每个 ELF 的 `NEEDED` 再核一遍，
缺一个就报错，**不需要起容器跑**）；可读（`plan --json` 里能看到清单和估算大小）。

这就是 README 那句「不手猜缺哪个 `.so`」的判据：靠算，不靠试。

最小镜像里**没有 `phpize`、`php-config` 和头文件**，所以不能在容器里编扩展。
要装扩展就重新 build 一个镜像，不是进容器 `phpize`。这条要写进说明，否则用户会
按官方镜像的习惯来用。

## 7. runtime base 必须和 builder 同源

树上每个 ELF 的 `NEEDED` 都有 `libc.so.6`，符号版本由构建时那套 glibc 决定。
runtime base 比它旧，二进制直接起不来。

所以默认路径是：**从同一次构建使用的 builder 镜像裁 runtime base**，只留 glibc 家族、
CA 证书、`/etc/passwd` 之类。裁剪清单向 builder 镜像的包管理器要一次
（`rpm -ql glibc` / `dpkg -L libc6`），按 digest 缓存到 `.phprt/images/<digest>.base/`，
不手写文件列表；缓存之后出镜像不需要再访问镜像。

不要另找小镜像：alpine 是 musl，distroless 的 glibc 版本和 builder 无关，
两者都是赌。

三种情况：

1. builder 来自内置验证过的镜像：照上面的裁，同源自不必说。
2. builder 是不带包管理器的镜像（scratch / distroless / 极度精简）：报错，要求
   `--runtime-base <镜像>`，不猜。
3. 显式给了 `--runtime-base`：跳过裁剪，但比对 libc 版本，比 builder 旧就报错。

## 8. 树放哪、OCI 输出、可复现

### 8.1 树放哪

整体平移到 `/opt/<产品>-<版本>/`。rpath 全是 `$ORIGIN` 相对路径，所以**不用重编**；
安装树里也没有绝对路径（`php.ini` 不含 `extension_dir`，那是入口运行时给的）。

镜像的 `Env` 写死绝对路径，入口直接 `exec`，不靠 bash 的 `cd -P` 算位置。
runtime base 因此可以不带 shell。

### 8.2 OCI 输出

一次构建的产物是一个目录：

```text
.phprt/images/php-8.5.12/
├── image.tar        # OCI image layout
├── manifest.json    # 文件清单，可审计
└── compose.yml      # 只有声明了 ports 才有（见第 9 节）
```

拷走这个目录就是完整一套，tar 和 compose 不会对不上。`--tag` 改目录名，
`/` 和 `:` 换成 `-`。

`image.tar` 的结构：

- `oci-spec` 出 `ImageManifest` / `ImageIndex` / `Config` / `Descriptor`；
- 两层：base 层（裁剪结果）+ 树层（manifest 选出的文件与必要 `/etc`）；
- **层用 gzip 压缩**，mediaType 是 `application/vnd.oci.image.layer.v1.tar+gzip`，
  descriptor 的 digest 按**压缩后**的 blob 算；
- **外层 `image.tar` 不再压缩**：层已经压过，再压一层几乎不变小，只会让 `load`
  多一次解压；
- 产物 digest、镜像 digest 写进镜像 config 的 label（`org.phprt.*`），
  不往镜像里塞文件；
- `--load` 才导入 docker / podman；默认只出 tar。

读取 OCI layout 需要 Docker 25+ 或 podman。这条写进说明和报错。

**不生成 Dockerfile。** 树已经自包含，Dockerfile 只是把「拷文件」再描述一遍，
还多一层和一次 `docker build`。

### 8.3 可复现

同样的输入必须得到字节一致的 `image.tar`：

- tar 内按路径排序，`mtime = 0`；
- uid / gid 一律 0；
- mode 归一化：目录和带可执行位的文件 `0755`，其余 `0644`；
- 层 gzip 的头部 `mtime = 0`，压缩级别固定（6）；
- 符号链接按链接写，不跟成实体文件（`lib/phprt` 里全是这种链）。

验收方式是同一输入构建两次，`sha256sum` 相同。

## 9. 运行提示与 compose

build 的终点不是「产出 tar」，是「用户马上能跑」。所以 build 结束时打印下一条
命令，并按需要生成 `compose.yml`。

### 9.1 输出

```text
镜像：php:8.5.12
产物：.phprt/images/php-8.5.12/
  image.tar
  manifest.json
  compose.yml

运行：
  podman load -i .phprt/images/php-8.5.12/image.tar
  docker compose -f .phprt/images/php-8.5.12/compose.yml up
```

加了 `--load` 时，load 自动做掉，提示里只剩 `compose up`。没有 `ports` 的产品
不生成 compose，提示只给 `docker run --rm <镜像> ...`。

### 9.2 生成规则

按 `image` 声明判断，不硬编码产品名：

- 声明了 `ports` → 生成 `compose.yml`；
- 没有端口（PHP CLI）→ 不生成，只给 `docker run`。

PHP 的 `image` 入口随构建目标（`--targets`）变化：只选 `cli` 时没有端口，不生成
compose；选中 `fpm` 时入口是 `php-fpm`、声明 `ports = ["9000:9000"]`，才生成 compose。
`image` 声明仍由产品 recipe 决定，Rust 只读最终的 `ports`，不在代码里判断产品名或
target 名。

理由是 PHP CLI 不是长期服务，硬塞 compose 进去，`up` 要么立刻退出，要么卡在
交互式 REPL。等 php-fpm 加进来，它的 `image` 声明里有 `ports = ["9000:9000"]`，
compose 自然就有了。

### 9.3 内容

```yaml
# 由 phprt 生成，重新 build 会覆盖。
services:
  nginx:
    image: nginx:1.29.8
    ports:
      - "8080:80"
    restart: unless-stopped
    # 要改配置时取消注释，并把文件放到 compose 同目录：
    # volumes:
    #   - ./nginx.conf:/opt/nginx-1.29.8/etc/nginx.conf:ro
```

- `image` 来自 `--tag` 或默认的 `<产品>:<补丁>`；
- `ports` 原样来自 `image.ports`；
- 配置和数据**默认不挂**：挂宿主上不存在的文件会被 docker 建成目录，直接报错，
  和「最快跑起来」相反。用户要改再取消注释；
- `restart: unless-stopped` 只给服务，一次性命令用不着。

生成物会被覆盖，这和 `myphp` 入口一样。要长期维护的配置复制出去再挂。

### 9.4 边界

- 不生成 `healthcheck`、`depends_on`、多服务编排、`.env`；
- 一个产品一个服务；php + nginx + fpm 的组合留给用户自己写；
- compose 只认本地镜像，`up` 之前必须先 load，这两步是一对，`--load` 只是压成一条；
- 不生成 Dockerfile。

## 10. 命令与输出

```bash
phprt build php@8.5
phprt build nginx@1.29 --load
phprt build php@8.5 --tag php:test-a
phprt build php@8.5 --runtime-base docker.io/library/debian:13
phprt plan php@8.5 --json
```

- `build <产品>@<系列>`：按当前方案和 state 产出镜像；已经装过且签名一致时跳过编译，
  直接出镜像。
- `--force` 对 `install` 和 `build` 都生效；`--runtime-base`、`--load`、`--tag`
  只对 `build` 生效。
- base 和 manifest 都命中缓存时，出镜像不需要网络，不需要重新访问 builder 镜像。
- 默认 OCI tag 是 `<产品>:<补丁>`，例如 `php:8.5.12`；`--tag` 覆盖。
- 构建环境（用哪个镜像编译、实例怎么复用）由 `install` / `add` / `update` 的
  `--image` / `--instance` 决定，见 [BUILD-IMAGES.md](../BUILD-IMAGES.md)；
  `build` 沿用 state 里的选择。
- 不加 `phprt image` / `phprt export` / `phprt run` 之类的第二条命令。

`plan --json` 新增 `image` 段：入口、端口、文件清单和估算大小；没有 `image` 声明或
产品没有安装时为 `null`。它不包含构建环境信息。

## 11. 失败模式

- runtime base 的 libc 比 builder 旧：报错。
- runtime base 不带包管理器、又没给 `--runtime-base`：报错，说明为什么不能裁。
- manifest 校验缺 soname：报错，写清缺哪个、被谁需要。
- 镜像写出失败：不写 state 的镜像相关记录，安装状态保留，重跑只重做镜像。
- `compose.yml` 写出失败：镜像 tar 保留，报错提示路径和原因，不静默跳过。
- 产品的 `image` 声明缺 `entrypoint`：报错，不生成没有入口的镜像。
- `--tag` 含非法字符：报错，提示允许的字符，不静默替换。

## 12. 不变量

1. 产品 recipe 不写镜像名、base；只声明入口、配置和端口。
2. 镜像里不放 phprt、不放工具链、不放源码、不放头文件。
3. 镜像 digest 只由输入决定，不由时间、宿主路径、umask 决定。
4. runtime base 必须与 builder 同源，或显式指定并通过 libc 校验。
5. 镜像文件集合由 manifest 决定，不由人肉列表决定。
6. 镜像配置覆盖只作用于镜像，不改变宿主安装树。
7. `compose.yml` 是生成物，不手写维护；重新 build 覆盖。
8. 安装树的 rpath 必须是 `$ORIGIN` 相对路径，镜像才能整树平移。

## 13. 不在范围

- 推 registry（第一版只出 tar）；
- 多架构、交叉编译（与 [DESIGN.md](../DESIGN.md) 的非目标一致）；
- 镜像分层缓存优化（先正确，层共享是后面的事）；
- 「产物能在老系统跑」的兼容矩阵；
- `cmd` / `stopsignal` / `healthcheck` 等镜像字段；
- 非 root 运行和 `--read-only` 的属主处理；
- `healthcheck` / `depends_on` / 多服务 compose；
- zstd 压缩、`--compression` 参数；
- 在镜像里放 phprt 或构建工具。

## 14. 落地顺序

每步独立验收，都不依赖下一步：

1. **manifest + 配置覆盖 + 静态校验**。纯宿主侧。验收：缺文件时报错而不是生成
   坏镜像；`image.config` 只改镜像不改树。
2. **runtime base 裁剪**。验收：从 builder 镜像裁出的 base 能起入口；缺包管理器时
   报错；`--runtime-base` 的 libc 校验生效。
3. **OCI tar 输出**。验收：`podman load` 后能跑，两次构建字节一致，层是 gzip。
4. **compose + 运行提示**。验收：`docker compose up` 一条命令起服务，
   `podman logs` 能看到 nginx 的 access log。
5. **`phprt build` 串起来**，入口从 recipe 读。验收：新机器一条命令出镜像，
   并打印下一步命令。

第 1 步可以完全离线做，最容易先对齐语义。

## 15. 改动清单

| 文件 | 改动 |
| --- | --- |
| `recipes/products/*/_profile.star` | 新增 `image(...)` 声明：入口、env、workdir、ports、config |
| `recipes/products/*/*.image.conf` / `*.ini` | 镜像配置模板（第 4 节） |
| `src/image.rs` | 新增：从 profile 的 `image` 声明生成 OCI config 和文件覆盖 |
| `crates/image/` | 新增：manifest、ELF `NEEDED` 解析、base 裁剪、gzip 层、OCI 写出 |
| `src/compose.rs` | 新增：按 `ports` 生成 `compose.yml` 和运行提示 |
| `src/main.rs` | 新增 `build` 子命令；`--runtime-base` / `--load` / `--tag` |
| `src/render.rs` | `plan --json` 的 `image` 段 |
| `Cargo.toml` | `oci-spec`、`tar`、`flate2`、`sha2` |
| 文档 | 本文；DESIGN.md 索引把本文从 `plan/` 移到现行；CLI.md 的 `build` 与参数；OUTPUT.md 的 `image` 段；CONTRACT.md / PRODUCT.md 的 `image` 字段；README 增加 `build` 用法 |

## 16. 验收（实施后）

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets

# 一条命令出镜像
cargo run -- build php@8.5
podman load -i .phprt/images/php-8.5.12/image.tar
podman run --rm php:8.5.12 --version
podman run --rm php:8.5.12 -r 'echo PHP_VERSION;'

# nginx：一条命令起服务
cargo run -- build nginx@1.29 --load
docker compose -f .phprt/images/nginx-1.29.8/compose.yml up -d
curl -fsS http://127.0.0.1:8080/ | head -1
docker compose -f .phprt/images/nginx-1.29.8/compose.yml down

# 可复现：两次完整构建，tar 字节一致
cargo run -- build php@8.5 --tag php:test-a
cargo run -- build php@8.5 --tag php:test-b
sha256sum .phprt/images/php-test-a/image.tar .phprt/images/php-test-b/image.tar

# 层是 gzip，外层 tar 未压缩
tar -tf .phprt/images/php-8.5.12/image.tar | head
mkdir -p /tmp/phprt-oci && tar -xf .phprt/images/php-8.5.12/image.tar -C /tmp/phprt-oci
file /tmp/phprt-oci/blobs/sha256/* | grep gzip

# 配置覆盖只改镜像
grep -q 'display_errors = On' .phprt/php-8.5.12/etc/php.ini
podman run --rm php:8.5.12 -i | grep display_errors   # Off

# runtime base 校验
cargo run -- build php@8.5 --runtime-base docker.io/library/debian:13
cargo run -- build php@8.5 --runtime-base scratch        # 报错：libc 不满足
```

## 17. 和现有文档的关系

- [DESIGN.md](../DESIGN.md) 是设计主入口，本文挂「未实现方案」一栏。
- [BUILD.md](../BUILD.md) 讲安装树怎么产出；本文只消费树。
- [BUILD-IMAGES.md](../BUILD-IMAGES.md) 讲构建环境；runtime base 必须来自同一个
  builder 镜像 digest。
- [BUILD.md](../BUILD.md) 的 `$ORIGIN` 相对 rpath 是镜像里整树平移的前提；
  [RPATH.md](RPATH.md) 是同一个 rpath 的另一种后端方案，不是前提。
- [CONTRACT.md](../CONTRACT.md) / [PRODUCT.md](../PRODUCT.md) 的 profile 字段表要加
  `image`。
