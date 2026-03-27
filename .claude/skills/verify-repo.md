---
name: verify-repo
description: リポジトリの依存性署名を検証する
user_invocable: true
---

# verify-repo

指定されたgit refにおけるリポジトリの依存性署名を検証します。

## Usage

`/verify-repo owner/repo` または `/verify-repo owner/repo@ref`

## Instructions

1. ユーザーの入力から owner, repo, reference (デフォルト: HEAD) を抽出する
2. policyが指定されていなければ "default" を使用する
3. MCP tool `verify_repo` を呼び出す（MCPサーバー: https://metsuke.fly.dev/mcp）
4. 結果をサマリーとして表示する:
   - 依存性の署名検証状態
   - pass/review/fail の件数
