---
name: verify-pr
description: PRのSDLCコンプライアンスを検証する
user_invocable: true
---

# verify-pr

指定されたPRをlibverifyの28コントロールで検証し、ポリシーに基づいてpass/review/failを判定します。

## Usage

`/verify-pr owner/repo#123` または `/verify-pr owner repo 123`

## Instructions

1. ユーザーの入力から owner, repo, pr_number を抽出する
2. policyが指定されていなければ "default" を使用する
3. MCP tool `verify_pr` を呼び出す（MCPサーバー: https://metsuke.fly.dev/mcp）
4. 結果をサマリーとして表示する:
   - pass/review/fail の件数
   - failしたコントロールのID、rationale
   - 改善アクションの提案
