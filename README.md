# phprt —— 免 root 的本地 PHP 工具链

从源码编译一套完整的 PHP（含 openssl / curl / sqlite / zip / redis / Composer），
装进安装根目录（默认 `~/.phprt` 或项目内的 `./.phprt`），不需要 root、不动系统目录、
不往 `~/.bashrc` 里塞东西。装完给每个项目一个 `myapp.sh`，`./myapp.sh worker.php` 直接跑。

## 三步

```bash
git clone <repo> ~/src/phprt && cd ~/src/phprt

# 1) 装 PHP（交互：装哪儿 / 哪个版本 / 用哪个 php.ini）
cd ~/myproject && ~/src/phprt/install.sh

# 2) 用
./myapp.sh -v
./myapp.sh worker.php
```

装的时候会问三件事：

1. **装到哪儿** —— `~/.phprt`（全局共用，推荐）或 `./.phprt`（只本项目，删目录即卸载）
2. **PHP 版本** —— 清单在 `lib/versions/`，现在只有 8.5.12
3. **php.ini** —— 默认 / `dev` / `prod` / 已有文件 / 在项目里新建一份

非交互（CI、脚本）：

```bash
./install.sh --yes                                  # 全默认
./install.sh --global --php 8.5.12 --ini dev
./install.sh --local --ini-local --no-app           # 装进 ./.phprt，ini 放项目里
./install.sh --offline                              # 只用 downloads/ 里的包
./install.sh --keep-src                             # 保留 build/ 里的源码（--rebuild 快）
```

装完 `build/` 会被自动清掉（解压出来的源码约 1G）；整个安装根一般是 320M 左右，
其中下载包 96M，删了不影响运行、只是下次要重下。

## 装出来长什么样

```
~/.phprt/
├── bin/myphp-8.5.12        某个版本的入口（设好 PHPRC / LD_LIBRARY_PATH / extension_dir）
├── bin/myphp               默认版本（软链）
├── bin/composer.phar
├── ini/{default,dev,prod}.ini
├── php-8.5.12/             PHP 本体
├── deps/                   依赖库（openssl、zlib、sqlite… 跨版本共享）
├── tools/                  autoconf、perl 核心模块（构建期用）
├── downloads/              下载缓存（可以整目录拷到别的机器）
├── build/                  解压出的源码（可删）
├── logs/                   history.log + 每次构建的日志
└── config.conf             本安装根的完整配置（含 PHP 版本）
```

项目里只有一个文件：

```bash
# myapp.sh —— 本项目固定的版本和 ini，改它就是改本项目的运行环境
PHPRT_ROOT=/home/z/.phprt
PHPRT_PHP=8.5.12
PHPRT_INI=/home/z/.phprt/ini/dev.ini
PHPRT_ARGS=(-d memory_limit=256M)
```

不想用 `myapp.sh` 也行：`./phprt link 8.5.12` 会在当前目录建 `./myphp` 软链。

## 日常命令

```bash
./phprt list              # 已装版本 / 默认版本 / 当前项目用的是哪个
./phprt init --ini dev    # 给当前目录生成 myapp.sh
./phprt link 8.5.12       # 建 ./myphp 软链
./phprt build             # 增量编译（--offline / --force / --only php）
./phprt verify            # 自检：扩展、动态库、搬迁测试
./phprt clean [--all]     # 清 build/（--all 连 downloads/）
```

`install.sh` 也能直接带 build 的参数：`./install.sh --yes --offline --jobs 8`。

## 多版本

`lib/versions/` 里一个文件 = 一个可选版本。加 8.6.0 就在那儿放一份清单，然后：

```bash
./install.sh --php 8.6.0      # 装到同一个安装根，依赖和下载缓存全部复用
./phprt link 8.6.0            # 项目切版本（只换软链 / 改 myapp.sh 里的 PHPRT_PHP）
rm -rf ~/.phprt/php-8.5.12 ~/.phprt/bin/myphp-8.5.12
./build.sh --prune --yes      # 再把没人引用的依赖清掉
```

## 目录说明

```
install.sh          用户入口
phprt               管理命令（list/init/link/build/verify/clean）
build.sh            实际构建调度
config.conf         开关与依赖版本（不含 PHP 版本）
ini/                php.ini 预设
lib/common.sh       路径、下载、stamp
lib/modules/*.sh    每个依赖一个脚本
lib/versions/       PHP 版本清单
lib/myphp.tmpl      版本入口模板
lib/app.tmpl        myapp.sh 模板
lib/verify.sh       自检
downloads/          预下载包（不进 git，见里面的 README）
```

## 几个说明

- **不复制进安装根**：构建脚本只在仓库里，安装根里只有编译产物和生成出来的入口。
- **可搬迁**：`myphp` 按自身位置算路径，整个安装根搬走后仍然能用（`phprt verify` 会测）。
  `php-<版本>/bin/php` 单独拿出去不行，那个要用 `myphp`/`myapp.sh` 起。
- **不碰系统**：没有 root，不写 `/usr/local`，不改 shell 配置；卸载就是删安装根。
- **联网**：默认直连，失败自动走 `http://127.0.0.1:10808`（`PHPRT_PROXY=` 可改）。
