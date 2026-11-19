# 容器化构建与 OCI 镜像

**本文是待实施的方案，代码尚未实现。** 实施后按第 12 节的清单同步 README、
CLI.md、BUILD.md、CONTRACT.md 和 DESIGN.md；本文与代码冲突时以代码实际行为为准。

## 1. 现在的缺口

构建直接跑在宿主机上，产物是 `.phprt/<产品>-<版本>/` 一棵树。它已经能跑，
但有两个缺口：

- **构建依赖宿主机**。换一台机器、上 CI，就要重新装 gcc / make / autoconf /
  perl / patchelf。`tools` 能检查缺什么，但不能把环境带过去。
- **树不能直接交付**。用户要手写 Dockerfile、自己拷库、自己试缺哪个 `.so`。

目标与 README「目标」一节一致：从一份声明直接得到能跑的最小镜像，而且 build
结束就能告诉用户下一条命令是什么。

现有的可搬移性是这件事的地基：运行时库已经收进 `lib/phprt`，rpath 全部是
`$ORIGIN` 相对路径（[RPATH.md](RPATH.md)），树整体平移不用重编。

## 2. 两个依赖，不要混

- **容器化构建**需要一个容器运行时来执行命令（podman / docker）。这是外部命令
  依赖，性质和 gcc 一样，不是可选项。
- **打包镜像**不需要 daemon 来做 `docker build`。用 Rust 的 `oci-spec` 出类型，
  `tar` + `flate2` 写层，`sha2` 算 digest。

第一件是执行环境，第二件是文件格式。混在一起会得出「用 Rust 写 OCI 所以不需要
容器」这种错误结论。

不过要补一句限定：首次出镜像仍要能读到 builder 镜像的内容（裁 runtime base、
做探针）。这一步可以起一次临时容器，也可以从 registry 取；结果按 digest 缓存后，
后续出镜像完全离线。所以准确的说法是：**不需要 daemon 来构建镜像，但第一次要能
访问那个镜像**。

## 3. 构建镜像是一条线

镜像不是写死的常量，而是和补丁号一样的数据：catalog 存、recipe 声明要求、
bundle 给默认、state 记实际选择。产品 recipe 里永远不出现 `debian:12`。

宿主构建不需要这一节；只有 `--image <线名>`（或 `build` 出镜像）时才用到。

### 3.1 catalog：`recipes/images.toml`

```toml
# 只登记验证过的镜像；digest 必须写，tag 会漂。
[images.debian12]
image = "docker.io/library/debian:12"
digest = "sha256:..."
pm     = "apt"
gcc    = "12.2"
glibc  = "2.36"

[images.fedora43]
image = "registry.fedoraproject.org/fedora:43"
digest = "sha256:..."
pm     = "dnf"
gcc    = "15.3.1"
glibc  = "2.42"

[images.rocky8]
image = "quay.io/rockylinux/rockylinux:8"
digest = "sha256:..."
pm     = "dnf"
gcc    = "8.5"
glibc  = "2.28"

# 工具名 -> 发行版包名，按包管理器分组。只写需要在镜像里自动安装的工具；
# 没写的名字认为基础镜像自带，装完由常规 tools 检查兜底。
# ld 现在不由 toolchain 声明；如果某个 recipe 把 ld 写进 tools，就按这里装。
[package_maps.apt]
gcc        = "gcc"
gxx        = "g++"
ld         = "binutils"
make       = "make"
perl       = "perl"
m4         = "m4"
pkg-config = "pkg-config"
patchelf   = "patchelf"
autoconf   = "autoconf"

[package_maps.dnf]
gcc        = "gcc"
gxx        = "gcc-c++"
ld         = "binutils"
make       = "make"
perl       = "perl"
m4         = "m4"
pkg-config = "pkgconf"
patchelf   = "patchelf"
autoconf   = "autoconf"
```

- 一个条目就是一条线，名字是 `debian12` / `fedora43` 这种稳定标识，和补丁号分开。
- `digest` 必填。可复现、签名、探针缓存都按它索引。
- `gcc` / `glibc` **手写 + 首次使用时探针校验**，不一致就报错，不静默采信。
  理由：纯探针生成的话，加一条镜像必须联网拉镜像，`--offline` 就用不了；
  纯手写不校验的话，tag 漂了或写错了，错误会拖到编译中途才暴露。
- **`gcc` 是唯一参与构建判断的版本**。`glibc` 只用于显示和运行下限提示，
  选镜像时告诉用户「产物要求目标系统 glibc >= 这个值」，不参与构建是否可用的判断。
- `pm` 只支持 `apt` / `dnf`；其它包管理器报错，不做通用抽象。
- 旧条目保留规则同 catalog 补丁：升级某条线的 digest 时把旧条目留在文件里，
  `--lock` 只从这里取。
- 用户加镜像 = 在这里加一段并跑一次探针校验；加错、验不过由用户自己处理，
  仓库只对自带的条目负责。

`phprt list_oci_images` 打印这张表：

```text
$ phprt list_oci_images
线名      镜像                                  GCC     glibc  包管理
debian12  docker.io/library/debian:12          12.2    2.36   apt
fedora43  registry.fedoraproject.org/fedora:43  15.3.1  2.42   dnf
rocky8    quay.io/rockylinux/rockylinux:8      8.5     2.28   dnf
```

表下一行固定说明：**glibc 是产物能运行的最低 glibc，不参与构建判断。**

### 3.2 产品声明：只声明 gcc / g++

选镜像的硬条件只有 `toolchain` 里的 gcc / g++ 区间：

```python
TOOLCHAIN = toolchain(
    gcc = [
        toolchain_range(min = "4.8", max = "13"),
        toolchain_range(
            min = "14",
            cflags = "-std=gnu17 -Wno-error=incompatible-pointer-types",
        ),
    ],
    gxx = [
        toolchain_range(min = "4.8", max = "13"),
        toolchain_range(min = "14"),
    ],
)

tools = [
    tool("make", min = "4.0"),
    tool("perl", min = "5.20"),
    tool("m4", min = "1.4"),
    tool("pkg-config", min = "0.29"),
    tool("patchelf", min = "0.13"),
]
```

- gcc / g++（`gxx`）只写在 `toolchain`，不再写在 `tools`（见 [TOOLS.md](TOOLS.md)）。
- **不声明 glibc**。glibc 与编译无关，只决定产物能不能在目标系统上跑；
  它的值来自所选镜像的 catalog 条目，选镜像时展示给用户。
- dep / component 默认不声明 gcc；确实有已知不兼容时再写一条收窄约束，
  只求交、不带 flags（TOOLS.md §4.2）。
- `toolchain_range` 只管 gcc / gxx；`tool()` 的 `max` 是给 autoconf 这类工具版本
  用的，两者不混用。

### 3.3 bundle 默认

默认线写在 bundle，不靠「catalog 里恰好剩一条」隐式选：

```toml
[image]
line = "fedora43"
```

放在 `[image]` 表里，不占 bundle 顶层：顶层裸键已经用来写有独立版本线的组件，
不能和它抢名字空间。

### 3.4 state 与签名

安装成功后记录实际选择：

```json
"image": {
  "line": "debian12",
  "image": "docker.io/library/debian:12",
  "digest": "sha256:...",
  "gcc": "12.2",
  "glibc": "2.36"
}
```

- 新增可选字段，旧 state 没有时按 bundle 默认处理，schema 版本不升。
- 构建签名新增两项：镜像的 **digest** 和 `build_backend`。
  用 digest 不用线名，因为同一行的 tag 会漂；glibc 不单独进签名，digest 已经覆盖。
- 影响：签名输入变了，已有安装树在升级后会判定签名变化并重编一次。
  这是必要的一次性成本，切换后端却不重编会拿到另一套 libc 编出来的产物。
- `--lock` 复用 state 里的镜像 digest。该 digest 在 catalog 里查不到时报错，
  不偷偷换回当前线，规则同 [LOCK.md](LOCK.md)。

### 3.5 求解与优先级

和现有五层优先级一致：catalog < recipe < bundle < state < CLI。

1. **默认**：宿主构建（`build_backend = "local"`），不选镜像。
   `build_backend = "container"` 或给了 `--image` 时进入镜像模式。
2. **指定**：`--image <线名>`。只查 `recipes/images.toml`；查不到就报错，
   并提示 `phprt list_oci_images`。
3. **`build` 出镜像**：没给 `--image` 就用 bundle 的 `[image].line`；
   没有默认线时报错。
4. **gcc 匹配**：产品声明了 `toolchain` 时，用镜像条目的 `gcc` 匹配区间：
   - 匹配：继续，按选中档位合并 CFLAGS；
   - 不匹配：报错，写清镜像 gcc、产品要求的区间，并列出 catalog 里满足的线。

   产品没有 `toolchain` 声明时跳过这一条。
5. **glibc 只提示**：

   ```text
   镜像 debian12：gcc 12.2，glibc 2.36
   产物要求运行系统 glibc >= 2.36
   ```

   不阻塞、不需要 `--force`。用户选这个镜像就代表接受这个运行下限。

第一版不做“拿任意镜像名直接构建”的兜底，也不做自动挑线；`--image` 只认 catalog
里的线名。将来要自动选时，仍然用同一套 gcc 区间当筛选条件。

## 4. 容器化构建

### 4.1 接缝在命令执行层

[DESIGN.md](DESIGN.md) 第 11 节写明：`crates/build` 只接收「工作目录 + 命令 +
环境 + 产物」，不关心产品。容器化就改这一层：

```toml
# phprt.toml
build_backend = "local"           # local（默认）/ container
container_runtime = "podman"      # 不写时自动探测 podman → docker
```

`local` 保持现在的行为，一行不改。`container` 时每条构建命令包成一次容器执行。
**求解、拓扑、签名、日志格式、日志路径全部不动。** 如果容器化需要动 `plan.rs`
或构建流程，说明接缝选错了。

### 4.2 一次安装 = 一个容器

不要每个节点起一个容器。做法是：`install` / `build` 开始时起一个常驻容器，
项目根 bind mount 进去，所有节点按拓扑序在里面执行，结束销毁。

- `build/`、`.phprt/`、`.phprt/downloads/` 都在挂载里，所以**下载和解压仍然在宿主
  做**。镜像站、代理、分片续传、`--offline` 全部原样复用，不在容器里再实现一遍。
- 容器内用当前 uid/gid 跑（`--user $(id -u):$(id -g)`）。否则宿主上会留下一堆
  root 拥有的文件，下一次本地构建就动不了。
- `tools` 检查照旧在容器内执行。缺 gcc 就报缺 gcc，不跳过。

### 4.3 builder 镜像内容：从 recipe 自动装依赖

镜像本身只保证有基础系统；构建工具按 recipe 自动安装，不再维护一份固定的
builder 包清单。

1. **收集工具名**：`toolchain` 的 `gcc` / `gxx`，加上产品 profile 和各节点
   `tools` 里的全部名字（make、perl、m4、pkg-config、patchelf、autoconf…）。
   同名去重。
2. **映射包名**：用镜像条目的 `pm` 选 `[package_maps.<pm>]`；没有映射的名字
   认为基础镜像自带，不安装。
3. **安装**：起容器后先执行一次
   - apt：`apt-get update && apt-get install -y --no-install-recommends <包...>`
   - dnf：`dnf install -y <包...>`
4. **照常检查**：装完继续跑现有 `tools` 检查。**版本是否满足不由包管理器保证**：
   发行版自带的版本可能低于 `tool.min`，这时按现有逻辑报错，用户换镜像或自己处理。
   不自动降级/升级，也不猜包版本。

- 第一版每次起容器安装一次；按「base digest + 包列表」缓存派生镜像属于后续优化，
  不影响语义。
- 包名映射会随发行版改名、拆包、EOL 腐化，这是数据问题：改
  `recipes/images.toml`，不改 Rust。映射写错的表现为装不上或装完检查失败，
  错误里带包名。
- 镜像里没有对应包、也不自带某个工具：`tools` 检查会报缺什么；用户补映射
  或换镜像，不静默跳过。

### 4.4 运行时依赖

探测 `podman` → `docker`，`phprt.toml` 的 `container_runtime` 可覆盖。
都找不到时在下载源码之前报错：

```text
error: build_backend = "container" 需要 podman 或 docker
```

写法对齐 [RPATH.md](RPATH.md) 第 7 节对 patchelf 的前置检查：失败要快，
不要跑到一半才发现。

### 4.5 容器里编的产物在宿主上不一定能跑

产物的 glibc 下限来自 builder。宿主 glibc 更旧时，`.phprt/.../bin/php` 在宿主上
直接跑会报 `GLIBC_2.xx not found`。这不是 bug，是符号版本决定的：

- 宿主和 builder 同发行版同代际：本地 `myphp` 照常用；
- 宿主比 builder 旧：产物只当镜像素材，本地跑不了；
- 要在旧宿主上跑：用和宿主同代或更旧的 `--image`。

这条要写进 `build` 的说明和报错，否则用户会以为是构建坏了。

## 5. 镜像

### 5.1 入口由 recipe 定

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
  用 `8080:80` 而不是 `80:80`，是因为 rootless podman 绑不了 1024 以下的宿主端口。
- `env` 只放运行时需要的变量，构建期的 `CC` / `PHP_AUTOCONF` 不进来。
- 第一版镜像默认以 root 跑，和官方 PHP / nginx 镜像一致；`user` 字段先不做。
  非 root、`--read-only` 要处理属主和 tmpfs，是另一摊。

几个产品的 entrypoint：

- PHP CLI（选中 `cli` target）：`["{tree}/bin/php"]`，没有端口，不生成 compose；
- PHP-FPM（选中 `fpm` target；镜像支持待实施）：`["{tree}/sbin/php-fpm", "-F"]`，
  `ports = ["9000:9000"]`；
- nginx：`["{tree}/sbin/nginx", "-p", "{tree}", "-c", "{tree}/etc/nginx.conf",
  "-g", "daemon off;"]`；
- MySQL：`["{tree}/bin/mysqld"]`；
- CPython：`["{tree}/bin/python3"]`。

`entrypoint` 是数组不是脚本。要脚本就把脚本文件放进产品目录（和 `entry.sh`
一样），解释器由同一个 recipe 声明带进镜像。带不带 shell 是 recipe 的选择，
工具不替它决定。

注意区分两个「入口」：产品 profile 或选中 target 的 `entry` / `entry_template` 是
**宿主上的开发入口**（`myphp`），不进镜像；`image.entrypoint` 是镜像的入口。

### 5.2 配置：树内默认 + 镜像覆盖层

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

### 5.3 日志与可写目录

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

### 5.4 文件清单：从入口反推

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

### 5.5 runtime base 必须和 builder 同源

树上每个 ELF 的 `NEEDED` 都有 `libc.so.6`，符号版本由构建时那套 glibc 决定。
runtime base 比它旧，二进制直接起不来。

所以默认路径是：**从同一个 builder 镜像裁 runtime base**，只留 glibc 家族、
CA 证书、`/etc/passwd` 之类。裁剪清单向容器里的包管理器要一次（`rpm -ql glibc` / `dpkg -L libc6`），
按 digest 缓存到 `.phprt/images/<digest>.base/`，不手写文件列表；缓存之后
出镜像不需要再访问镜像。

不要另找小镜像：alpine 是 musl，distroless 的 glibc 版本和你的 builder 无关，
两者都是赌。

三种情况：

1. catalog 里的线：照上面的裁，同源自不必说。
2. catalog 外的镜像：先探有没有包管理器。有就照裁；没有（scratch / distroless /
   极度精简）就报错，要求 `--runtime-base <镜像>`，不猜。
3. 显式给了 `--runtime-base`：跳过裁剪，但比对 libc 版本，比 builder 旧就报错。

### 5.6 树放哪

整体平移到 `/opt/<产品>-<版本>/`。rpath 全是 `$ORIGIN` 相对路径，所以**不用重编**；
安装树里也没有绝对路径（`php.ini` 不含 `extension_dir`，那是入口运行时给的）。

镜像的 `Env` 写死绝对路径，入口直接 `exec`，不靠 bash 的 `cd -P` 算位置。
runtime base 因此可以不带 shell。

### 5.7 OCI 输出与压缩

一次构建的产物是一个目录：

```text
.phprt/images/php-8.5.12/
├── image.tar        # OCI image layout
├── manifest.json    # 文件清单，可审计
└── compose.yml      # 只有声明了 ports 才有（见第 6 节）
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

### 5.8 可复现

同样的输入必须得到字节一致的 `image.tar`：

- tar 内按路径排序，`mtime = 0`；
- uid / gid 一律 0；
- mode 归一化：目录和带可执行位的文件 `0755`，其余 `0644`；
- 层 gzip 的头部 `mtime = 0`，压缩级别固定（6）；
- 符号链接按链接写，不跟成实体文件（`lib/phprt` 里全是这种链）。

验收方式是同一输入构建两次，`sha256sum` 相同。

## 6. 运行提示与 compose

build 的终点不是「产出 tar」，是「用户马上能跑」。所以 build 结束时打印下一条
命令，并按需要生成 `compose.yml`。

### 6.1 输出

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

### 6.2 生成规则

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

### 6.3 内容

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

### 6.4 边界

- 不生成 `healthcheck`、`depends_on`、多服务编排、`.env`；
- 一个产品一个服务；php + nginx + fpm 的组合留给用户自己写；
- compose 只认本地镜像，`up` 之前必须先 load，这两步是一对，`--load` 只是压成一条；
- 不生成 Dockerfile。

## 7. 命令与输出

```bash
phprt list_oci_images
phprt build php@8.5
phprt install php@8.4 --image debian12
```

- `list_oci_images` 只读 `recipes/images.toml`，不联网、不拉镜像；输出见 §3.1。
- `install --image <线名>` 用该镜像的容器做构建；镜像的 `gcc` 必须落在产品
  `toolchain` 的区间里，`glibc` 只打印提示。
- `build` 内部走完整安装流程：已装且签名一致就跳过编译，直接出镜像；
  所以没装过的机器上这一条命令就够。
- 给了 `--image` 就按 container 后端跑；没给就按 `phprt.toml` 的
  `build_backend`。显式指定镜像却继续用宿主工具链会让人误解，所以不做「静默忽略」。
- `--image`、`--force` 对 `install` 和 `build` 都生效（容器化构建对两者都适用）；
  `--runtime-base`、`--load`、`--tag` 只对 `build` 生效。
- base 和 manifest 都命中缓存时，出镜像不需要网络，也不需要起容器。
- 默认 tag 是 `<产品>:<补丁>`，例如 `php:8.5.12`；`--tag` 覆盖。
- 不加 `phprt image` / `phprt export` / `phprt run` 之类的第二条命令。

`plan --json` 新增 `image` 段：镜像线、digest、gcc、glibc、自动安装的包列表、
base 裁剪出的文件数、manifest、估算大小、入口、会不会生成 compose。
agent 在真跑之前就能核对。

## 8. 失败模式

- `--image` 的值不在 catalog 里：报错，并提示 `phprt list_oci_images`。
- catalog 里没有满足产品 gcc 区间的线：报错，列出每条线的 gcc 和产品要求的区间。
- catalog 声明和探针不一致（gcc / glibc）：报错，提示 tag 可能漂了或条目写错。
- glibc 比目标系统新：**不报错**，只在选镜像和 `plan` 时提示运行下限。
- 镜像里没有包管理器：`pm` 写错或镜像不对，报错。
- 包映射缺失、包名写错：安装失败或装完 `tools` 检查报缺工具，错误里带包名；
  用户补 `package_maps` 或换镜像。
- runtime base 的 libc 比 builder 旧：报错。
- manifest 校验缺 soname：报错，写清缺哪个、被谁需要。
- 没有容器运行时：下载源码之前报错。
- 镜像写出失败：不写 state 的镜像相关记录，安装状态保留，重跑只重做镜像。
- `compose.yml` 写出失败：镜像 tar 保留，报错提示路径和原因，不静默跳过。

## 9. 不变量

1. 产品 recipe 不写镜像名、base、镜像线；只声明 gcc 要求、入口和配置。
2. 镜像里不放 phprt、不放工具链、不放源码、不放头文件。
3. 镜像 digest 只由输入决定，不由时间、宿主路径、umask 决定。
4. 执行后端只换环境，不改语义；切后端必须重编（签名含 digest）。
5. runtime base 必须与 builder 同源，或显式指定并通过 libc 校验。
6. 镜像文件集合由 manifest 决定，不由人肉列表决定。
7. 镜像配置覆盖只作用于镜像，不改变宿主安装树。
8. `compose.yml` 是生成物，不手写维护；重新 build 覆盖。
9. glibc 不参与构建判断；它只说明产物的运行下限。

## 10. 不在范围

- 推 registry（第一版只出 tar）；
- 多架构、交叉编译（与 DESIGN.md 的非目标一致）；
- 生成 Dockerfile；
- 镜像分层缓存优化（先正确，层共享是后面的事）；
- 派生 builder 镜像的缓存（第一版每次起容器装包，后续再优化）；
- 「产物能在老系统跑」的兼容矩阵；
- `cmd` / `stopsignal` / `healthcheck` 等镜像字段；
- 非 root 运行和 `--read-only` 的属主处理；
- `healthcheck` / `depends_on` / 多服务 compose；
- zstd 压缩、`--compression` 参数；
- 在镜像里放 phprt 或构建工具。

## 11. 落地顺序

每步独立验收，都不依赖下一步：

1. **image catalog + 声明 + 求解**。纯数据处理，不碰容器。验收：
   `phprt list_oci_images` 输出表格、`php@8.5` 用默认线、catalog 里没有满足
   gcc 区间的线时报错并列出缺项、`--image no-such-line` 报错并提示命令。
2. **执行后端 + 探针 + 自动装包**。容器跑通一个产品，签名带上 digest。验收：
   镜像里缺 make/perl 时自动装上，tools 检查通过；包版本低于 `min` 时按现有
   逻辑报错；产物和 `local` 后端都能跑，切后端触发重编。
3. **manifest + 配置覆盖 + 静态校验**。纯宿主侧。验收：缺文件时报错而不是生成
   坏镜像；`image.config` 只改镜像不改树。
4. **OCI tar 输出**。验收：`podman load` 后能跑，两次构建字节一致，层是 gzip。
5. **compose + 运行提示**。验收：`docker compose up` 一条命令起服务，
   `podman logs` 能看到 nginx 的 access log。
6. **`phprt build` 串起来**，入口从 recipe 读。验收：新机器一条命令出镜像，
   并打印下一步命令。

第 1 步可以完全离线做，最容易先对齐语义。

## 12. 实施时的改动清单

| 文件 | 改动 |
| --- | --- |
| `recipes/images.toml` | 新增：镜像 catalog + `[package_maps.apt]` / `[package_maps.dnf]`（第 3.1 节） |
| `src/toolchain.rs` | 新增：读镜像 catalog、按 gcc 区间选线、探针结果缓存 |
| `src/config.rs` | `build_backend` / `container_runtime`，校验文案对齐 `rpath_backend` |
| `src/state.rs` | `image` 可选字段 |
| `src/build.rs` | 签名加入镜像 digest 与后端；命令执行走执行后端 |
| `crates/build/` | 加 `exec` 模块：`Local` / `Container` 两个执行后端 |
| `crates/image/` | 新增：manifest、ELF `NEEDED` 解析、base 裁剪、gzip 层、OCI 写出 |
| `src/image.rs` | 新增：从 profile 的 `image` 声明生成 OCI config 和文件覆盖 |
| `src/compose.rs` | 新增：按 `ports` 生成 `compose.yml` 和运行提示 |
| `src/main.rs` | `build` 子命令、`list_oci_images` 子命令、`--image` / `--runtime-base` / `--load` / `--tag` |
| `src/render.rs` | `plan --json` 的 `image` 段 |
| `recipes/products/*/_profile.star` | `toolchain(...)`、`image(...)`（不再写 `libc`） |
| `recipes/products/*/*.image.conf` / `*.ini` | 镜像配置模板（5.2） |
| `recipes/bundles/*/<系列>.toml` | `[image].line` |
| `Cargo.toml` | `oci-spec`、`tar`、`flate2`、`sha2` |
| 文档 | 本文；DESIGN.md §14；CLI.md 的 `build`、`list_oci_images`、`--image`、退出码；BUILD.md 加「构建环境」一节；CONTRACT.md 的 `toolchain` / `image` 字段与 bundle `[image].line`；PRODUCT.md 的 toolchain 说明；README「目标」一节改成已实现 |

## 13. 验收（实施后）

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets

# 一条命令出镜像（新机器，未装过任何工具链）
cargo run -- build php@8.5
podman load -i .phprt/images/php-8.5.12/image.tar
podman run --rm php:8.5.12 --version
podman run --rm php:8.5.12 -r 'echo PHP_VERSION;'
test ! -e .phprt/images/php-8.5.12/compose.yml   # CLI 不生成 compose

# nginx：一条命令起服务
cargo run -- build nginx@1.29 --load
docker compose -f .phprt/images/nginx-1.29.8/compose.yml up -d
curl -fsS http://127.0.0.1:8080/ | head -1
docker compose -f .phprt/images/nginx-1.29.8/compose.yml logs --tail 5
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

# 日志走 stdout
podman run --rm -d --name phprt-nginx -p 8081:80 nginx:1.29.8
curl -fsS http://127.0.0.1:8081/ >/dev/null
podman logs phprt-nginx | grep -q 'GET /'
podman stop phprt-nginx

# 镜像目录
cargo run -- list_oci_images

# 镜像选择
cargo run -- build php@8.5 --image fedora43
cargo run -- install php@7.4 --image debian12    # gcc 12.2 落在 4.8..13 档
cargo run -- install php@7.4 --image fedora43    # gcc 15.3.1 落在 >=14 档
cargo run -- build php@8.5 --image no-such-line  # 报错：不在 catalog，提示 list_oci_images
# 临时把 7.4 的 toolchain 改成 gcc >= 16，再试：
cargo run -- install php@7.4 --image fedora43    # 报错：gcc 15.3.1 不满足 >=16，列出满足的线

# 计划可校验
cargo run -- plan php@8.5 --json | jq .image

# 切后端重编
# phprt.toml: build_backend = "container"
cargo run -- install php@8.5
```

## 14. 和现有文档的关系

- [DESIGN.md](DESIGN.md) 是设计主入口，本文挂 §14；四层数据模型、不变量、
  非目标以主文档为准。
- [BUILD.md](BUILD.md) 讲现有构建；容器化只是换执行环境，顺序和签名规则不变。
- [RPATH.md](RPATH.md) 的 `$ORIGIN` 相对 rpath 是镜像里整树平移、不进 bash 算路径的前提。
- [CONTRACT.md](CONTRACT.md) 的 profile 字段表要加 `toolchain`、`image`；
  bundle 要加 `[image].line`。
- [TOOLS.md](TOOLS.md) 定义 `toolchain` 只声明 gcc / g++，以及 recipe 级约束的规则。
