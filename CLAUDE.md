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
- argon2 0.5 (パスワードハッシュ — Argon2id)
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
- `JWT_ISSUER` — JWT の issuer（必須）
- `JWT_AUDIENCE` — JWT の audience（必須）
- `JWT_LEEWAY_SECONDS` — JWT 検証の leeway（秒、未設定時は 60）
- `ACCESS_TOKEN_EXPIRY_MINUTES` — アクセストークン有効期限（分）
- `REFRESH_TOKEN_EXPIRY_DAYS` — リフレッシュトークン有効期限（日）
- `REFRESH_TOKEN_PEPPER` — リフレッシュトークンHMAC用のペッパー（32文字以上を推奨、必須）
- `SNOWFLAKE_MACHINE_ID` — Snowflake 生成器の machine_id（0〜1023 を想定、必須）
- `RUST_LOG` — ログレベル制御（例: `info`）

## Design Document
詳細な設計は `DESIGN.md` を参照。

## ToDo
実装の進捗管理は `TODO.md` を参照。GitHub にプッシュする際は、必ず `TODO.md` の進捗を最新の状態に更新すること。

## Conventions
- 日本語でコミュニケーションする
- ユーザーは Rust 初心者のため、丁寧に解説する
- コードを勝手に書かず、設計図 (DESIGN.md) をベースに学習者自身が実装する
- nvim + rust-analyzer を使用
- 一つの機能の実装が完了したら、コミットして GitHub にプッシュする
- RGBC サイクルを厳守する:
  1. **Red** — 失敗するテストを書く
  2. **Green** — テストを通す最小限のコードを書く
  3. **Blue** — リファクタリングする
  4. **Commit** — コミットする

## 禁止事項
- `.env*` ファイルをユーザーの許可なく読み取らないこと（シークレット・認証情報を含むため）

## コーディング規約
`CODING_GUIDELINES.md` を参照。
