---
name: metsuke
description: "Metsuke MCPサーバーでSDLCコンプライアンスを検証する（PR・リリース・リポジトリ対応）。Trigger: verify pr, verify release, verify repo, コンプライアンスチェック, 依存性チェック"
---

# metsuke

Metsuke MCPサーバー (https://metsuke.fly.dev/mcp) を使い、PR・リリース・リポジトリのSDLCコンプライアンスを検証します。

## Instructions

### Policy

- policyが指定されていなければ "default" を使用する
- 利用可能なポリシー:
  - `default` — デフォルトのSDLCコンプライアンスポリシー
  - `oss` — OSSプロジェクト向けポリシー
  - `aiops` — AI Ops向けポリシー
  - `soc1` — SOC 1準拠ポリシー
  - `soc2` — SOC 2準拠ポリシー（CC7/CC8）
  - `slsa-l1` — SLSA v1.2 Level 1ポリシー
  - `slsa-l2` — SLSA v1.2 Level 2ポリシー
  - `slsa-l3` — SLSA v1.2 Level 3ポリシー
  - `slsa-l4` — SLSA v1.2 Level 4ポリシー（最高レベル）

### PR検証 (`pr`)

1. ユーザーの入力から owner, repo, pr_number を抽出する
2. MCP tool `verify_pr` を呼び出す
3. 結果をサマリーとして表示する:
   - Pass / Review / Fail の件数
   - 不合格コントロールのIDと理由
   - エビデンス収集に欠損があればその旨
   - 改善アクションの提案

### リリース検証 (`release`)

1. ユーザーの入力から owner, repo, base_tag, head_tag を抽出する
2. MCP tool `verify_release` を呼び出す
3. 結果をサマリーとして表示する:
   - Pass / Review / Fail の件数
   - 不合格コントロールのIDと理由
   - リリース間の変更に対するコンプライアンス状態

### リポジトリ検証 (`repo`)

1. ユーザーの入力から owner, repo, reference (デフォルト: HEAD) を抽出する
2. MCP tool `verify_repo` を呼び出す
3. 結果をサマリーとして表示する:
   - Pass / Review / Fail の件数
   - 依存パッケージの署名検証状態
   - 不合格コントロールのIDと理由
