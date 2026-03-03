# user-api 設計書

ユーザー管理 REST API の設計図。
Axum + Tokio で構築し、Supabase (PostgreSQL) でデータを永続化する。
JWT 認証により、保護エンドポイントへのアクセス制御を行う。

---

## ディレクトリ構成

```
src/
├── main.rs              # エントリーポイント
├── auth.rs              # JWT コアロジック + カスタム Extractor
├── db.rs                # データベース接続の初期化
├── state.rs             # アプリケーション共有状態
├── snowflake.rs         # Snowflake ID 生成器
├── errors.rs            # エラー型の定義
├── validation.rs        # バリデーションロジック
├── models/
│   ├── mod.rs           # models モジュールの公開窓口
│   ├── auth.rs          # 認証関連の構造体
│   └── user.rs          # User 関連の構造体
└── routes/
    ├── mod.rs           # routes モジュールの公開窓口
    ├── auth.rs          # register / login ハンドラー
    └── users.rs         # /users エンドポイントのハンドラー群
```

**環境変数** (`.env`) ※ `.gitignore` に追加すること:
```
DATABASE_URL=postgresql://postgres:<password>@<host>:5432/postgres
JWT_SECRET=32文字以上のランダム文字列
ACCESS_TOKEN_EXPIRY_MINUTES=60
REFRESH_TOKEN_EXPIRY_DAYS=7
```

---

## 各ファイルの詳細

### `main.rs` — エントリーポイント

**役割**: アプリケーションの起動、ルーティング定義、ミドルウェア(トレースレイヤー、CORS、レートリミット)の設定

**モジュール宣言**:
- `mod auth;`
- `mod db;`
- `mod errors;`
- `mod models;`
- `mod routes;`
- `mod snowflake;`
- `mod state;`
- `mod validation;`

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `main` | `#[tokio::main] async fn main()` | .env 読み込み → `tracing_subscriber::registry()` + `EnvFilter` + `fmt::layer()` で tracing 初期化 → DB接続プール作成 → JWT_SECRET 読み込み → AppState 生成 → レートリミット・CORS・トレースレイヤー設定 → Router にルート登録 → `0.0.0.0:3000` で起動 |

**ルート定義**:

| メソッド | パス | ハンドラー | 認証 |
|---------|------|-----------|------|
| POST | `/auth/register` | `auth::register` | 不要 |
| POST | `/auth/login` | `auth::login` | 不要 |
| POST | `/auth/refresh` | `auth::refresh` | 不要 |
| POST | `/auth/logout` | `auth::logout` | **必要** |
| GET | `/users` | `users::list_users` | 不要 |
| GET | `/users/{id}` | `users::get_user` | 不要 |
| PUT | `/users/{id}` | `users::update_user` | **必要 + 本人のみ** |
| DELETE | `/users/{id}` | `users::delete_user` | **必要 + 本人のみ** |

**ミドルウェア**:
- `GovernorLayer` — IP ベースのレートリミット。ルートグループごとに異なる設定を適用する。超過時は `429 Too Many Requests` を返す
- `TraceLayer` — 全リクエスト/レスポンスを自動ログ出力
- `CorsLayer` — 全オリジン許可、GET/POST/PUT/DELETE メソッド許可、`Authorization` / `Content-Type` ヘッダー許可

**レートリミット設定**:

| ルートグループ | `per_second` | `burst_size` | 理由 |
|--------------|-------------|-------------|------|
| 認証ルート (`/auth/*`) | 1 | 5 | bcrypt が重い + ブルートフォース防止のため厳しく制限 |
| ユーザールート (`/users/*`) | 10 | 50 | 軽い SELECT クエリが中心。正規ユーザーの利便性を優先 |

- ルートグループごとに別の `GovernorConfig` を作成し、それぞれの `Router` に `.layer()` で適用してから `merge` で合流する

```rust
// レートリミット設定
let auth_governor = GovernorConfigBuilder::default()
    .per_second(1)
    .burst_size(5)
    .finish()
    .unwrap();

let user_governor = GovernorConfigBuilder::default()
    .per_second(10)
    .burst_size(50)
    .finish()
    .unwrap();

// 認証ルート（厳しいレートリミット）
let auth_routes = Router::new()
    .route("/auth/register", post(...))
    .route("/auth/login", post(...))
    .route("/auth/refresh", post(...))
    .route("/auth/logout", post(...))
    .layer(GovernorLayer::new(&auth_governor));

// ユーザールート（緩いレートリミット）
let user_routes = Router::new()
    .route("/users", get(...))
    .route("/users/{id}", get(...).put(...).delete(...))
    .layer(GovernorLayer::new(&user_governor));

// 合流
let app = Router::new()
    .merge(auth_routes)
    .merge(user_routes)
    .with_state(state)
    .layer(cors)
    .layer(TraceLayer::new_for_http());
```

> **ポイント**: `GovernorLayer` は IP アドレスでクライアントを識別する。IP を取得するために、`axum::serve` の引数を `app.into_make_service_with_connect_info::<SocketAddr>()` に変更する必要がある

---

### `auth.rs` — JWT コアロジック + カスタム Extractor

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
├── sub : String — ユーザーID (subject)。RFC 7519 に準拠し文字列型。生成時に i64 → String 変換
├── exp : u64    — 有効期限 (UNIX タイムスタンプ、秒)
└── iat : u64    — 発行時刻 (UNIX タイムスタンプ、秒)
```

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `create_token` | `pub fn create_token(user_id: i64, secret: &str, expiry_minutes: u64) -> Result<String, jsonwebtoken::errors::Error>` | `user_id.to_string()` で sub を生成 → Claims を組み立て → `jsonwebtoken::encode` で HS256 署名付きトークンを生成して返す |
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
5. 成功 → `Ok(claims)`
6. 失敗 → `tracing::warn!` でエラー種別をサーバーログに記録 → クライアントには一律 `Err(ApiError::Unauthorized)` を返す

> **ポイント**: `FromRequestParts` はリクエストボディを消費しない Extractor。`FromRequest` だとボディを消費してしまい、後続の `Json<T>` Extractor と競合するため、`FromRequestParts` を使う。

> **Axum 0.8 の注意**: `#[async_trait]` マクロは不要。Axum 0.8 では RPITIT (return-position impl trait in traits) により、`impl FromRequestParts` で直接 `async fn` が書ける。

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
    id            BIGINT PRIMARY KEY,
    username      TEXT NOT NULL,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE refresh_tokens (
    id         BIGINT PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token      TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- `id` は `BIGINT` (= i64) — Snowflake ID を格納
- AUTO INCREMENT は使わない（アプリ側で Snowflake 採番するため）
- `email` に `UNIQUE` 制約 — 同じメールアドレスで重複登録を防ぐ
- `password_hash` — bcrypt でハッシュ化したパスワードを格納
- `created_at` — レコード作成日時を自動記録
- `refresh_tokens.token` — UUID v4 文字列。`UNIQUE` 制約で一意性を保証
- `ON DELETE CASCADE` — ユーザー削除時にリフレッシュトークンも自動削除

---

### `state.rs` — アプリケーション共有状態

**役割**: DB接続プール、Snowflake 生成器、JWT シークレット、トークン有効期限を全ハンドラーで共有する

**構造体**:

```
AppState (Clone)
├── db                          : PgPool                          — DB 接続プール
├── snowflake                   : Arc<Mutex<SnowflakeGenerator>>  — ID 生成器
├── jwt_secret                  : String                          — JWT 署名用シークレット
├── access_token_expiry_minutes : u64                             — アクセストークン有効期限（分）
└── refresh_token_expiry_days   : u64                             — リフレッシュトークン有効期限（日）
```

- `PgPool` — 内部で `Arc` を持っているので `Clone` するだけで共有できる
- `Arc<Mutex<...>>` — SnowflakeGenerator は内部状態を変更するためロックが必要
- トークン有効期限は起動時に `.env` から1回だけ読み取り、ハンドラーでは `state` 経由で参照する

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `AppState::new` | `pub fn new(pool: PgPool, machine_id: u16, jwt_secret: String, access_token_expiry_minutes: u64, refresh_token_expiry_days: u64) -> Self` | DB プール、SnowflakeGenerator、JWT シークレット、トークン有効期限を受け取って初期化する |

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
├── Unauthorized          — 認証失敗 (HTTP 401)、トークン無効・未提供
├── Forbidden             — 認可失敗 (HTTP 403)、認証済みだが他人のリソースにアクセス
└── Internal(String)      — サーバー内部エラー (HTTP 500)、DB エラー等
```

**トレイト実装**:

| 実装 | 説明 |
|------|------|
| `impl IntoResponse for ApiError` | ApiError を Axum の HTTP レスポンスに変換する。ステータスコードと `{"error": "メッセージ"}` の JSON ボディを返す。`Forbidden` は `{"error": "Forbidden"}` を返す |

---

### `validation.rs` — バリデーション

**役割**: リクエストデータの検証ルールを集約する

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `validate_username` | `pub fn validate_username(username: &str) -> Result<(), String>` | 空チェック、文字数制限(1〜32文字)。失敗時はエラーメッセージを返す |
| `validate_email` | `pub fn validate_email(email: &str) -> Result<(), String>` | 空チェック、`@` を含むかの簡易形式チェック。失敗時はエラーメッセージを返す |
| `validate_password` | `pub fn validate_password(password: &str) -> Result<(), String>` | 空チェック、8文字以上、72バイト以下（bcrypt の入力上限）。失敗時はエラーメッセージを返す |

`routes/users.rs` の `update_user` および `routes/auth.rs` の `register` から呼び出す。

---

### `models/user.rs` — ユーザー構造体

**役割**: ユーザーリソースに関するデータ型を定義する

**構造体**:

```
User (Clone, Serialize, FromRow)     — レスポンス用 / DB行マッピング用
├── id            : i64
├── username      : String
├── email         : String
└── password_hash : String            ← #[serde(skip_serializing)] + #[sqlx(default)]

UpdateUser (Deserialize)             — PUT /users/{id} リクエストボディ用
├── username : Option<String>         ← 省略可能（部分更新のため）
└── email    : Option<String>         ← 省略可能
```

- `#[serde(skip_serializing)]` を `password_hash` に付けることで、JSON レスポンスにパスワードハッシュが含まれないようにする
- `#[sqlx(default)]` を `password_hash` に付けることで、`SELECT` に `password_hash` カラムを含めなくても `FromRow` が動作する（デフォルト値は `String::default()` = 空文字列）
- 公開クエリ（`list_users`, `get_user` 等）では `password_hash` を `SELECT` しないことで、不要なデータ転送を防ぐ

---

### `models/auth.rs` — 認証関連の構造体

**役割**: 認証フローで使用するリクエスト・レスポンスのデータ型を定義する

**use 宣言**:
- `chrono::{DateTime, Utc}` — RefreshToken の expires_at 用
- `serde::{Deserialize, Serialize}` — JSON 変換
- `sqlx::prelude::FromRow` — DB 行マッピング

**構造体**:

```
RegisterUser (Deserialize)           — POST /auth/register リクエストボディ用
├── username : String
├── email    : String
└── password : String

LoginUser (Deserialize)              — POST /auth/login リクエストボディ用
├── email    : String
└── password : String

AuthResponse (Serialize)             — 認証成功レスポンス用
├── access_token  : String            ← JWT アクセストークン文字列
└── refresh_token : String            ← UUID v4 リフレッシュトークン文字列

RefreshRequest (Deserialize)         — POST /auth/refresh リクエストボディ用
└── refresh_token : String

LogoutRequest (Deserialize)          — POST /auth/logout リクエストボディ用
└── refresh_token : String

RefreshToken (FromRow)               — refresh_tokens テーブルの行マッピング用
├── id         : i64                  ← Snowflake ID
├── user_id    : i64                  ← ユーザーID
├── token      : String               ← UUID v4 リフレッシュトークン
└── expires_at : DateTime<Utc>        ← 有効期限 (TIMESTAMPTZ → DateTime<Utc>)
```

---

### `models/mod.rs` — モジュール公開窓口

**役割**: `user.rs` と `auth.rs` のモジュール宣言と、構造体の再エクスポート

```rust
pub mod auth;
pub mod user;

pub use auth::*;
pub use user::*;
```

これにより他のファイルから `crate::models::User` や `crate::models::AuthResponse` のように簡潔にアクセスできる。

---

### `routes/auth.rs` — 認証ハンドラー

**役割**: ユーザー登録、ログイン、トークンリフレッシュ、ログアウトのエンドポイントを実装する

**use 宣言**:
- `axum::{Json, extract::State, http::StatusCode}` — Axum の基本型
- `bcrypt::{hash, verify, DEFAULT_COST}` — パスワードのハッシュ化・照合
- `uuid::Uuid` — リフレッシュトークン生成
- `chrono::Utc` — リフレッシュトークン有効期限の計算
- `crate::auth::{create_token, Claims}` — JWT 発行・Claims 型
- `crate::errors::ApiError` — エラー返却
- `crate::models::{RegisterUser, LoginUser, AuthResponse, RefreshRequest, LogoutRequest, User}` — リクエスト・レスポンス型
- `crate::state::AppState` — DB プール・Snowflake・jwt_secret
- `crate::validation::{validate_username, validate_email, validate_password}` — バリデーション

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `issue_refresh_token` | `async fn issue_refresh_token(state: &AppState, user_id: i64) -> Result<String, ApiError>` | リフレッシュトークンを生成して DB に保存し、トークン文字列を返す（ヘルパー関数） |
| `register` | `pub async fn register(State(state): State<AppState>, body: Json<RegisterUser>) -> Result<(StatusCode, Json<AuthResponse>), ApiError>` | 下記参照 |
| `login` | `pub async fn login(State(state): State<AppState>, body: Json<LoginUser>) -> Result<Json<AuthResponse>, ApiError>` | 下記参照 |
| `refresh` | `pub async fn refresh(State(state): State<AppState>, body: Json<RefreshRequest>) -> Result<Json<AuthResponse>, ApiError>` | 下記参照 |
| `logout` | `pub async fn logout(State(state): State<AppState>, claims: Claims, body: Json<LogoutRequest>) -> Result<StatusCode, ApiError>` | 下記参照 |

**`issue_refresh_token` の処理フロー**:
1. `Uuid::new_v4().to_string()` でリフレッシュトークン生成
2. `state.snowflake.lock().unwrap().generate()` で Snowflake ID 生成
3. `Utc::now() + Duration::days(state.refresh_token_expiry_days)` で有効期限を計算
4. `INSERT INTO refresh_tokens (id, user_id, token, expires_at) VALUES ($1, $2, $3, $4)` で DB に保存
5. トークン文字列を返す

> **ポイント**: `register`, `login`, `refresh` の3箇所で同じリフレッシュトークン発行処理を繰り返すため、ヘルパー関数に切り出して DRY にする。`pub` を付けない（ファイル内でのみ使用）。

**`register` の処理フロー**:
1. `validate_username`, `validate_email`, `validate_password` でバリデーション
2. `bcrypt::hash(&body.password, DEFAULT_COST)` でパスワードをハッシュ化
3. `state.snowflake.lock().unwrap().generate()` で Snowflake ID 生成
4. `INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)` で DB に保存（`RETURNING` 不要 — レスポンスはトークンのみで、ID は生成済みの変数を使う）
5. `create_token(id, &state.jwt_secret, expiry_minutes)` でアクセストークン発行
6. `issue_refresh_token(&state, user_id)` でリフレッシュトークン発行
7. `(StatusCode::CREATED, Json(AuthResponse { access_token, refresh_token }))` を返す

**`login` の処理フロー**:
1. `SELECT id, username, email, password_hash FROM users WHERE email = $1` で DB 検索
2. 見つからなければ `ApiError::Unauthorized`
3. `bcrypt::verify(&body.password, &password_hash)` でパスワード照合
4. 不一致なら `ApiError::Unauthorized`
5. `DELETE FROM refresh_tokens WHERE user_id = $1` で既存リフレッシュトークンを全削除（古いセッションを無効化）
6. `create_token(user.id, &state.jwt_secret, expiry_minutes)` でアクセストークン発行
7. `issue_refresh_token(&state, user.id)` でリフレッシュトークン発行
8. `Json(AuthResponse { access_token, refresh_token })` を返す

**`refresh` の処理フロー**:
1. `SELECT id, user_id, token, expires_at FROM refresh_tokens WHERE token = $1` で DB 検索
2. 見つからなければ `ApiError::Unauthorized`
3. `expires_at < Utc::now()` なら期限切れ → 古いトークンを DELETE → `ApiError::Unauthorized`
4. 古いリフレッシュトークンを DELETE（ローテーション）
5. `create_token(user_id, &state.jwt_secret, expiry_minutes)` で新しいアクセストークン発行
6. `issue_refresh_token(&state, user_id)` で新しいリフレッシュトークン発行
7. `Json(AuthResponse { access_token, refresh_token })` を返す

**`logout` の処理フロー**:
1. `DELETE FROM refresh_tokens WHERE token = $1 AND user_id = $2` で DB から削除（`$2` は `claims.sub` をパースした `i64`）
2. 204 No Content を返す

> **セキュリティ上の注意**: ログイン失敗時に「メールが存在しない」「パスワードが違う」を区別せず、一律 `Unauthorized` を返す。これにより、攻撃者がメールアドレスの存在有無を推測できないようにする。

> **login での password_hash 取得について**: `User` 構造体には `#[serde(skip_serializing)]` を付けるが、login では `password_hash` が必要なので、`SELECT` で明示的に `password_hash` を含めて取得し、`query_as::<_, User>` でマッピングする。`#[sqlx(default)]` により、他のクエリでは `password_hash` を省略できる。

> **リフレッシュトークンのローテーション**: `refresh` エンドポイントでは、使用済みのリフレッシュトークンを削除して新しいものを発行する。これにより、リフレッシュトークンが漏洩した場合のリスクを軽減する。

---

### `routes/users.rs` — ユーザー CRUD ハンドラー

**役割**: 各エンドポイントの具体的な処理を実装する

**関数一覧**:

| 関数 | シグネチャ | 認証 | 説明 |
|------|-----------|------|------|
| `list_users` | `async fn(State) -> Result<Json<Vec<User>>, ApiError>` | 不要 | `SELECT id, username, email FROM users ORDER BY id` で全ユーザーを取得して返す |
| `get_user` | `async fn(State, Path<i64>) -> Result<Json<User>, ApiError>` | 不要 | `SELECT id, username, email FROM users WHERE id = $1` で検索 → 見つかれば返す、なければ NotFound |
| `update_user` | `async fn(State, Path<i64>, Claims, Json<UpdateUser>) -> Result<Json<User>, ApiError>` | **必要 + 本人のみ** | `claims.sub` と Path の `user_id` を比較 → 不一致なら `ApiError::Forbidden` → バリデーション → `UPDATE users SET username = COALESCE($1, username), email = COALESCE($2, email) WHERE id = $3 RETURNING id, username, email` で更新 |
| `delete_user` | `async fn(State, Path<i64>, Claims) -> Result<StatusCode, ApiError>` | **必要 + 本人のみ** | `claims.sub` と Path の `user_id` を比較 → 不一致なら `ApiError::Forbidden` → `DELETE FROM users WHERE id = $1` で削除 → 204 No Content、なければ NotFound |

- `Claims` を引数に加えるだけで、Axum が自動的に `FromRequestParts` を呼び出して認証を実行する
- トークンが無い/無効な場合は `ApiError::Unauthorized` が自動的に返され、ハンドラーまで到達しない
- **認可チェック**: `claims.sub.parse::<i64>()` で得た user_id と Path の user_id を比較し、一致しない場合は `ApiError::Forbidden` を返す
- **明示的カラム指定**: `SELECT *` ではなく `SELECT id, username, email` を使い、`password_hash` を取得しない。`User` 構造体の `password_hash` は `#[sqlx(default)]` により空文字列になる

> **引数の順番に注意**: Axum の Extractor は引数の順番が重要。`State` を最初に、ボディを消費する `Json<T>` は最後に置く。`Claims` と `Path` はボディを消費しないので中間に配置可能。

---

### `routes/mod.rs` — モジュール公開窓口

```rust
pub mod auth;
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
| `sqlx` | 0.8 (features: runtime-tokio, postgres, chrono) | PostgreSQL 非同期クライアント (コンパイル時クエリチェック対応)。`chrono` feature で `DateTime<Utc>` ↔ `TIMESTAMPTZ` の変換に対応 |
| `dotenvy` | 0.15 | `.env` ファイルから環境変数を読み込む |
| `tower-http` | 0.6 (features: cors, trace) | CORS ミドルウェアとリクエストトレース |
| `tracing` | 0.1 | 構造化ログ出力の API (`info!`, `warn!`, `error!` マクロ) |
| `tracing-subscriber` | 0.3 (features: env-filter) | tracing のログをターミナルに表示するフォーマッター。`EnvFilter` で `RUST_LOG` 環境変数によるログレベル制御に対応 |
| `jsonwebtoken` | 10 (features: aws_lc_rs) | JWT トークンの生成 (`encode`) と検証 (`decode`)。v10 では暗号バックエンドの feature 指定が必須 |
| `bcrypt` | 0.18 | パスワードのハッシュ化 (`hash`) と照合 (`verify`) |
| `chrono` | 0.4 (features: serde) | トークン有効期限の計算 (`Utc::now()` + Duration) |
| `uuid` | 1 (features: v4) | リフレッシュトークンの生成 (`Uuid::new_v4`) |
| `tower_governor` | 0.8 | 認証エンドポイントへの IP ベースレートリミット (`GovernorLayer`, `GovernorConfigBuilder`) |

---

## 実装の順番（おすすめ）

### Phase 1: CRUD API

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

### Phase 2: JWT 認証 + 認可 + リフレッシュトークン

12. **Supabase** — `ALTER TABLE users ADD COLUMN password_hash TEXT NOT NULL DEFAULT '';` + `CREATE TABLE refresh_tokens (...)`
13. **`Cargo.toml`** — `jsonwebtoken`, `bcrypt`, `chrono`, `uuid` を追加。`sqlx` の features に `chrono` を追加
14. **`.env`** — `JWT_SECRET`, `ACCESS_TOKEN_EXPIRY_MINUTES`, `REFRESH_TOKEN_EXPIRY_DAYS` を追加
15. **`models/user.rs`** — `password_hash` に `#[sqlx(default)]` 追加、`RegisterUser`, `LoginUser`, `AuthResponse`（`access_token` + `refresh_token`）, `RefreshRequest`, `LogoutRequest` 追加、`CreateUser` 削除
16. **`models/mod.rs`** — 再エクスポート更新（`CreateUser` 削除、`RefreshRequest`, `LogoutRequest` 追加）
17. **`errors.rs`** — `Unauthorized` + `Forbidden` バリアント追加
18. **`validation.rs`** — `validate_password` 追加
19. **`state.rs`** — `jwt_secret`, `access_token_expiry_minutes`, `refresh_token_expiry_days` フィールド追加、`new()` シグネチャ変更
20. **`auth.rs`** — `Claims`, `create_token`（`expiry_minutes`）, `validate_token`, `FromRequestParts` 実装
21. **`routes/auth.rs`** — `register`, `login`, `refresh`, `logout` ハンドラー実装
22. **`routes/mod.rs`** — `pub mod auth;` 追加
23. **`routes/users.rs`** — `create_user` 削除、認可チェック追加（`claims.sub == user_id`）、明示的カラム指定（`SELECT id, username, email`）
24. **`main.rs`** — `mod auth;`, JWT_SECRET 読み込み, `POST /users` 削除, `auth/refresh` + `auth/logout` ルート追加, CORS に `allow_headers` 追加

### Phase 3: セキュリティ強化

25. **`Cargo.toml`** — `tower_governor` 0.8 を追加
26. **`routes/auth.rs`** — `login` でリフレッシュトークン発行前に既存トークンを全削除
27. **`main.rs`** — `GovernorConfigBuilder` で `per_second: 1, burst_size: 5` を設定 → 認証ルートにのみ `GovernorLayer` を適用 → `axum::serve` を `into_make_service_with_connect_info::<SocketAddr>()` に変更

---

## 動作確認

```bash
# ユーザー登録 → access_token + refresh_token
curl -X POST http://localhost:3000/auth/register \
  -H "Content-Type: application/json" \
  -d '{"username":"taro","email":"taro@example.com","password":"password123"}'
# → 201 + {"access_token":"eyJ...","refresh_token":"550e8400-..."}

# ログイン
curl -X POST http://localhost:3000/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"taro@example.com","password":"password123"}'
# → 200 + {"access_token":"eyJ...","refresh_token":"550e8400-..."}

# トークンリフレッシュ
curl -X POST http://localhost:3000/auth/refresh \
  -H "Content-Type: application/json" \
  -d '{"refresh_token":"550e8400-..."}'
# → 200 + {"access_token":"eyJ...(新)","refresh_token":"660f9500-...(新)"}

# ログアウト（リフレッシュトークン無効化）
curl -X POST http://localhost:3000/auth/logout \
  -H "Authorization: Bearer eyJ..." \
  -H "Content-Type: application/json" \
  -d '{"refresh_token":"660f9500-..."}'
# → 204 No Content

# ユーザー一覧（認証不要）
curl http://localhost:3000/users
# → 200 + [{"id":...,"username":"taro","email":"taro@example.com"}, ...]

# ユーザー詳細（認証不要）
curl http://localhost:3000/users/123
# → 200 + {"id":123,"username":"taro","email":"taro@example.com"}

# 本人による更新（OK — claims.sub == 123）
curl -X PUT http://localhost:3000/users/123 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer eyJ..." \
  -d '{"username":"taro_updated"}'
# → 200 + {"id":123,"username":"taro_updated","email":"taro@example.com"}

# 他人による更新（拒否 — claims.sub == 123 だが Path は 456）
curl -X PUT http://localhost:3000/users/456 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer eyJ..." \
  -d '{"username":"hacked"}'
# → 403 + {"error":"Forbidden"}

# ユーザー削除（本人のみ）
curl -X DELETE http://localhost:3000/users/123 \
  -H "Authorization: Bearer eyJ..."
# → 204 No Content

# 保護ルート（トークンなし → 401）
curl -X DELETE http://localhost:3000/users/123
# → 401 + {"error":"Unauthorized"}

# POST /users は存在しない
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"username":"jiro","email":"jiro@example.com"}'
# → 405 Method Not Allowed
```
