---
name: verify-release
description: "リリースタグ範囲のSDLCコンプライアンスをMetsuke MCPサーバーで検証する。Trigger: リリース検証, verify release"
---

# verify-release

指定されたリリースタグ範囲をlibverifyで検証します。

## Usage

`/verify-release owner/repo v1.0.0..v1.1.0`

## Instructions

1. ユーザーの入力から owner, repo, base_tag, head_tag を抽出する
2. policyが指定されていなければ "default" を使用する
3. MCP tool `verify_release` を呼び出す（MCPサーバー: https://metsuke.fly.dev/mcp）
   - 引数: `{"owner": "...", "repo": "...", "base_tag": "v1.0.0", "head_tag": "v1.1.0", "policy": "default"}`
4. 結果をサマリーとして表示する:
   - pass/review/fail の件数
   - failしたコントロールのID、rationale
