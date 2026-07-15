#!/bin/sh
set -eu

workspace=$PWD
if test -f "$workspace/yanxu-platform/Cargo.toml"; then
  root="$workspace/yanxu-platform"
elif test -f "$workspace/Cargo.toml" && test -f "$workspace/言序.toml.in"; then
  root="$workspace"
else
  echo "请从言序多仓工作区根目录运行 yanxu-platform/scripts/prepare-current.sh" >&2
  exit 1
fi
target=$(rustc -vV | sed -n 's/^host: //p')
case "$target" in
  *-apple-darwin) os=macos ;;
  *-pc-windows-*) os=windows ;;
  *-unknown-linux-*) os=linux ;;
  *) echo "不支持的构建目标：$target" >&2; exit 1 ;;
esac
case "$target" in
  aarch64-*) arch=arm64 ;;
  x86_64-*) arch=x64 ;;
  *) echo "不支持的构建架构：$target" >&2; exit 1 ;;
esac
case "$os" in
  windows) source="$root/target/release/yanxu_platform_native.dll" ;;
  macos) source="$root/target/release/libyanxu_platform_native.dylib" ;;
  linux) source="$root/target/release/libyanxu_platform_native.so" ;;
esac
test -f "$source" || {
  echo "缺少后端制品；先从 $workspace 运行 cargo build --manifest-path yanxu-platform/Cargo.toml --release" >&2
  exit 1
}
mkdir -p "$root/dist/$target"
file="$root/dist/$target/$(basename -- "$source")"
temporary="$file.tmp.$$"
trap 'rm -f "$temporary"' EXIT HUP INT TERM
cp "$source" "$temporary"
chmod a-w "$temporary"
mv -f "$temporary" "$file"
trap - EXIT HUP INT TERM
if command -v shasum >/dev/null 2>&1; then
  checksum=$(shasum -a 256 "$file" | awk '{print $1}')
elif command -v sha256sum >/dev/null 2>&1; then
  checksum=$(sha256sum "$file" | awk '{print $1}')
else
  echo "缺少 SHA-256 工具（shasum 或 sha256sum）" >&2
  exit 1
fi
size=$(wc -c < "$file" | tr -d ' ')
cp "$root/言序.toml.in" "$root/言序.toml"
{
  printf '[原生.%s.%s]\n' "$os" "$arch"
  printf '文件 = "dist/%s/%s"\n' "$target" "$(basename -- "$file")"
  printf '校验和 = "%s"\n' "$checksum"
  printf '大小 = %s\n' "$size"
} >> "$root/言序.toml"
echo "已生成 ${root}/言序.toml（${target}，${checksum}）"
