# phprt

## 项目介绍

phprt 类似 Python 的 uv，用一条命令为 PHP 项目配置一套隔离系统环境的 PHP 运行环境。

仓库只做软件源，装好的东西全部放在项目自己的 `.phprt/` 里：PHP、依赖库、扩展、源码缓存
都是项目私有的，不往系统目录写文件，不同项目互不影响。运行时不需要设置任何环境变量，
整个 `.phprt` 可以跟着项目一起搬走。

## 使用示例

```bash
cd /path/to/project

# 首次：选版本和扩展，构建到项目的 .phprt/
/path/to/phprt/phprt set 8.5.12 --ext "openssl zlib libxml2 curl sqlite mysqli mbstring zip redis" --yes

# 使用
./myphp -v
./myphp -m
./myphp worker.php

# 想直接敲 php / composer，把环境导进当前 shell
eval "$(./myphp --shell)"
```

```bash
# 看计划，不构建
phprt set 8.5.12 --ext "sqlite redis" --plan

# 切版本（同一项目可以有多套）
phprt set 8.6.0-dev --ext "openssl zlib sqlite"
phprt current
phprt list

# 加/去一个 PECL 扩展：只编它，PHP 不动
phprt add ext yaml
phprt remove ext yaml

# 自检
phprt verify

# 强制重编 PHP / 清源码重来
phprt set 8.5.12 --recompile
phprt set 8.5.12 --clean-src
```

支持 `php-8.5.12`、`php-8.6.0-dev`、`php-7.4.33`（7.4 目前只有最小档）。

## 特点

**1. wheel 方式管理动态库。** 依赖库的名字加内容 hash、SONAME 同步改写，所有消费方的
`DT_NEEDED` 一起改，RPATH 写成 `$ORIGIN` 相对路径。于是同名库不会和系统里的抢，
运行时不用 `LD_LIBRARY_PATH`，整棵树搬走或改名都能跑。

**2. 依赖按 builtin / deps / ext / tools 分类。** 内置扩展、第三方库、PECL 扩展、构建工具
各自独立 recipe、独立签名、按依赖拓扑排序，把系统依赖隔离在项目内，只使用树里的库。

**3. downloads 支持离线。** 可以提前把源码包放进 `downloads/`，构建时直接硬链接到项目，
不联网；缺哪个才按 URL 下载。

## 文档

- [运行时库的 wheel 化](docs/WHEEL-LIBS.md)
- [给 bundle 加扩展或依赖](docs/ADD-EXTENSION.md)
- [从 v1 重构到 v2](docs/REFACTOR-V1-TO-V2.md)
- [项目本地自包含模型](docs/PROJECT-LOCAL.md)
- [多 PHP 版本的配置组织](docs/PHP-VERSIONS.md)
