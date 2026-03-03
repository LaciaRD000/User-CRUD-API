# ToDo

DESIGN.md の実装ステップに基づくタスク管理。

## Phase 1: CRUD API (Step 1-11)

- [x] 1. Supabase セットアップ
- [x] 2. `Cargo.toml` 依存クレート追加
- [x] 3. `.env` 設定
- [x] 4. `models/user.rs` + `models/mod.rs`
- [x] 5. `snowflake.rs`
- [x] 6. `db.rs`
- [x] 7. `state.rs`
- [x] 8. `errors.rs`
- [x] 9. `validation.rs`
- [x] 10. `routes/users.rs` + `routes/mod.rs`
- [x] 11. `main.rs`

## Phase 2: JWT 認証 + 認可 + リフレッシュトークン (Step 12-24)

- [x] 12. Supabase — `ALTER TABLE users` + `CREATE TABLE refresh_tokens`
- [x] 13. `Cargo.toml` — jsonwebtoken, bcrypt, chrono, uuid 追加
- [x] 14. `.env` — `JWT_SECRET`, `ACCESS_TOKEN_EXPIRY_MINUTES`, `REFRESH_TOKEN_EXPIRY_DAYS` 追加
- [x] 15. `models/user.rs` + `models/auth.rs` — 認証関連構造体を auth.rs に分離, RefreshToken 追加, password_hash に `#[sqlx(default)]`
- [x] 16. `models/mod.rs` — auth モジュール追加, 再エクスポート更新
- [x] 17. `errors.rs` — Unauthorized + Forbidden 追加
- [x] 18. `validation.rs` — validate_password 追加
- [x] 19. `state.rs` — jwt_secret, access_token_expiry_minutes, refresh_token_expiry_days 追加
- [x] 20. `auth.rs` — Claims, create_token, validate_token, FromRequestParts 実装
- [x] 21. `routes/auth.rs` — register, login, refresh, logout, issue_refresh_token 実装完了
- [x] 22. `routes/mod.rs` — `pub mod auth;` 追加
- [x] 23. `routes/users.rs` — create_user 削除済み, 認可チェック追加, `SELECT *` を明示的カラム指定に変更
- [x] 24. `main.rs` — JWT_SECRET 読み込み, AppState::new 引数追加, POST /users 削除済み, auth ルート追加, CORS に allow_headers 追加

## Phase 3: セキュリティ強化 (Step 25-27)

- [x] 25. `Cargo.toml` — `tower_governor` 0.8 を追加
- [x] 26. `routes/auth.rs` — `login` でリフレッシュトークン発行前に既存トークンを全削除
- [x] 27. `main.rs` — `GovernorLayer` を認証・ユーザールートそれぞれに適用, `into_make_service_with_connect_info::<SocketAddr>()` に変更

## Phase 4: ミドルウェア追加

- [ ] `tower-http` の `CompressionLayer` — レスポンスの gzip 圧縮
- [ ] `axum::extract::DefaultBodyLimit` — リクエストボディのサイズ上限設定
- [ ] `tower-http` の `TimeoutLayer` — リクエスト処理のタイムアウト設定
- [ ] `tower-http` の `NormalizePathLayer` — 末尾スラッシュの正規化（`/users/` → `/users`）
- [ ] `tower-helmet` の `SecureHeaders` — セキュリティヘッダー一括設定（HSTS 等）

## コーディング規約の残タスク

- [x] `routes/users.rs` の SQL で `SELECT *` / `RETURNING *` を明示的カラム指定に変更 (Step 23 に含まれる)
