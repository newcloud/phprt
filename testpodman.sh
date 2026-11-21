#!/bin/bash
set -euo pipefail

CONTAINER="toolchain-fedora43-build"
IMAGE="toolchain-fedora43"
BASE_IMAGE="quay.io/fedora/fedora:43"
TMP_TAR="/tmp/toolchain-fedora43-flat.tar"

# ---------- 清理旧容器（如果存在） ----------
podman rm -f "$CONTAINER" 2>/dev/null || true

# ---------- 启动长期容器 ----------
echo ">>> 启动 Fedora 43 容器..."
podman run -d --name "$CONTAINER" "$BASE_IMAGE" sleep infinity

# ---------- 换源为清华大学 TUNA 镜像 ----------
echo ">>> 配置国内镜像源 (TUNA)..."
podman exec "$CONTAINER" bash -c '
    sed -e "s|^metalink=|#metalink=|g" \
        -e "s|^#baseurl=http://download.example/pub/fedora/linux|baseurl=https://mirrors.tuna.tsinghua.edu.cn/fedora|g" \
        -i.bak \
        /etc/yum.repos.d/fedora.repo \
        /etc/yum.repos.d/fedora-updates.repo
    dnf makecache
'

# ---------- 定义“拍平提交”函数 ----------
flatten_commit() {
    local step_desc="$1"
    echo ">>> [$step_desc] 导出容器文件系统..."
    podman export "$CONTAINER" -o "$TMP_TAR"

    echo ">>> [$step_desc] 导入为单层镜像 $IMAGE..."
    podman import "$TMP_TAR" "$IMAGE"

    echo ">>> [$step_desc] 清理临时 tar 和悬空镜像..."
    rm -f "$TMP_TAR"
    podman image prune -f
}

# ---------- 第一次：安装 gcc ----------
echo ">>> 第 1 次 dnf install: gcc"
podman exec "$CONTAINER" dnf install -y gcc
podman exec "$CONTAINER" bash -c "dnf clean all && rm -rf /tmp/*"
flatten_commit "第 1 次 gcc"

# ---------- 第二次：安装 perl ----------
echo ">>> 第 2 次 dnf install: perl"
podman exec "$CONTAINER" dnf install -y perl
podman exec "$CONTAINER" bash -c "dnf clean all && rm -rf /tmp/*"
flatten_commit "第 2 次 perl"

# ---------- 第三次：安装 g++ cmake make ----------
echo ">>> 第 3 次 dnf install: g++ cmake make"
podman exec "$CONTAINER" dnf install -y gcc-c++ cmake make
podman exec "$CONTAINER" bash -c "dnf clean all && rm -rf /tmp/*"
flatten_commit "第 3 次 g++ cmake make"

# ---------- 清理构建容器 ----------
echo ">>> 删除构建容器..."
podman rm -f "$CONTAINER"

# ---------- 验证 ----------
echo ">>> 完成。镜像信息："
podman images "$IMAGE"

echo ">>> 验证工具链："
podman run --rm "$IMAGE" bash -c "gcc --version | head -1; g++ --version | head -1; cmake --version | head -1; make --version | head -1; perl --version | head -2 | tail -1"
