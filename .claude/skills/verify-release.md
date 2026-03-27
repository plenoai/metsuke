---
name: verify-release
description: リリースタグ範囲のSDLCコンプライアンスを検証する
user_invocable: true
---

# verify-release

指定されたリリースタグ範囲をlibverifyで検証します。

## Usage

`/verify-release owner/repo v1.0.0..v1.1.0`

## Instructions

1. ユーザーの入力から owner, repo, base_tag, head_tag を抽出する
2. policyが指定されていなければ "default" を使用する
3. MCP tool `verify_release` を呼び出す（MCPサーバー: https://metsuke.fly.dev/mcp）
4. 結果をサマリーとして表示する:
   - pass/review/fail の件数
   - failしたコントロールのID、rationale
