#!/bin/sh
set -eu

workspace=$PWD
if test -f "$workspace/yanxu-platform/言序.toml"; then
  root="$workspace/yanxu-platform"
elif test -f "$workspace/言序.toml" && test -f "$workspace/Cargo.toml"; then
  root="$workspace"
else
  echo "请从言序多仓工作区根目录或 yanxu-platform 仓库根目录运行" >&2
  exit 1
fi

manifest="$root/言序.toml"
test "$(grep -c '^\[原生\..*\]$' "$manifest")" -eq 6
test "$(find "$root/dist" -type f | wc -l | tr -d ' ')" -eq 6

checksum_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

while read -r target filename; do
  relative="dist/$target/$filename"
  file="$root/$relative"
  test -f "$file"
  checksum=$(checksum_file "$file")
  size=$(wc -c < "$file" | tr -d ' ')
  grep -Fx "文件 = \"$relative\"" "$manifest" >/dev/null
  grep -Fx "校验和 = \"$checksum\"" "$manifest" >/dev/null
  grep -Fx "大小 = $size" "$manifest" >/dev/null
  git -C "$root" ls-files --error-unmatch "$relative" >/dev/null
done <<'TARGETS'
x86_64-unknown-linux-gnu libyanxu_platform_native.so
aarch64-unknown-linux-gnu libyanxu_platform_native.so
x86_64-apple-darwin libyanxu_platform_native.dylib
aarch64-apple-darwin libyanxu_platform_native.dylib
x86_64-pc-windows-msvc yanxu_platform_native.dll
aarch64-pc-windows-msvc yanxu_platform_native.dll
TARGETS

git -C "$root" ls-files --error-unmatch 言序.toml >/dev/null
echo "言台 Git 包含完整且摘要匹配的六目标制品"
