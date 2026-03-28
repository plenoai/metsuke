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

### レスポンス構造

全ツール共通で `VerificationResult` を返す:

- `profile_name` — 適用されたポリシー名
- `findings[]` — コントロールごとの評価結果
  - `control_id` — コントロールID（例: `SLSA-L2-SCM-1`）
  - `status` — `Satisfied` | `Violated` | `Indeterminate` | `NotApplicable`
  - `rationale` — 判定理由
  - `subjects[]` — 対象エンティティ（ファイルパス、コミットSHAなど）
  - `evidence_gaps[]` — エビデンス収集の欠損（`CollectionFailed`, `Truncated`, `MissingField`, `DiffUnavailable`, `Unsupported`）
- `outcomes[]` — ポリシーに基づくゲート判定
  - `control_id` — コントロールID
  - `severity` — `Info` | `Warning` | `Error`
  - `decision` — `Pass` | `Review` | `Fail`
  - `rationale` — 判定理由
- `severity_labels` — severityの表示ラベル
- `evidence`（オプション） — 生エビデンスバンドル

### PR検証 (`pr`)

1. ユーザーの入力から owner, repo, pr_number を抽出する
2. MCP tool `verify_pr` を呼び出す
3. 結果をサマリーとして表示する:
   - `profile_name` と decision ごとの件数（Pass / Review / Fail）
   - Fail/Review の `control_id`、`severity`、`rationale`
   - `evidence_gaps` があれば欠損情報を補足
   - 改善アクションの提案

### リリース検証 (`release`)

1. ユーザーの入力から owner, repo, base_tag, head_tag を抽出する
2. MCP tool `verify_release` を呼び出す
3. 結果をサマリーとして表示する:
   - `profile_name` と decision ごとの件数（Pass / Review / Fail）
   - Fail/Review の `control_id`、`severity`、`rationale`
   - `findings` の `status` が `Violated` / `Indeterminate` のコントロール詳細

### リポジトリ検証 (`repo`)

1. ユーザーの入力から owner, repo, reference (デフォルト: HEAD) を抽出する
2. MCP tool `verify_repo` を呼び出す
3. 結果をサマリーとして表示する:
   - `profile_name` と decision ごとの件数（Pass / Review / Fail）
   - `findings` から依存性の署名検証状態（`status` と `subjects`）
   - Fail/Review の `control_id`、`severity`、`rationale`
