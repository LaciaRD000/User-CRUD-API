# user-api

Rust 製のユーザー管理 REST API。Axum + Tokio で構築し、Supabase (PostgreSQL) でデータを永続化する。

## Tech Stack

| クレート | 用途 |
|---------|------|
| Axum 0.8 | Web フレームワーク |
| Tokio | 非同期ランタイム |
| sqlx 0.8 | PostgreSQL クライアント |
| jsonwebtoken 10 | JWT 生成・検証 |
| bcrypt 0.18 | パスワードハッシュ |
| chrono 0.4 | 日時操作 |
| uuid 1 | リフレッシュトークン生成 |
| tower-http 0.6 | CORS, トレースミドルウェア |
| Snowflake ID | ユニーク ID 生成 (自前実装) |

## セットアップ

### 前提条件

- Rust (Edition 2024)
- PostgreSQL (Supabase)

### 手順

1. リポジトリをクローン

```bash
git clone https://github.com/LaciaRD000/User-CRUD-API.git
cd User-CRUD-API
```

2. `.env` ファイルを作成

```
DATABASE_URL=postgresql://postgres:<password>@<host>:5432/postgres
JWT_SECRET=32文字以上のランダム文字列
ACCESS_TOKEN_EXPIRY_MINUTES=60
REFRESH_TOKEN_EXPIRY_DAYS=7
```

3. ビルド・起動

```bash
cargo build
cargo run
```

サーバーが `http://localhost:3000` で起動します。

## API エンドポイント

### 認証

| メソッド | パス | 認証 | 説明 |
|---------|------|------|------|
| POST | `/auth/register` | 不要 | ユーザー登録 |
| POST | `/auth/login` | 不要 | ログイン |
| POST | `/auth/refresh` | 不要 | トークンリフレッシュ |
| POST | `/auth/logout` | 必要 | ログアウト |

### ユーザー

| メソッド | パス | 認証 | 説明 |
|---------|------|------|------|
| GET | `/users` | 不要 | ユーザー一覧 |
| GET | `/users/{id}` | 不要 | ユーザー詳細 |
| PUT | `/users/{id}` | 必要 + 本人のみ | ユーザー更新 |
| DELETE | `/users/{id}` | 必要 + 本人のみ | ユーザー削除 |

## 使い方

```bash
# ユーザー登録
curl -X POST http://localhost:3000/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"taro","email":"taro@example.com","password":"password123"}'

# ログイン
curl -X POST http://localhost:3000/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"taro@example.com","password":"password123"}'

# ユーザー一覧
curl http://localhost:3000/users

# ユーザー更新 (要認証)
curl -X PUT http://localhost:3000/users/{id} \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <access_token>" \
  -d '{"username":"taro_updated"}'

# ユーザー削除 (要認証)
curl -X DELETE http://localhost:3000/users/{id} \
  -H "Authorization: Bearer <access_token>"
```

## 開発

```bash
cargo build          # ビルド
cargo run            # サーバー起動
cargo test           # テスト実行
cargo clippy         # リント
cargo fmt --check    # フォーマットチェック
```
