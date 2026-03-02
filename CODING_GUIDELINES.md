# コーディング規約

## コメント
- 日本語で書く
- 自明なコードにはコメントを書かない。ロジックの意図が分かりにくい箇所にのみ記載する

## エラーメッセージ
- APIレスポンスのエラーメッセージは英語（例: `"username is empty"`）

## unwrap() の使用
- 原則 `expect("理由")` を使い、パニックの理由を明記する
- Mutex の `lock()` など、失敗しないことが自明な場合のみ `unwrap()` を許可する

## unsafe
- このプロジェクトでは使用禁止

## use 文の整理
- 外部クレート → `crate::` 内部モジュールの順に並べる
- グループ間に空行を入れる
- 同一クレートからの複数インポートはネストしてまとめる

## フォーマット
- `rustfmt` に従う
- 関数の引数が多い場合は引数ごとに改行する

## 型アノテーション
- コンパイラが推論できる場合は省略する

## トレイト実装の配置
- 構造体と同じファイルに置く（例: `ApiError` の `IntoResponse` は `errors.rs` に）

## SQL
- `sqlx::query` / `query_as` に生 SQL を直接書く
- `SELECT *` は使わず、必要なカラムを明示的に指定する

## エラーハンドリング
- ハンドラーの戻り値は `Result<T, ApiError>` で統一する
- ライブラリエラーは `.map_err(|err| ApiError::Internal(err.to_string()))` で変換する
- バリデーションエラーは `.map_err(|err| ApiError::BadRequest(err))` で変換する

## ログレベルの使い分け
- `tracing::error!` — 500系の内部エラー
- `tracing::warn!` — 認証失敗・不正リクエスト等
- `tracing::info!` — サーバー起動・DB接続成功等
- `tracing::debug!` — 開発時のデバッグ情報

## コミットメッセージ
- Conventional Commits の prefix + 日本語の説明
- 例: `feat: ログインハンドラー実装`、`fix: バリデーションの修正`

## テスト
- 単体テストは同じファイル内の `#[cfg(test)] mod tests` に書く
- 結合テストは `tests/` ディレクトリに置く
