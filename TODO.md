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

- [x] `tower-http` の `CompressionLayer` — レスポンスの gzip 圧縮
- [x] `axum::extract::DefaultBodyLimit` — リクエストボディのサイズ上限設定
- [x] `tower-http` の `TimeoutLayer` — リクエスト処理のタイムアウト設定
- [x] `tower-http` の `NormalizePathLayer` — 末尾スラッシュの正規化（`/users/` → `/users`）
- [x] `tower-helmet` の `HelmetLayer` — セキュリティヘッダー一括設定（HSTS 等）

## Phase 5: ユニットテスト

- [x] `snowflake.rs` — ID の正値・一意性・単調増加・machine_id 分離 (4 tests)
- [x] `validation.rs` — username/email/password の正常系・異常系・境界値 (12 tests)
- [x] `errors.rs` — 全 ApiError バリアントのステータスコード・JSON ボディ検証 (5 tests)
- [x] `auth.rs` — トークン往復・不正シークレット・期限切れ・ゴミ入力・sub 一致・iat<exp (6 tests)

## コーディング規約の残タスク

- [x] `routes/users.rs` の SQL で `SELECT *` / `RETURNING *` を明示的カラム指定に変更 (Step 23 に含まれる)

## Phase 6: 今後の課題（未実装）

- [x] 28. `GET /users` にページネーションを追加（`limit` / `after_id` クエリ、既定値と上限のバリデーションを含む）
- [x] 29. リフレッシュトークンの保存方式をハッシュ化に変更（`refresh_tokens` はハッシュ値を保持し、`refresh` / `logout` はハッシュ照合）

## Phase 7: セキュリティ改善（実施済み）

- [x] 30. `.env` — `SNOWFLAKE_MACHINE_ID` を必須化（未設定/不正なら起動失敗）
- [x] 31. `.env` — `REFRESH_TOKEN_PEPPER` を追加し、リフレッシュトークンハッシュを `HMAC-SHA256` 化
- [x] 32. `.env` — `JWT_ISSUER` / `JWT_AUDIENCE` を追加し、JWT の発行/検証で `iss` / `aud` を必須化
- [x] 33. レート制限 — `SmartIpKeyExtractor` に切り替え（`Forwarded`/`X-Forwarded-For`/`X-Real-Ip` 優先）
- [x] 34. 重複email登録 — UNIQUE違反を `users_email_key` のみ 409 Conflict にマッピング

## Phase 8: セキュリティ改善（これから）

- [x] 35. email 正規化（`trim` + `lowercase`）とDB側の case-insensitive UNIQUE（`citext` または `unique(lower(email))`）
- [ ] 36. login のタイミング差対策（email不存在でも bcrypt verify を走らせる）
- [x] 37. JWT 検証の追加強化（`JWT_LEEWAY_SECONDS`/必須claim/`JWT_SECRET` 長チェックの明示）
