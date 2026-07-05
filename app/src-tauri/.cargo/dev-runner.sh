#!/bin/sh
# cargo runner(仅 dev):启动前用稳定身份给二进制签名,再原样执行。
#
# 背景:macOS TCC 对裸二进制按「路径 + 代码签名」记账。ad-hoc 签名的哈希每次
# 重编都变 → 系统音频录制授权在每次 Rust 重编后失效,且因「路径相同、签名不符」
# 被判防替换:静默拒绝 + 不再弹授权框(设置里残留无效的 echoless-app 条目)。
# 用自签证书签名后,TCC 记「identifier + 证书」,重编不再失效。
#
# 一次性准备(可选;没有证书时本脚本什么都不做):
#   钥匙串访问 → 证书助理 → 创建证书…
#   名称: Echoless Dev   身份类型: 自签名根证书   证书类型: 代码签名
IDENTITY="Echoless Dev"
BIN="$1"
shift
if [ "$(uname)" = "Darwin" ] && security find-identity -v -p codesigning 2>/dev/null | grep -q "$IDENTITY"; then
    codesign --force --sign "$IDENTITY" "$BIN" 2>/dev/null || true
fi
exec "$BIN" "$@"
