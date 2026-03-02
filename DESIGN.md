# user-api 設計書

ユーザー管理 REST API の設計図。
Axum + Tokio で構築し、Supabase (PostgreSQL) でデータを永続化する CRUD API。

---

## ディレクトリ構成

```
src/
├── main.rs              # エントリーポイント
├── db.rs                # データベース接続の初期化
├── state.rs             # アプリケーション共有状態
├── snowflake.rs         # Snowflake ID 生成器
├── errors.rs            # エラー型の定義
├── validation.rs        # バリデーションロジック
├── models/
│   ├── mod.rs           # models モジュールの公開窓口
│   └── user.rs          # User 関連の構造体
└── routes/
    ├── mod.rs           # routes モジュールの公開窓口
    └── users.rs         # /users エンドポイントのハンドラー群
```

**環境変数** (`.env`) ※ `.gitignore` に追加すること:
```
DATABASE_URL=postgresql://postgres:<password>@<host>:5432/postgres
```

---

## 各ファイルの詳細

### `main.rs` — エントリーポイント

**役割**: アプリケーションの起動、ルーティング定義、ミドルウェア(トレースレイヤー、CORS)の設定

**モジュール宣言**:
- `mod errors;`
- `mod models;`
- `mod routes;`
- `mod state;`
- `mod db;`
- `mod snowflake;`
- `mod validation;`

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `main` | `#[tokio::main] async fn main()` | .env 読み込み → tracing 初期化 → DB接続プール作成 → AppState 生成 → CORS・トレースレイヤー設定 → Router にルート登録 → `0.0.0.0:3000` で起動 |

**ルート定義**:

| メソッド | パス | ハンドラー |
|---------|------|-----------|
| POST | `/users` | `users::create_user` |
| GET | `/users` | `users::list_users` |
| GET | `/users/{id}` | `users::get_user` |
| PUT | `/users/{id}` | `users::update_user` |
| DELETE | `/users/{id}` | `users::delete_user` |

**ミドルウェア**:
- `TraceLayer` — 全リクエスト/レスポンスを自動ログ出力
- `CorsLayer` — 全オリジン許可、GET/POST/PUT/DELETE メソッド許可

---

### `db.rs` — データベース接続

**役割**: Supabase (PostgreSQL) への接続プールを作成する

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `create_pool` | `pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error>` | 接続文字列を引数で受け取り、`PgPoolOptions` で最大接続数 5、接続タイムアウト 3 秒を指定して `PgPool` を作成して返す。接続失敗時はエラーを返す |

**テーブル定義** (Supabase の SQL Editor で実行):

```sql
CREATE TABLE users (
    id         BIGINT PRIMARY KEY,
    username   TEXT NOT NULL,
    email      TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- `id` は `BIGINT` (= i64) — Snowflake ID を格納
- AUTO INCREMENT は使わない（アプリ側で Snowflake 採番するため）
- `email` に `UNIQUE` 制約 — 同じメールアドレスで重複登録を防ぐ
- `created_at` — レコード作成日時を自動記録

---

### `state.rs` — アプリケーション共有状態

**役割**: DB接続プールと Snowflake 生成器を全ハンドラーで共有する

**構造体**:

```
AppState (Clone)
├── db        : PgPool                          — DB 接続プール
└── snowflake : Arc<Mutex<SnowflakeGenerator>>  — ID 生成器
```

- `PgPool` — 内部で `Arc` を持っているので `Clone` するだけで共有できる
- `Arc<Mutex<...>>` — SnowflakeGenerator は内部状態を変更するためロックが必要

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `AppState::new` | `pub fn new(pool: PgPool, machine_id: u16) -> Self` | DB プールと SnowflakeGenerator を受け取って初期化する |

---

### `snowflake.rs` — Snowflake ID 生成器

**役割**: Twitter 方式の Snowflake アルゴリズムでユニークな i64 ID を生成する

**ID のビット構成** (64bit):

```
[未使用 1bit][タイムスタンプ 41bit][マシンID 10bit][シーケンス 12bit]
```

- タイムスタンプ (41bit) — カスタムエポックからのミリ秒。約69年分
- マシンID (10bit) — サーバー識別子 (0〜1023)
- シーケンス (12bit) — 同一ミリ秒内の連番 (0〜4095)

**定数**:

| 名前 | 値 | 説明 |
|------|-----|------|
| `EPOCH` | `1740000000000` (2025-02-20 に相当) | カスタムエポック (ミリ秒) |
| `MACHINE_ID_BITS` | `10` | マシンID のビット数 |
| `SEQUENCE_BITS` | `12` | シーケンスのビット数 |

**構造体**:

```
SnowflakeGenerator
├── machine_id      : u16    — このサーバーの識別子
├── sequence        : u16    — 現在のシーケンス番号
└── last_timestamp  : u64    — 最後に ID を生成した時刻 (ミリ秒)
```

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `SnowflakeGenerator::new` | `pub fn new(machine_id: u16) -> Self` | machine_id を設定、sequence と last_timestamp を 0 で初期化 |
| `SnowflakeGenerator::generate` | `pub fn generate(&mut self) -> i64` | 現在時刻を取得 → 同一ミリ秒ならシーケンスを加算、新しいミリ秒ならリセット → ビットシフトで ID を組み立て → `as i64` で変換して返す（先頭1bitが未使用=0 なので常に正の値） |

---

### `errors.rs` — エラー型

**役割**: API で返すエラーを統一的に扱う型を定義する

**列挙型 (enum)**:

```
ApiError
├── NotFound              — リソースが見つからない (HTTP 404)
├── BadRequest(String)    — リクエスト不正 (HTTP 400)、理由を文字列で持つ
└── Internal(String)      — サーバー内部エラー (HTTP 500)、DB エラー等
```

**トレイト実装**:

| 実装 | 説明 |
|------|------|
| `impl IntoResponse for ApiError` | ApiError を Axum の HTTP レスポンスに変換する。ステータスコードと `{"error": "メッセージ"}` の JSON ボディを返す |

---

### `validation.rs` — バリデーション

**役割**: リクエストデータの検証ルールを集約する

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `validate_username` | `pub fn validate_username(username: &str) -> Result<(), String>` | 空チェック、文字数制限(1〜32文字)。失敗時はエラーメッセージを返す |
| `validate_email` | `pub fn validate_email(email: &str) -> Result<(), String>` | 空チェック、`@` を含むかの簡易形式チェック。失敗時はエラーメッセージを返す |

`routes/users.rs` の `create_user` / `update_user` から呼び出す。

---

### `models/user.rs` — ユーザー構造体

**役割**: ユーザーに関するデータ型を定義する

**構造体**:

```
User (Clone, Serialize, FromRow)     — レスポンス用 / DB行マッピング用
├── id       : i64
├── username : String
└── email    : String

CreateUser (Deserialize)             — POST リクエストボディ用
├── username : String
└── email    : String

UpdateUser (Deserialize)             — PUT リクエストボディ用
├── username : Option<String>         ← 省略可能（部分更新のため）
└── email    : Option<String>         ← 省略可能
```

---

### `models/mod.rs` — モジュール公開窓口

**役割**: `user.rs` のモジュール宣言と、構造体の再エクスポート

```rust
pub mod user;
pub use user::{CreateUser, UpdateUser, User};
```

これにより他のファイルから `crate::models::User` のように簡潔にアクセスできる。

---

### `routes/users.rs` — ユーザー CRUD ハンドラー

**役割**: 各エンドポイントの具体的な処理を実装する

**関数一覧**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `create_user` | `async fn(State, Json<CreateUser>) -> Result<(StatusCode, Json<User>), ApiError>` | username/email をバリデーション → Snowflake で ID 生成 → `INSERT INTO users` で DB に保存 → 201 Created で返す |
| `list_users` | `async fn(State) -> Result<Json<Vec<User>>, ApiError>` | `SELECT * FROM users ORDER BY id` で全ユーザーを取得して返す |
| `get_user` | `async fn(State, Path<i64>) -> Result<Json<User>, ApiError>` | `SELECT ... WHERE id = $1` で検索 → 見つかれば返す、なければ NotFound |
| `update_user` | `async fn(State, Path<i64>, Json<UpdateUser>) -> Result<Json<User>, ApiError>` | 送られたフィールドをバリデーション → `UPDATE users SET username = COALESCE($1, username), email = COALESCE($2, email) WHERE id = $3` で指定フィールドのみ更新 → `RETURNING *` で更新後の User を返す |
| `delete_user` | `async fn(State, Path<i64>) -> Result<StatusCode, ApiError>` | `DELETE FROM users WHERE id = $1` で削除 → 204 No Content を返す、なければ NotFound |

---

### `routes/mod.rs` — モジュール公開窓口

```rust
pub mod users;
```

---

## 依存クレート (`Cargo.toml`)

| クレート | バージョン | 用途 |
|---------|-----------|------|
| `axum` | 0.8 | Web フレームワーク (ルーティング、リクエスト抽出、レスポンス生成) |
| `tokio` | 1 (features: full) | 非同期ランタイム (async/await の実行基盤) |
| `serde` | 1.0 (features: derive) | 構造体の Serialize/Deserialize を derive マクロで自動実装 |
| `serde_json` | 1 | JSON の生成 (エラーレスポンス用の `json!` マクロ) |
| `sqlx` | 0.8 (features: runtime-tokio, postgres) | PostgreSQL 非同期クライアント (コンパイル時クエリチェック対応) |
| `dotenvy` | 0.15 | `.env` ファイルから環境変数を読み込む |
| `tower-http` | 0.6 (features: cors, trace) | CORS ミドルウェアとリクエストトレース |
| `tracing` | 0.1 | 構造化ログ出力の API (`info!`, `warn!`, `error!` マクロ) |
| `tracing-subscriber` | 0.3 | tracing のログをターミナルに表示するフォーマッター |

---

## 実装の順番（おすすめ）

1. **Supabase** — プロジェクト作成、`users` テーブルを SQL Editor で作成、接続文字列を控える
2. **`Cargo.toml`** — 依存クレートを追加する
3. **`.env`** — `DATABASE_URL` を設定する
4. **`models/user.rs`** と **`models/mod.rs`** — データ型を定義する (`FromRow` derive を含む)
5. **`snowflake.rs`** — Snowflake ID 生成器を作る
6. **`db.rs`** — DB 接続プールを作る
7. **`state.rs`** — 状態管理を作る（PgPool + SnowflakeGenerator）
8. **`errors.rs`** — エラー型を作る（Internal バリアント追加）
9. **`validation.rs`** — バリデーション関数を作る
10. **`routes/users.rs`** と **`routes/mod.rs`** — ハンドラーを実装する（SQL クエリを使用）
11. **`main.rs`** — 全部をつなげ、.env 読み込み・CORS・トレースレイヤーを組み込んで起動する

---
---

# JWT 認証 — 設計追記

## 概要

CRUD API に JWT (JSON Web Token) ベースの認証を追加する。
`/auth/register` と `/auth/login` でトークンを発行し、保護エンドポイントでは `Authorization: Bearer <token>` ヘッダーで認証する。

---

## ディレクトリ構成（変更後）

```
src/
├── main.rs              # エントリーポイント ← 変更
├── auth.rs              # ★ 新規: JWT コアロジック + カスタム Extractor
├── db.rs                # データベース接続の初期化
├── state.rs             # アプリケーション共有状態 ← 変更
├── snowflake.rs         # Snowflake ID 生成器
├── errors.rs            # エラー型の定義 ← 変更
├── validation.rs        # バリデーションロジック ← 変更
├── models/
│   ├── mod.rs           # models モジュールの公開窓口 ← 変更
│   └── user.rs          # User 関連の構造体 ← 変更
└── routes/
    ├── mod.rs           # routes モジュールの公開窓口 ← 変更
    ├── auth.rs          # ★ 新規: register / login ハンドラー
    └── users.rs         # /users エンドポイントのハンドラー群 ← 変更
```

**環境変数の追加** (`.env`):
```
JWT_SECRET=ここに32文字以上のランダム文字列
TOKEN_EXPIRY_HOURS=24
```

---

## 追加する依存クレート (`Cargo.toml`)

| クレート | バージョン | 用途 |
|---------|-----------|------|
| `jsonwebtoken` | 9 | JWT トークンの生成 (`encode`) と検証 (`decode`) |
| `bcrypt` | 0.17 | パスワードのハッシュ化 (`hash`) と照合 (`verify`) |
| `chrono` | 0.4 (features: serde) | トークン有効期限の計算 (`Utc::now()` + Duration) |

---

## DB スキーマ変更

Supabase の SQL Editor で実行:

```sql
ALTER TABLE users ADD COLUMN password_hash TEXT NOT NULL DEFAULT '';
```

- `password_hash` — bcrypt でハッシュ化したパスワードを格納
- `DEFAULT ''` — 既存レコードとの互換性のため（本番では既存ユーザーへの対応を別途検討）

---

## 各ファイルの詳細

### `auth.rs` — JWT コアロジック + カスタム Extractor（★ 新規）

**役割**: JWT トークンの生成・検証ロジックと、Axum のカスタム Extractor を提供する

**use 宣言**:
- `axum::extract::FromRequestParts` — カスタム Extractor トレイト
- `axum::http::request::Parts` — リクエストヘッダーへのアクセス
- `jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey}` — JWT 操作
- `chrono::Utc` — 現在時刻取得
- `serde::{Serialize, Deserialize}` — Claims の JSON 変換
- `crate::state::AppState` — jwt_secret の取得
- `crate::errors::ApiError` — エラー返却

**構造体**:

```
Claims (Serialize, Deserialize)
├── sub : i64    — ユーザーID (subject)
├── exp : u64    — 有効期限 (UNIX タイムスタンプ、秒)
└── iat : u64    — 発行時刻 (UNIX タイムスタンプ、秒)
```

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `create_token` | `pub fn create_token(user_id: i64, secret: &str, expiry_hours: i64) -> Result<String, jsonwebtoken::errors::Error>` | Claims を組み立て → `jsonwebtoken::encode` で HS256 署名付きトークンを生成して返す |
| `validate_token` | `pub fn validate_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error>` | `jsonwebtoken::decode` でトークンを検証・デコードして Claims を返す。期限切れ・署名不正はエラー |

**トレイト実装**:

| 実装 | 説明 |
|------|------|
| `impl FromRequestParts<AppState> for Claims` | リクエストの `Authorization` ヘッダーから `Bearer <token>` を抽出 → `validate_token` で検証 → 成功なら `Claims` を返す、失敗なら `ApiError::Unauthorized` を返す |

**`FromRequestParts` の処理フロー**:
1. `parts.headers` から `Authorization` ヘッダーを取得
2. `"Bearer "` プレフィックスを除去してトークン文字列を取り出す
3. `parts.state` (= `&AppState`) から `jwt_secret` を取得
4. `validate_token(token, secret)` を呼ぶ
5. 成功 → `Ok(claims)`、失敗 → `Err(ApiError::Unauthorized)`

> **ポイント**: `FromRequestParts` はリクエストボディを消費しない Extractor。`FromRequest` だとボディを消費してしまい、後続の `Json<T>` Extractor と競合するため、`FromRequestParts` を使う。

---

### `routes/auth.rs` — 認証ハンドラー（★ 新規）

**役割**: ユーザー登録とログインのエンドポイントを実装する

**use 宣言**:
- `axum::{Json, extract::State, http::StatusCode}` — Axum の基本型
- `bcrypt::{hash, verify, DEFAULT_COST}` — パスワードのハッシュ化・照合
- `crate::auth::create_token` — JWT 発行
- `crate::errors::ApiError` — エラー返却
- `crate::models::{RegisterUser, LoginUser, AuthResponse, User}` — リクエスト・レスポンス型
- `crate::state::AppState` — DB プール・Snowflake・jwt_secret
- `crate::validation::{validate_username, validate_email, validate_password}` — バリデーション

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `register` | `pub async fn register(State(state): State<AppState>, body: Json<RegisterUser>) -> Result<(StatusCode, Json<AuthResponse>), ApiError>` | 下記参照 |
| `login` | `pub async fn login(State(state): State<AppState>, body: Json<LoginUser>) -> Result<Json<AuthResponse>, ApiError>` | 下記参照 |

**`register` の処理フロー**:
1. `validate_username`, `validate_email`, `validate_password` でバリデーション
2. `bcrypt::hash(&body.password, DEFAULT_COST)` でパスワードをハッシュ化
3. `state.snowflake.lock().unwrap().generate()` で Snowflake ID 生成
4. `INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4) RETURNING id, username, email` で DB に保存
5. `create_token(user.id, &state.jwt_secret, 24)` で JWT 発行
6. `(StatusCode::CREATED, Json(AuthResponse { token }))` を返す

**`login` の処理フロー**:
1. `SELECT id, username, email, password_hash FROM users WHERE email = $1` で DB 検索
2. 見つからなければ `ApiError::Unauthorized`
3. `bcrypt::verify(&body.password, &password_hash)` でパスワード照合
4. 不一致なら `ApiError::Unauthorized`
5. `create_token(user.id, &state.jwt_secret, 24)` で JWT 発行
6. `Json(AuthResponse { token })` を返す

> **セキュリティ上の注意**: ログイン失敗時に「メールが存在しない」「パスワードが違う」を区別せず、一律 `Unauthorized` を返す。これにより、攻撃者がメールアドレスの存在有無を推測できないようにする。

> **login での password_hash 取得について**: `User` 構造体には `#[serde(skip_serializing)]` を付けるが、`FromRow` では全カラムを取得する。login では `password_hash` が必要なので、`SELECT` で明示的に `password_hash` を含めて取得し、`query_as::<_, User>` でマッピングする。

---

### `models/user.rs` — 変更点

**追加フィールド**:

```
User (Clone, Serialize, FromRow)
├── id            : i64
├── username      : String
├── email         : String
└── password_hash : String    ← 追加、#[serde(skip_serializing)] を付ける
```

- `#[serde(skip_serializing)]` — JSON レスポンスに `password_hash` を含めない（セキュリティ）
- `FromRow` はそのまま動く（DB の `password_hash` カラムにマッピングされる）

**追加構造体**:

```
RegisterUser (Deserialize)          — POST /auth/register リクエストボディ用
├── username : String
├── email    : String
└── password : String

LoginUser (Deserialize)             — POST /auth/login リクエストボディ用
├── email    : String
└── password : String

AuthResponse (Serialize)            — 認証成功レスポンス用
└── token : String                   ← JWT トークン文字列
```

---

### `models/mod.rs` — 変更点

再エクスポートに追加:

```rust
pub mod user;
pub use user::{CreateUser, UpdateUser, User, RegisterUser, LoginUser, AuthResponse};
```

---

### `errors.rs` — 変更点

`ApiError` enum にバリアントを追加:

```
ApiError
├── NotFound
├── BadRequest(String)
├── Internal(String)
└── Unauthorized         ← 追加 (HTTP 401)
```

`IntoResponse` 実装に追加:

```rust
ApiError::Unauthorized => {
    (StatusCode::UNAUTHORIZED, Json(json!({"error": "Unauthorized"}))).into_response()
}
```

---

### `validation.rs` — 変更点

**追加関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `validate_password` | `pub fn validate_password(password: &str) -> Result<(), String>` | 空チェック、8文字以上。失敗時はエラーメッセージを返す |

---

### `state.rs` — 変更点

**構造体**:

```
AppState (Clone)
├── db         : PgPool
├── snowflake  : Arc<Mutex<SnowflakeGenerator>>
└── jwt_secret : String                           ← 追加
```

**関数のシグネチャ変更**:

| 関数 | 変更前 | 変更後 |
|------|--------|--------|
| `AppState::new` | `pub fn new(pool: PgPool, machine_id: u16) -> Self` | `pub fn new(pool: PgPool, machine_id: u16, jwt_secret: String) -> Self` |

---

### `routes/users.rs` — 変更点

保護が必要なハンドラーに `Claims` 引数を追加:

| 関数 | 変更前のシグネチャ | 変更後のシグネチャ | 認証 |
|------|-------------------|-------------------|------|
| `create_user` | `async fn(State, Json<CreateUser>)` | `async fn(State, Claims, Json<CreateUser>)` | **必要** |
| `update_user` | `async fn(State, Path, Json<UpdateUser>)` | `async fn(State, Path, Claims, Json<UpdateUser>)` | **必要** |
| `delete_user` | `async fn(State, Path)` | `async fn(State, Path, Claims)` | **必要** |
| `list_users` | 変更なし | 変更なし | 不要 |
| `get_user` | 変更なし | 変更なし | 不要 |

- `use crate::auth::Claims;` を追加する
- `Claims` を引数に加えるだけで、Axum が自動的に `FromRequestParts` を呼び出して認証を実行する
- トークンが無い/無効な場合は `ApiError::Unauthorized` が自動的に返され、ハンドラーまで到達しない

> **引数の順番に注意**: Axum の Extractor は引数の順番が重要。`State` を最初に、ボディ消費する `Json<T>` は最後に置く。`Claims` と `Path` はボディを消費しないので中間に配置可能。

---

### `routes/mod.rs` — 変更点

```rust
pub mod auth;     // ← 追加
pub mod users;
```

---

### `main.rs` — 変更点

**モジュール宣言に追加**:
- `mod auth;`

**`main()` 関数の変更**:

1. 環境変数の追加読み込み:
   ```rust
   let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
   ```

2. `AppState::new` の引数変更:
   ```rust
   let state = AppState::new(pool, 10, jwt_secret);
   ```

3. CORS に `Authorization` ヘッダーを許可:
   ```rust
   use axum::http::header::AUTHORIZATION;

   let cors = CorsLayer::new()
       .allow_origin(Any)
       .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
       .allow_headers([AUTHORIZATION, axum::http::header::CONTENT_TYPE]);
   ```
   > `allow_headers` を指定しないと、ブラウザからの `Authorization` ヘッダー付きリクエストが CORS でブロックされる

4. auth ルートの追加:
   ```rust
   use crate::routes::auth;

   let app = Router::new()
       .route("/auth/register", post(auth::register))
       .route("/auth/login", post(auth::login))
       .route("/users", post(users::create_user).get(users::list_users))
       // ... 既存のルート
   ```

**ルート定義（変更後）**:

| メソッド | パス | ハンドラー | 認証 |
|---------|------|-----------|------|
| POST | `/auth/register` | `auth::register` | 不要 |
| POST | `/auth/login` | `auth::login` | 不要 |
| POST | `/users` | `users::create_user` | **必要** |
| GET | `/users` | `users::list_users` | 不要 |
| GET | `/users/{id}` | `users::get_user` | 不要 |
| PUT | `/users/{id}` | `users::update_user` | **必要** |
| DELETE | `/users/{id}` | `users::delete_user` | **必要** |

---

## 実装の順番（JWT 認証）

| # | ファイル | 内容 |
|---|---------|------|
| 12 | Supabase | `ALTER TABLE users ADD COLUMN password_hash TEXT NOT NULL DEFAULT '';` |
| 13 | `Cargo.toml` | `jsonwebtoken`, `bcrypt`, `chrono` を追加 |
| 14 | `.env` | `JWT_SECRET`, `TOKEN_EXPIRY_HOURS` を追加 |
| 15 | `models/user.rs` | `password_hash` フィールド追加、`RegisterUser`, `LoginUser`, `AuthResponse` 追加 |
| 16 | `models/mod.rs` | 再エクスポートに `RegisterUser`, `LoginUser`, `AuthResponse` 追加 |
| 17 | `errors.rs` | `Unauthorized` バリアント追加 |
| 18 | `validation.rs` | `validate_password` 追加 |
| 19 | `state.rs` | `jwt_secret` フィールド追加、`new()` シグネチャ変更 |
| 20 | `auth.rs` | `Claims`, `create_token`, `validate_token`, `FromRequestParts` 実装 |
| 21 | `routes/auth.rs` | `register`, `login` ハンドラー実装 |
| 22 | `routes/mod.rs` | `pub mod auth;` 追加 |
| 23 | `routes/users.rs` | 保護ルートに `Claims` 引数追加 |
| 24 | `main.rs` | `mod auth;`, JWT_SECRET 読み込み, auth ルート登録, CORS 変更 |

---

## 動作確認

```bash
# 1. ユーザー登録
curl -X POST http://localhost:3000/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"taro","email":"taro@example.com","password":"password123"}'
# → 201 + {"token":"eyJ..."}

# 2. ログイン
curl -X POST http://localhost:3000/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"taro@example.com","password":"password123"}'
# → 200 + {"token":"eyJ..."}

# 3. 保護ルート（トークンあり）
curl -X DELETE http://localhost:3000/users/123 \
  -H "Authorization: Bearer eyJ..."
# → 204 No Content

# 4. 保護ルート（トークンなし）
curl -X DELETE http://localhost:3000/users/123
# → 401 + {"error":"Unauthorized"}
```
