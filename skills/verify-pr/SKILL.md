---
name: verify-pr
description: PRのSDLCコンプライアンスをMetsuke MCPサーバーで検証する。Trigger: PR検証, verify pr, コンプライアンスチェック
---

# verify-pr

指定されたPRをlibverifyの28コントロール(SLSA v1.2 + SOC2)で検証し、ポリシーに基づいてpass/review/failを判定します。

## Usage

`/verify-pr owner/repo#123`

## Instructions

1. ユーザーの入力から owner, repo, pr_number を抽出する
2. policyが指定されていなければ "default" を使用する
3. MCP tool `verify_pr` を呼び出す（MCPサーバー: https://metsuke.fly.dev/mcp）
   - 引数: `{"owner": "...", "repo": "...", "pr_number": 123, "policy": "default"}`
4. 結果をサマリーとして表示する:
   - pass/review/fail の件数
   - failしたコントロールのID、rationale
   - 改善アクションの提案
