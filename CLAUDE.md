# CLAUDE.md

## Project Overview
Rust製のユーザー管理 REST API。Axum + Tokio で構築し、Supabase (PostgreSQL) でデータを永続化する。

## Tech Stack
- Rust (Edition 2024)
- Axum 0.8 (Web framework)
- Tokio (Async runtime)
- sqlx (PostgreSQL client)
- Serde (JSON serialization)
- tower-http (CORS, tracing middleware)
- Snowflake ID (自前実装)

## Project Structure
- `src/main.rs` — エントリーポイント、ルーティング、ミドルウェア設定
- `src/db.rs` — DB接続プール作成
- `src/state.rs` — AppState (PgPool + SnowflakeGenerator)
- `src/snowflake.rs` — Snowflake ID生成器
- `src/errors.rs` — ApiError enum (NotFound, BadRequest, Internal)
- `src/validation.rs` — バリデーション関数
- `src/models/` — User, CreateUser, UpdateUser 構造体
- `src/routes/` — CRUD ハンドラー

## Design Document
詳細な設計は `DESIGN.md` を参照。

## Conventions
- 日本語でコミュニケーションする
- ユーザーは Rust 初心者のため、丁寧に解説する
- コードを勝手に書かず、設計図 (DESIGN.md) をベースに学習者自身が実装する
- nvim + rust-analyzer を使用
