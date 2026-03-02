# CLAUDE.md

## Project Overview
Rust製のユーザー管理 REST API。Axum + Tokio で構築し、Supabase (PostgreSQL) でデータを永続化する。

## Tech Stack
- Rust (Edition 2024)
- Axum 0.8 (Web framework)
- Tokio (Async runtime)
- sqlx 0.8 (PostgreSQL client)
- Serde (JSON serialization)
- tower-http 0.6 (CORS, tracing middleware)
- jsonwebtoken 10 (JWT 生成・検証)
- bcrypt 0.18 (パスワードハッシュ)
- chrono 0.4 (日時操作)
- uuid 1 (リフレッシュトークン生成)
- Snowflake ID (自前実装)

## Commands
- `cargo build` — ビルド
- `cargo run` — サーバー起動 (localhost:3000)
- `cargo test` — テスト実行
- `cargo clippy` — リント
- `cargo fmt --check` — フォーマットチェック

## Project Structure
- `src/main.rs` — エントリーポイント、ルーティング、ミドルウェア設定
- `src/auth.rs` — JWT コアロジック (create_token, validate_token, FromRequestParts)
- `src/db.rs` — DB接続プール作成
- `src/state.rs` — AppState (PgPool + SnowflakeGenerator + JWT設定)
- `src/snowflake.rs` — Snowflake ID生成器
- `src/errors.rs` — ApiError enum (NotFound, BadRequest, Unauthorized, Forbidden, Internal)
- `src/validation.rs` — バリデーション関数 (username, email, password)
- `src/models/` — User, UpdateUser, RegisterUser, LoginUser, AuthResponse 等
- `src/routes/auth.rs` — register / login / refresh / logout ハンドラー
- `src/routes/users.rs` — ユーザー CRUD ハンドラー

## Environment Variables (.env)
- `DATABASE_URL` — PostgreSQL 接続文字列（必須）
- `JWT_SECRET` — JWT 署名用シークレット、32文字以上（必須）
- `ACCESS_TOKEN_EXPIRY_MINUTES` — アクセストークン有効期限（分）
- `REFRESH_TOKEN_EXPIRY_DAYS` — リフレッシュトークン有効期限（日）
- `RUST_LOG` — ログレベル制御（例: `info`）

## Design Document
詳細な設計は `DESIGN.md` を参照。

## ToDo
実装の進捗管理は `ToDo.md` を参照。GitHub にプッシュする際は、必ず `ToDo.md` の進捗を最新の状態に更新すること。

## Conventions
- 日本語でコミュニケーションする
- ユーザーは Rust 初心者のため、丁寧に解説する
- コードを勝手に書かず、設計図 (DESIGN.md) をベースに学習者自身が実装する
- nvim + rust-analyzer を使用
- 一つの機能の実装が完了したら、コミットして GitHub にプッシュする

## 禁止事項
- `.env*` ファイルをユーザーの許可なく読み取らないこと（シークレット・認証情報を含むため）

## コーディング規約

### コメント
- 日本語で書く
- 自明なコードにはコメントを書かない。ロジックの意図が分かりにくい箇所にのみ記載する

### エラーメッセージ
- APIレスポンスのエラーメッセージは英語（例: `"username is empty"`）

### unwrap() の使用
- 原則 `expect("理由")` を使い、パニックの理由を明記する
- Mutex の `lock()` など、失敗しないことが自明な場合のみ `unwrap()` を許可する

### unsafe
- このプロジェクトでは使用禁止

### use 文の整理
- 外部クレート → `crate::` 内部モジュールの順に並べる
- グループ間に空行を入れる
- 同一クレートからの複数インポートはネストしてまとめる

### フォーマット
- `rustfmt` に従う
- 関数の引数が多い場合は引数ごとに改行する

### 型アノテーション
- コンパイラが推論できる場合は省略する

### トレイト実装の配置
- 構造体と同じファイルに置く（例: `ApiError` の `IntoResponse` は `errors.rs` に）

### SQL
- `sqlx::query` / `query_as` に生 SQL を直接書く
- `SELECT *` は使わず、必要なカラムを明示的に指定する

### エラーハンドリング
- ハンドラーの戻り値は `Result<T, ApiError>` で統一する
- ライブラリエラーは `.map_err(|err| ApiError::Internal(err.to_string()))` で変換する
- バリデーションエラーは `.map_err(|err| ApiError::BadRequest(err))` で変換する

### ログレベルの使い分け
- `tracing::error!` — 500系の内部エラー
- `tracing::warn!` — 認証失敗・不正リクエスト等
- `tracing::info!` — サーバー起動・DB接続成功等
- `tracing::debug!` — 開発時のデバッグ情報

### コミットメッセージ
- Conventional Commits の prefix + 日本語の説明
- 例: `feat: ログインハンドラー実装`、`fix: バリデーションの修正`

### テスト
- 単体テストは同じファイル内の `#[cfg(test)] mod tests` に書く
- 結合テストは `tests/` ディレクトリに置く
