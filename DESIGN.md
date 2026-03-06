# user-api 設計書

ユーザー管理 REST API の設計図。
Axum + Tokio で構築し、Supabase (PostgreSQL) でデータを永続化する。
JWT 認証により、保護エンドポイントへのアクセス制御を行う。

---

## ディレクトリ構成

```
src/
├── main.rs              # エントリーポイント（バイナリ）
├── lib.rs               # ライブラリクレート（モジュール公開）
├── auth.rs              # JWT コアロジック + カスタム Extractor
├── config.rs            # 環境変数の読み込み + 設定構造体
├── db.rs                # データベース接続の初期化
├── rate_limit.rs        # レート制限のキー(IP)抽出
├── state.rs             # アプリケーション共有状態
├── snowflake.rs         # Snowflake ID 生成器
├── errors.rs            # エラー型の定義
├── validation.rs        # バリデーション + email 正規化
├── models/
│   ├── mod.rs           # models モジュールの公開窓口
│   ├── auth.rs          # 認証関連の構造体
│   └── user.rs          # User 関連の構造体
└── routes/
    ├── mod.rs           # routes モジュールの公開窓口
    ├── auth.rs          # register / login / refresh / logout ハンドラー
    └── users.rs         # /users エンドポイントのハンドラー群
```

**環境変数** (`.env`) ※ `.gitignore` に追加すること:
```
DATABASE_URL=postgresql://postgres:<password>@<host>:5432/postgres
JWT_SECRET=32文字以上のランダム文字列
JWT_ISSUER=user-api
JWT_AUDIENCE=user-api
JWT_LEEWAY_SECONDS=60
ACCESS_TOKEN_EXPIRY_MINUTES=60
REFRESH_TOKEN_EXPIRY_DAYS=7
REFRESH_TOKEN_PEPPER=32文字以上のランダム文字列
SNOWFLAKE_MACHINE_ID=10
RATE_LIMIT_IP_MODE=peer
```

---

## 各ファイルの詳細

### `main.rs` — エントリーポイント

**役割**: アプリケーションの起動、ルーティング定義、ミドルウェア(トレースレイヤー、CORS、レートリミット)の設定

**ライブラリクレート構成**:
`main.rs` はバイナリのエントリーポイント、`lib.rs` がモジュールを公開するライブラリクレート。

```rust
// src/lib.rs
pub mod auth;
pub mod config;
pub mod db;
pub mod errors;
pub mod models;
pub mod rate_limit;
pub mod routes;
pub mod snowflake;
pub mod state;
pub mod validation;
```

`main.rs` では `use user_api::{...}` でインポートする。

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `main` | `#[tokio::main] async fn main()` | .env 読み込み → tracing 初期化 → `Config::from_env()` で設定読み込み → DB接続プール作成 → AppState 生成 → レートリミット・CORS・トレースレイヤー設定 → Router にルート登録 → `0.0.0.0:3000` で起動（graceful shutdown 付き） |
| `shutdown_signal` | `async fn shutdown_signal()` | Ctrl+C (SIGINT) と SIGTERM を `tokio::select!` で待ち受ける。いずれかのシグナルを受信すると future が完了し、`axum::serve` が新規接続の受付を停止して進行中のリクエストを処理し終えてから終了する |

**Graceful Shutdown**:
- `axum::serve(...).with_graceful_shutdown(shutdown_signal())` でシグナル待ち受けを登録する
- シグナル受信後、新規接続は受け付けず、進行中のリクエストが完了するまで待機してから終了する
- Unix 環境では `SIGTERM` にも対応（Docker / Kubernetes 等のコンテナ環境で `kill` コマンドが送る標準シグナル）
- 非 Unix 環境では `Ctrl+C` のみ対応

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
- `GovernorLayer` — IP ベースのレートリミット。ルートグループごとに異なる設定を適用する。超過時は `429 Too Many Requests` を返す。キー抽出は `RATE_LIMIT_IP_MODE` で切替する。`peer` (既定) は peer IP のみを使い、`Forwarded`/`X-Forwarded-For` 等を一切信頼しない。`smart` は `Forwarded` / `X-Forwarded-For` / `X-Real-Ip` を優先し、無ければ peer IP にフォールバックする（信頼できるリバースプロキシ配下でのみ利用）。
- `SetRequestIdLayer` — リクエストごとに UUID v4 の `x-request-id` ヘッダーを付与（`MakeRequestUuid` で生成）
- `TraceLayer` — 全リクエスト/レスポンスを自動ログ出力。`make_span_with` で `x-request-id`・メソッド・URI をスパンに記録し、ハンドラー内のログに自動で含まれるようにする
- `CorsLayer` — 全オリジン許可、GET/POST/PUT/DELETE メソッド許可、`Authorization` / `Content-Type` ヘッダー許可
- `CompressionLayer` — レスポンスの gzip 圧縮
- `DefaultBodyLimit` — リクエストボディのサイズ上限 (5MB)
- `TimeoutLayer` — リクエスト処理のタイムアウト (10秒、超過時は 408 Request Timeout)
- `HelmetLayer` — セキュリティヘッダー一括設定 (HSTS, X-Content-Type-Options 等)
- `NormalizePathLayer` — 末尾スラッシュの正規化 (`/users/` → `/users`)。Router の外側からラップする必要がある

**レートリミット設定**:

| ルートグループ | `per_second` | `burst_size` | 理由 |
|--------------|-------------|-------------|------|
| 認証ルート (`/auth/*`) | 1 | 5 | bcrypt が重い + ブルートフォース防止のため厳しく制限 |
| ユーザールート (`/users/*`) | 10 | 50 | 軽い SELECT クエリが中心。正規ユーザーの利便性を優先 |

- ルートグループごとに別の `GovernorConfig` を作成し、それぞれの `Router` に `.layer()` で適用してから `merge` で合流する

```rust
// RATE_LIMIT_IP_MODE (peer/smart) でキー抽出を切替
let rate_limit_ip_mode = std::env::var("RATE_LIMIT_IP_MODE")
    .ok()
    .map(|s| {
        RateLimitIpMode::parse(&s).unwrap_or_else(|| {
            panic!("RATE_LIMIT_IP_MODE must be one of: peer, smart (got: {s})")
        })
    })
    .unwrap_or(RateLimitIpMode::Peer);
let key_extractor = RateLimitIpKeyExtractor::new(rate_limit_ip_mode);

// レートリミット設定
let auth_governor = GovernorConfigBuilder::default()
    .key_extractor(key_extractor.clone())
    .per_second(1)
    .burst_size(5)
    .finish()
    .unwrap();

let user_governor = GovernorConfigBuilder::default()
    .key_extractor(key_extractor)
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
    .layer(GovernorLayer::new(auth_governor));

// ユーザールート（緩いレートリミット）
let user_routes = Router::new()
    .route("/users", get(...))
    .route("/users/{id}", get(...).put(...).delete(...))
    .layer(GovernorLayer::new(user_governor));

// 合流 + ミドルウェア
let app = Router::new()
    .merge(auth_routes)
    .merge(user_routes)
    .with_state(state)
    .layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::new(
                x_request_id.clone(),
                MakeRequestUuid,
            ))
            .layer(TraceLayer::new_for_http())
            .layer(cors)
            .layer(CompressionLayer::new())
            .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(10),
            ))
            .layer(HelmetLayer::with_defaults()),
    );

// NormalizePathLayer は Router の外側からラップ
let app: NormalizePath<Router> =
    NormalizePathLayer::trim_trailing_slash().layer(app);
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
├── iss : String — issuer（発行者）。検証時に一致必須
├── aud : String — audience（受信者）。検証時に一致必須
├── exp : u64    — 有効期限 (UNIX タイムスタンプ、秒)
└── iat : u64    — 発行時刻 (UNIX タイムスタンプ、秒)
```

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `create_token` | `pub fn create_token(user_id: i64, secret: &str, issuer: &str, audience: &str, expiry_minutes: u64) -> Result<String, jsonwebtoken::errors::Error>` | `user_id.to_string()` で sub を生成 → `iss`/`aud`/`iat`/`exp` を含む Claims を組み立て → `jsonwebtoken::encode` で **HS256** 署名付きトークンを生成して返す |
| `validate_token` | `pub fn validate_token(token: &str, secret: &str, issuer: &str, audience: &str, leeway_seconds: u64) -> Result<Claims, jsonwebtoken::errors::Error>` | `jsonwebtoken::decode` でトークンを検証・デコードして Claims を返す。**許可アルゴリズムは HS256 のみに固定**し、`iss`/`aud`/`sub`/`exp`/`iat` を必須化する。追加で `iat` が現在時刻より大きく未来（`leeway` を超える）なら拒否する |

**トレイト実装**:

| 実装 | 説明 |
|------|------|
| `impl FromRequestParts<AppState> for Claims` | リクエストの `Authorization` ヘッダーから `Bearer <token>` を抽出 → `validate_token` で検証 → 成功なら `Claims` を返す、失敗なら `ApiError::Unauthorized` を返す |

**`FromRequestParts` の処理フロー**:
1. `parts.headers` から `Authorization` ヘッダーを取得
2. `"Bearer "` プレフィックスを除去してトークン文字列を取り出す
3. `state` (= `&AppState`) から `jwt_secret` / `jwt_issuer` / `jwt_audience` / `jwt_leeway_seconds` を取得
4. `validate_token(token, secret, issuer, audience, leeway_seconds)` を呼ぶ
5. 成功 → `Ok(claims)`
6. 失敗 → `tracing::warn!` でエラー種別をサーバーログに記録 → クライアントには一律 `Err(ApiError::Unauthorized)` を返す

> **ポイント**: `FromRequestParts` はリクエストボディを消費しない Extractor。`FromRequest` だとボディを消費してしまい、後続の `Json<T>` Extractor と競合するため、`FromRequestParts` を使う。

> **Axum 0.8 の注意**: `#[async_trait]` マクロは不要。Axum 0.8 では RPITIT (return-position impl trait in traits) により、`impl FromRequestParts` で直接 `async fn` が書ける。

---

### `config.rs` — 環境変数の読み込み

**役割**: `.env` から読み込んだ環境変数を構造体にまとめる。`main.rs` の起動処理を簡潔にする

**構造体**:

```
Config
├── database_url               : String           — PostgreSQL 接続文字列
├── jwt_secret                 : String           — JWT 署名用シークレット（32文字以上）
├── jwt_issuer                 : String           — JWT issuer
├── jwt_audience               : String           — JWT audience
├── jwt_leeway_seconds         : u64              — JWT 検証の leeway（秒、既定 60）
├── access_token_expiry_minutes: u64              — アクセストークン有効期限（分）
├── refresh_token_expiry_days  : u64              — リフレッシュトークン有効期限（日）
├── refresh_token_pepper       : String           — リフレッシュトークン HMAC 用ペッパー
├── rate_limit_ip_mode         : RateLimitIpMode  — レート制限の IP 抽出モード（既定 Peer）
└── snowflake_machine_id       : u16              — Snowflake 生成器の machine_id
```

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `Config::from_env` | `pub fn from_env() -> Self` | `std::env::var` で各環境変数を読み込み、バリデーション（`JWT_SECRET` の最小長チェック等）を行い、`Config` を返す。必須変数が未設定、またはパースに失敗した場合は `panic!` する |

- `JWT_LEEWAY_SECONDS` と `RATE_LIMIT_IP_MODE` はオプション（未設定時はデフォルト値を使用）
- それ以外の環境変数はすべて必須

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
	    email         TEXT NOT NULL,
	    password_hash TEXT NOT NULL DEFAULT '',
	    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
	);

CREATE TABLE refresh_tokens (
    id         BIGINT PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- `id` は `BIGINT` (= i64) — Snowflake ID を格納
- AUTO INCREMENT は使わない（アプリ側で Snowflake 採番するため）
- `email` はアプリ側で正規化して保存し、DB側の `unique(lower(email))` で重複登録を防ぐ
- `password_hash` — bcrypt でハッシュ化したパスワードを格納
- `created_at` — レコード作成日時を自動記録
- `refresh_tokens.token_hash` — リフレッシュトークンのハッシュ値。`UNIQUE` 制約で一意性を保証
- `ON DELETE CASCADE` — ユーザー削除時にリフレッシュトークンも自動削除

**email の正規化と制約（推奨）**:
- アプリ側で `trim` + `lowercase` して保存・検索する（`User@Example.COM` と `user@example.com` を同一扱いにする）
- DB側で case-insensitive な一意制約を追加する（関数インデックス）

```sql
-- case-insensitive UNIQUE (Option 2)
-- 既存の email UNIQUE を使っていた場合は制約を落とす（任意）
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_email_key;

-- 既存データに大小だけ違う重複が無いことを確認してから作成する
CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_key ON users (lower(email));
```

---

### `state.rs` — アプリケーション共有状態

**役割**: DB接続プール、Snowflake 生成器、JWT 設定、トークン有効期限、認証用の補助データを全ハンドラーで共有する

**構造体**:

```
AppState (Clone)
├── db                          : PgPool                          — DB 接続プール
├── snowflake                   : Arc<Mutex<SnowflakeGenerator>>  — ID 生成器
├── jwt_secret                  : String                          — JWT 署名用シークレット
├── jwt_issuer                  : String                          — JWT issuer
├── jwt_audience                : String                          — JWT audience
├── jwt_leeway_seconds          : u64                             — JWT 検証の leeway（秒）
├── access_token_expiry_minutes : u64                             — アクセストークン有効期限（分）
├── refresh_token_expiry_days   : u64                             — リフレッシュトークン有効期限（日）
├── refresh_token_pepper        : String                          — リフレッシュトークンHMAC用のペッパー
└── dummy_password_hash         : String                          — login のタイミング差対策用ダミー bcrypt ハッシュ
```

- `PgPool` — 内部で `Arc` を持っているので `Clone` するだけで共有できる
- `Arc<Mutex<...>>` — SnowflakeGenerator は内部状態を変更するためロックが必要
- トークン有効期限は起動時に `.env` から1回だけ読み取り、ハンドラーでは `state` 経由で参照する

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `AppState::new` | `pub fn new(pool: PgPool, machine_id: u16, jwt_secret: String, jwt_issuer: String, jwt_audience: String, jwt_leeway_seconds: u64, access_token_expiry_minutes: u64, refresh_token_expiry_days: u64, refresh_token_pepper: String, dummy_password_hash: String) -> Self` | DB プール、SnowflakeGenerator、JWT 設定、トークン有効期限、認証用の補助データを受け取って初期化する |

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
| `SnowflakeGenerator::generate` | `pub fn generate(&mut self) -> i64` | 現在時刻を取得 → **時刻が `last_timestamp` より過去なら panic** → 同一ミリ秒ならシーケンスを加算（オーバーフロー時は次のミリ秒までスピン待ち）、新しいミリ秒ならリセット → ビットシフトで ID を組み立て → `as i64` で変換して返す（先頭1bitが未使用=0 なので常に正の値） |

**時刻巻き戻り（Clock Rollback）ポリシー**:
- `generate()` で取得した現在時刻が `last_timestamp` より過去の場合、**即座に panic** する
- NTP 同期等でシステム時刻が巻き戻ると、同じタイムスタンプで ID が再生成され、一意性と単調増加が崩壊するため、安全側に倒す
- 同一ミリ秒内のシーケンス枯渇（4096 個/ms を超えた場合）は、次のミリ秒になるまでスピンループで待機する（こちらは正常動作）

---

### `errors.rs` — エラー型

**役割**: API で返すエラーを統一的に扱う型を定義する

**列挙型 (enum)**:

```
ApiError (Debug)
├── NotFound              — リソースが見つからない (HTTP 404)
├── BadRequest(String)    — リクエスト不正 (HTTP 400)、理由を文字列で持つ
├── Conflict(String)      — リソース競合 (HTTP 409)、email 重複等
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
| `normalize_email` | `pub fn normalize_email(email: &str) -> String` | `trim` + `to_ascii_lowercase` で email を正規化する。`register` / `login` / `update_user` で保存・検索前に呼び出す |

`routes/users.rs` の `update_user` および `routes/auth.rs` の `register` から呼び出す。

---

### `models/user.rs` — ユーザー構造体

**役割**: ユーザーリソースに関するデータ型を定義する

**構造体**:

```
User (Clone, Serialize, FromRow)     — 内部用 / DB行マッピング用（login 等で password_hash が必要な場合）
├── id            : i64
├── username      : String
├── email         : String            ← #[serde(skip_serializing)]
└── password_hash : String            ← #[serde(skip_serializing)] + #[sqlx(default)]

PublicUser (Clone, Serialize, FromRow) — 公開レスポンス用（最小限のデータのみ）
├── id       : i64
└── username : String

UpdateUser (Deserialize)             — PUT /users/{id} リクエストボディ用
├── username : Option<String>         ← 省略可能（部分更新のため）
└── email    : Option<String>         ← 省略可能
```

- `User` は `email` と `password_hash` の両方に `#[serde(skip_serializing)]` を付けている。JSON レスポンスに個人情報やパスワードハッシュが含まれないようにする
- `#[sqlx(default)]` を `password_hash` に付けることで、`SELECT` に `password_hash` カラムを含めなくても `FromRow` が動作する（デフォルト値は `String::default()` = 空文字列）
- **公開エンドポイント** (`list_users`, `get_user`) は `PublicUser` を使い、`SELECT id, username` のみで最小限のデータを返す
- `User` は `login` 等の内部処理で `password_hash` を取得する必要がある場合に使う

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
├── token_hash : String               ← リフレッシュトークンのハッシュ値
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
- `chrono::{Duration, Utc}` — リフレッシュトークン有効期限の計算
- `hmac::{Hmac, Mac}` — HMAC-SHA256 によるトークンハッシュ化
- `sha2::Sha256` — HMAC のハッシュアルゴリズム
- `uuid::Uuid` — リフレッシュトークン生成
- `crate::auth::{create_token, Claims}` — JWT 発行・Claims 型
- `crate::errors::ApiError` — エラー返却
- `crate::models::{RegisterUser, LoginUser, AuthResponse, RefreshRequest, LogoutRequest, RefreshToken, User}` — リクエスト・レスポンス型
- `crate::state::AppState` — DB プール・Snowflake・jwt_secret
- `crate::validation::{normalize_email, validate_username, validate_email, validate_password}` — バリデーション・正規化

**関数**:

| 関数 | シグネチャ | 説明 |
|------|-----------|------|
| `map_unique_violation_to_conflict` | `fn map_unique_violation_to_conflict(code: Option<&str>, constraint: Option<&str>) -> Option<ApiError>` | PostgreSQL の UNIQUE 違反（SQLSTATE 23505）を検出し、email 制約（`users_email_key` / `users_email_lower_key`）の場合のみ `ApiError::Conflict` を返すヘルパー |
| `hash_refresh_token` | `fn hash_refresh_token(token: &str, pepper: &str) -> Result<String, ApiError>` | HMAC-SHA256 でリフレッシュトークンをハッシュ化する。pepper を鍵として使用し、hex エンコードした文字列を返す |
| `issue_refresh_token` | `async fn issue_refresh_token(state: &AppState, user_id: i64) -> Result<String, ApiError>` | リフレッシュトークンを生成して DB にハッシュ保存し、生トークン文字列を返す（ヘルパー関数） |
| `register` | `pub async fn register(State(state): State<AppState>, body: Json<RegisterUser>) -> Result<(StatusCode, Json<AuthResponse>), ApiError>` | 下記参照 |
| `login` | `pub async fn login(State(state): State<AppState>, body: Json<LoginUser>) -> Result<Json<AuthResponse>, ApiError>` | 下記参照 |
| `refresh` | `pub async fn refresh(State(state): State<AppState>, body: Json<RefreshRequest>) -> Result<Json<AuthResponse>, ApiError>` | 下記参照 |
| `logout` | `pub async fn logout(State(state): State<AppState>, claims: Claims, body: Json<LogoutRequest>) -> Result<StatusCode, ApiError>` | 下記参照 |

**`issue_refresh_token` の処理フロー**:
1. `Uuid::new_v4().to_string()` でリフレッシュトークン生成
2. 生成した生トークンをハッシュ化して `token_hash` を作る（DB にはハッシュのみ保存し、生トークンは保持しない）
3. `state.snowflake.lock().unwrap().generate()` で Snowflake ID 生成
4. `Utc::now() + Duration::days(state.refresh_token_expiry_days)` で有効期限を計算
5. `INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, $4)` で DB に保存
6. クライアント向けに返す値は生トークン（`refresh_token`）とする

> **ポイント**: `register`, `login`, `refresh` の3箇所で同じリフレッシュトークン発行処理を繰り返すため、ヘルパー関数に切り出して DRY にする。`pub` を付けない（ファイル内でのみ使用）。

**`register` の処理フロー**:
1. `validate_username`, `normalize_email` + `validate_email`, `validate_password` でバリデーション（email は正規化後に検証）
2. `bcrypt::hash(&body.password, DEFAULT_COST)` でパスワードをハッシュ化
3. `state.snowflake.lock().unwrap().generate()` で Snowflake ID 生成
4. `INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)` で DB に保存（`RETURNING` 不要 — レスポンスはトークンのみで、ID は生成済みの変数を使う）。UNIQUE 違反時は `map_unique_violation_to_conflict` で email 重複を `409 Conflict` に変換する
5. `create_token(id, &state.jwt_secret, &state.jwt_issuer, &state.jwt_audience, expiry_minutes)` でアクセストークン発行
6. `issue_refresh_token(&state, user_id)` でリフレッシュトークン発行
7. `(StatusCode::CREATED, Json(AuthResponse { access_token, refresh_token }))` を返す

**`login` の処理フロー**:
1. `normalize_email` で email を正規化
2. `SELECT id, username, email, password_hash FROM users WHERE email = $1` で DB 検索
3. **タイミング差対策**: ユーザーが見つからない場合でも `state.dummy_password_hash`（起動時に生成したダミー bcrypt ハッシュ）に対して `bcrypt::verify` を実行する。これにより email の存在有無でレスポンス時間に差が出ないようにする
4. `bcrypt::verify(&body.password, &password_hash)` でパスワード照合。不一致またはユーザー不在なら `ApiError::Unauthorized`
5. `DELETE FROM refresh_tokens WHERE user_id = $1` で既存リフレッシュトークンを全削除（古いセッションを無効化）
6. `create_token(user.id, &state.jwt_secret, &state.jwt_issuer, &state.jwt_audience, expiry_minutes)` でアクセストークン発行
7. `issue_refresh_token(&state, user.id)` でリフレッシュトークン発行
8. `Json(AuthResponse { access_token, refresh_token })` を返す

**`refresh` の処理フロー**:
1. リクエストから生トークン（`refresh_token`）を受け取る
2. 受け取った生トークンをハッシュ化して `token_hash` を作る
3. `SELECT id, user_id, token_hash, expires_at FROM refresh_tokens WHERE token_hash = $1` で DB 検索
4. 見つからなければ `ApiError::Unauthorized`
5. `expires_at < Utc::now()` なら期限切れ → 古いトークンを DELETE → `ApiError::Unauthorized`
6. 古いリフレッシュトークンを DELETE（ローテーション）
7. `create_token(user_id, &state.jwt_secret, &state.jwt_issuer, &state.jwt_audience, expiry_minutes)` で新しいアクセストークン発行
8. `issue_refresh_token(&state, user_id)` で新しいリフレッシュトークン発行
9. `Json(AuthResponse { access_token, refresh_token })` を返す

**`logout` の処理フロー**:
1. リクエストから生トークン（`refresh_token`）を受け取る
2. 受け取った生トークンをハッシュ化して `token_hash` を作る
3. `DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2` で DB から削除（`$2` は `claims.sub` をパースした `i64`）
4. 204 No Content を返す

> **セキュリティ上の注意**: ログイン失敗時に「メールが存在しない」「パスワードが違う」を区別せず、一律 `Unauthorized` を返す。これにより、攻撃者がメールアドレスの存在有無を推測できないようにする。

> **login での password_hash 取得について**: `User` 構造体には `#[serde(skip_serializing)]` を付けるが、login では `password_hash` が必要なので、`SELECT` で明示的に `password_hash` を含めて取得し、`query_as::<_, User>` でマッピングする。`#[sqlx(default)]` により、他のクエリでは `password_hash` を省略できる。

> **リフレッシュトークンのローテーション**: `refresh` エンドポイントでは、使用済みのリフレッシュトークンを削除して新しいものを発行する。これにより、リフレッシュトークンが漏洩した場合のリスクを軽減する。

---

### `routes/users.rs` — ユーザー CRUD ハンドラー

**役割**: 各エンドポイントの具体的な処理を実装する

**関数一覧**:

| 関数 | シグネチャ | 認証 | 説明 |
|------|-----------|------|------|
| `list_users` | `async fn(State, Query<UsersPagination>) -> Result<Json<Vec<PublicUser>>, ApiError>` | 不要 | `limit` / `after_id` をクエリから受け取る（未指定時は既定値）→ バリデーション → `SELECT id, username FROM users WHERE id > $after_id ORDER BY id LIMIT $limit` で取得して返す |
| `get_user` | `async fn(State, Path<i64>) -> Result<Json<PublicUser>, ApiError>` | 不要 | `SELECT id, username FROM users WHERE id = $1` で検索 → 見つかれば返す、なければ NotFound |
| `update_user` | `async fn(State, Path<i64>, Claims, Json<UpdateUser>) -> Result<Json<PublicUser>, ApiError>` | **必要 + 本人のみ** | `claims.sub` と Path の `user_id` を比較 → 不一致なら `ApiError::Forbidden` → バリデーション → email を `normalize_email` で正規化 → `UPDATE users SET username = COALESCE($1, username), email = COALESCE($2, email) WHERE id = $3 RETURNING id, username` で更新。UNIQUE 違反時は email 重複を `409 Conflict` に変換する |
| `delete_user` | `async fn(State, Path<i64>, Claims) -> Result<StatusCode, ApiError>` | **必要 + 本人のみ** | `claims.sub` と Path の `user_id` を比較 → 不一致なら `ApiError::Forbidden` → `DELETE FROM users WHERE id = $1` で削除 → 204 No Content、なければ NotFound |

**ページネーション構造体**:

```
UsersPagination (Deserialize)        — GET /users のクエリパラメータ
├── limit    : Option<i64>            ← 未指定時は 20、1〜100 の範囲
└── after_id : Option<i64>            ← 未指定時は先頭から、0 以上
```

**`list_users` の処理フロー（カーソル/キーセットページネーション）**:
1. クエリから `limit` / `after_id` を受け取る（例: `GET /users?limit=20&after_id=123`）
2. 未指定の値には既定値を適用する（例: `limit=20`、`after_id` 未指定なら先頭から）
3. `limit` の上限・`after_id` の下限を検証し、範囲外は `ApiError::BadRequest` を返す
4. `after_id` が指定されている場合は `SELECT id, username FROM users WHERE id > $1 ORDER BY id LIMIT $2` を実行する
5. `after_id` が未指定の場合は `SELECT id, username FROM users ORDER BY id LIMIT $1` を実行する
6. `Json<Vec<PublicUser>>` を返す（次ページはレスポンスの最後の `id` を `after_id` に指定する）

- `Claims` を引数に加えるだけで、Axum が自動的に `FromRequestParts` を呼び出して認証を実行する
- トークンが無い/無効な場合は `ApiError::Unauthorized` が自動的に返され、ハンドラーまで到達しない
- **認可チェック**: `claims.sub.parse::<i64>()` で得た user_id と Path の user_id を比較し、一致しない場合は `ApiError::Forbidden` を返す
- **最小限のデータ公開**: 公開エンドポイント（`list_users`, `get_user`）は `PublicUser` を使い `SELECT id, username` のみ取得する。`update_user` も `RETURNING id, username` で `PublicUser` を返す
- **email 重複チェック**: `update_user` でも `map_unique_violation_to_conflict` で UNIQUE 違反を `409 Conflict` に変換する（`register` と同じ挙動）

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
| `tower` | 0.5 | `ServiceBuilder` によるミドルウェア合成、`Layer` トレイト |
| `tower-http` | 0.6 (features: cors, trace, compression-gzip, timeout, normalize-path, request-id) | CORS、トレース、gzip 圧縮、タイムアウト、パス正規化、リクエストID付与 |
| `tower-helmet` | 0.3 | セキュリティヘッダー一括設定 (`HelmetLayer`) |
| `tracing` | 0.1 | 構造化ログ出力の API (`info!`, `warn!`, `error!` マクロ) |
| `tracing-subscriber` | 0.3 (features: env-filter) | tracing のログをターミナルに表示するフォーマッター。`EnvFilter` で `RUST_LOG` 環境変数によるログレベル制御に対応 |
| `jsonwebtoken` | 10 (features: aws_lc_rs) | JWT トークンの生成 (`encode`) と検証 (`decode`)。v10 では暗号バックエンドの feature 指定が必須 |
| `bcrypt` | 0.18 | パスワードのハッシュ化 (`hash`) と照合 (`verify`) |
| `chrono` | 0.4 (features: serde) | トークン有効期限の計算 (`Utc::now()` + Duration) |
| `uuid` | 1 (features: v4) | リフレッシュトークンの生成 (`Uuid::new_v4`) |
| `tower_governor` | 0.8 | 認証エンドポイントへの IP ベースレートリミット (`GovernorLayer`, `GovernorConfigBuilder`) |
| `sha2` | 0.10 | HMAC-SHA256 のハッシュアルゴリズム（リフレッシュトークンハッシュ用） |
| `hmac` | 0.12 | HMAC 計算（リフレッシュトークンの pepper 付きハッシュ） |
| `hex` | 0.4 | HMAC ダイジェストの hex エンコード |

---

## ログ方針

### ログレベル基準

| レベル | 用途 | 例 |
|-------|------|-----|
| `ERROR` | サーバー内部エラー。運用チームの即時対応が必要 | DB 接続エラー、`ApiError::Internal` 発生時 |
| `WARN` | 不正なリクエストや攻撃の兆候。監視対象だが即時対応は不要 | ログイン失敗、JWT 検証失敗、期限切れリフレッシュトークンの使用 |
| `INFO` | 正常な重要アクション。システムの動作確認やアクティビティ追跡に使用 | サーバー起動/停止、ユーザー登録/ログイン/ログアウト、トークンリフレッシュ、ユーザー更新/削除、リクエスト/レスポンス（TraceLayer） |
| `DEBUG` | 開発時のデバッグ情報。本番では通常無効 | 個別の DB クエリ結果、ミドルウェアの詳細動作 |

### リクエスト ID によるトレーサビリティ

`SetRequestIdLayer` が付与する `x-request-id`（UUID v4）を `TraceLayer` のスパンに記録する。これにより、ハンドラー内で `tracing::info!` 等を呼ぶと、自動的にリクエスト ID がログに含まれる。

```rust
// TraceLayer のカスタマイズ（main.rs）
TraceLayer::new_for_http()
    .make_span_with(|req: &Request<_>| {
        let request_id = req.headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-");
        tracing::info_span!(
            "request",
            method = %req.method(),
            uri = %req.uri(),
            request_id = %request_id,
        )
    })
```

**出力例**:
```
2026-03-06T12:00:00Z  INFO request{method=POST uri=/auth/login request_id=550e8400-...}: user_api::routes::auth: User logged in user_id=123456
```

- 障害発生時にリクエスト ID で関連ログを一括検索できる
- クライアントにも `x-request-id` レスポンスヘッダーで返されるため、ユーザーから問い合わせがあった際にログを追跡可能

### ログ出力箇所の一覧

| ファイル | レベル | メッセージ | フィールド | タイミング |
|---------|--------|-----------|-----------|-----------|
| `main.rs` | INFO | `Listening on 0.0.0.0:3000` | — | サーバー起動時 |
| `main.rs` | INFO | `Shutdown signal received, finishing in-flight requests...` | — | シャットダウンシグナル受信時 |
| `main.rs` | INFO | （TraceLayer 自動出力） | method, uri, request_id, status, latency | リクエスト/レスポンスごと |
| `routes/auth.rs` | INFO | `User registered` | user_id | 登録成功 |
| `routes/auth.rs` | WARN | `Login failed` | email | ログイン失敗（パスワード不一致 or ユーザー不在） |
| `routes/auth.rs` | INFO | `User logged in` | user_id | ログイン成功 |
| `routes/auth.rs` | WARN | `Expired refresh token used` | user_id | 期限切れリフレッシュトークンの使用 |
| `routes/auth.rs` | INFO | `Token refreshed` | user_id | トークンリフレッシュ成功 |
| `routes/auth.rs` | INFO | `User logged out` | user_id | ログアウト成功 |
| `routes/users.rs` | INFO | `User updated` | user_id | ユーザー情報更新成功 |
| `routes/users.rs` | INFO | `User deleted` | user_id | ユーザー削除成功 |
| `auth.rs` | WARN | `Token Validation failed: {err}` | — | JWT 検証失敗 |
| `errors.rs` | ERROR | `Internal error: {s}` | — | `ApiError::Internal` 生成時 |

### ログに含めてはいけない情報

- パスワード（平文・ハッシュともに）
- JWT トークン文字列（ログからトークンを窃取されるリスク）
- リフレッシュトークン（生トークン・ハッシュともに）
- `JWT_SECRET`、`REFRESH_TOKEN_PEPPER` 等のシークレット

---

## `sqlx` エラーハンドリング方針

ハンドラーで `sqlx` を呼び出す箇所では、すべて `.map_err(|err| ...)` で `ApiError` に変換する。変換ルールは以下の通り。

### 基本ルール — 内部エラー

```rust
.map_err(|err| ApiError::Internal(err.to_string()))?
```

- DB 接続エラー、タイムアウト、クエリ構文エラー等はすべて `ApiError::Internal` にする
- `Internal` はログに詳細を出力し、クライアントには `{"error": "An internal error occurred"}` のみ返す（エラー詳細を漏洩しない）

### 例外 — UNIQUE 違反の特別処理

`INSERT` / `UPDATE` で PostgreSQL の UNIQUE 制約違反（SQLSTATE `23505`）が発生した場合、email に関する制約のみ `409 Conflict` にマッピングする。

```rust
.map_err(|err| {
    if let Some(db_err) = err.as_database_error()
        && let Some(api_err) = map_unique_violation_to_conflict(
            db_err.code().as_deref(),
            db_err.constraint(),
        )
    {
        return api_err;
    }
    ApiError::Internal(err.to_string())
})?
```

**`map_unique_violation_to_conflict`** ヘルパー（`routes/auth.rs` と `routes/users.rs` にそれぞれ定義）:
- SQLSTATE が `23505`（unique_violation）でなければ `None`
- 制約名が `users_email_key` または `users_email_lower_key` なら `Some(ApiError::Conflict("email already exists"))` を返す
- それ以外の UNIQUE 違反は `None`（`Internal` にフォールバック）

### `fetch_optional` + `ok_or` パターン

レコードが見つからない場合の処理は `fetch_optional` → `ok_or` で行う。

```rust
let row: Option<T> = sqlx::query_as("...")
    .bind(...)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;  // DB エラー → Internal

let row = row.ok_or(ApiError::NotFound)?;    // 行なし → NotFound or Unauthorized
```

- `routes/users.rs` の `get_user`: `ok_or(ApiError::NotFound)`
- `routes/auth.rs` の `refresh`: `ok_or(ApiError::Unauthorized)`

### `rows_affected` による存在チェック

`DELETE` 等で対象が存在しなかった場合は `rows_affected() == 0` で判定する。

```rust
let result = sqlx::query("DELETE FROM users WHERE id = $1")
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

if result.rows_affected() == 0 {
    return Err(ApiError::NotFound);
}
```

### エラー変換の一覧

| sqlx のエラー種別 | 変換先 | 使用箇所 |
|------------------|--------|---------|
| DB 接続/クエリエラー全般 | `ApiError::Internal` | 全ハンドラー |
| UNIQUE 違反 (`23505`) + email 制約 | `ApiError::Conflict` | `register`, `update_user` |
| UNIQUE 違反 (`23505`) + その他の制約 | `ApiError::Internal` | — |
| `fetch_optional` → `None` | `ApiError::NotFound` or `Unauthorized` | `get_user`, `login`, `refresh` |
| `rows_affected() == 0` | `ApiError::NotFound` | `delete_user` |

---

## テスト

### テスト方針

- **ユニットテスト**: DB に依存しないロジックをファイル内の `#[cfg(test)] mod tests` で検証する
- **結合テスト**: `tests/` ディレクトリで実際の DB（Supabase）に接続し、API のエンドツーエンド動作を検証する
- DB 依存のユニットテスト（モック等）は行わない。DB 関連の検証は結合テストに集約する
- RGBC サイクル（Red → Green → Blue → Commit）を厳守する

### ユニットテスト

DB やネットワークに依存しない純粋なロジックのみ対象。各ファイル末尾の `#[cfg(test)] mod tests` ブロックに記述する。

| ファイル | テスト数 | 検証内容 |
|---------|---------|---------|
| `snowflake.rs` | 5 | ID の正値、一意性、単調増加、machine_id ビット分離、時刻巻き戻り時の panic |
| `validation.rs` | 13 | username/email/password の正常系・異常系・境界値、`normalize_email` の trim+lowercase |
| `errors.rs` | 6 | 全 `ApiError` バリアントのステータスコード・JSON ボディ検証（`Conflict` 含む） |
| `auth.rs` | 11 | トークン往復、不正シークレット、期限切れ、ゴミ入力、issuer/audience 不一致、`iat` 未来チェック（leeway 内/超過/i64 オーバーフロー） |
| `rate_limit.rs` | 5 | `peer`/`smart` モードの IP 抽出（ヘッダー有無・フォールバック・エラー） |
| `routes/auth.rs` | 2 | `hash_refresh_token` の pepper 依存性、`map_unique_violation_to_conflict` の制約判定 |

**実行**:
```bash
cargo test          # 全テスト実行（結合テスト含む、DB 接続が必要）
cargo test --lib    # ユニットテストのみ実行（DB 不要）
```

### 結合テスト (`tests/api.rs`)

実際の PostgreSQL（Supabase）に接続し、ルーターに HTTP リクエストを送信して API の動作を検証する。

**前提条件**:
- `.env` に `DATABASE_URL` をはじめとする全環境変数が設定されていること
- 環境変数が未設定の場合はテストを**失敗**させる（`panic!`）。CI で見逃さないようにするため、「スキップ」ではなく「失敗」とする

**テストインフラ**:

```
TestApp
├── router          : Router           — レートリミットなしの Router（テスト用）
├── db              : PgPool           — DB 接続プール（クリーンアップ用）
├── jwt_secret      : String           — トークン検証用
├── jwt_issuer      : String
├── jwt_audience    : String
└── jwt_leeway_seconds : u64
```

- `TestApp::new()` — `.env` から環境変数を読み込み、`AppState` と `Router` を構築する。レートリミットは適用しない（テストの安定性のため）
- `TestApp::request()` — `Router::oneshot()` で HTTP リクエストを送信し、`(StatusCode, Value)` を返す
- `TestApp::register()` / `login()` — よく使うリクエストのショートカット
- `TestApp::extract_user_id()` — アクセストークンから `user_id` を取り出す
- `TestApp::cleanup_user_by_email()` — テストユーザーを DB から直接削除する（`refresh_tokens` → `users` の順で CASCADE に頼らず明示的に削除）
- `unique_email()` — `test-{UUID}@example.com` 形式のユニーク email を生成し、テスト間の干渉を防ぐ

**テスト一覧**:

| カテゴリ | テスト名 | 検証内容 |
|---------|---------|---------|
| Auth | `auth_register_success` | 登録成功 → 201 + access_token + refresh_token |
| Auth | `auth_register_invalid_username` | 空 username → 400 |
| Auth | `auth_register_invalid_email` | 不正 email → 400 |
| Auth | `auth_register_invalid_password` | 短い password → 400 |
| Auth | `auth_register_duplicate_email` | email 重複 → 409 |
| Auth | `auth_login_success` | ログイン成功 → 200 + トークン |
| Auth | `auth_login_wrong_password` | パスワード不一致 → 401 |
| Auth | `auth_login_nonexistent_email` | 存在しない email → 401 |
| Auth | `auth_refresh_success` | リフレッシュ成功 → 200 + 新トークン（ローテーション確認） |
| Auth | `auth_refresh_invalid_token` | 無効トークン → 401 |
| Auth | `auth_logout_success` | ログアウト成功 → 204 |
| Auth | `auth_logout_without_auth` | 認証なし → 401 |
| Users | `users_list_success` | 一覧取得 → 200 + 配列 |
| Users | `users_list_with_pagination` | `limit=2` → 200 + 2件以下 |
| Users | `users_list_invalid_limit` | `limit=0` → 400 |
| Users | `users_get_success` | 詳細取得 → 200 + id/username（password_hash なし） |
| Users | `users_get_not_found` | 存在しない ID → 404 |
| Users | `users_update_own` | 本人更新 → 200 |
| Users | `users_update_other_user_forbidden` | 他人更新 → 403 |
| Users | `users_update_duplicate_email_returns_409` | email 重複更新 → 409 |
| Users | `users_update_without_auth` | 認証なし更新 → 401 |
| Users | `users_delete_own` | 本人削除 → 204 + 再取得 404 |
| Users | `users_delete_other_user_forbidden` | 他人削除 → 403 |
| Users | `users_delete_without_auth` | 認証なし削除 → 401 |

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

### Phase 4: セキュリティ強化（実装済み）

29. **リフレッシュトークンのハッシュ保存** — `HMAC-SHA256` + `REFRESH_TOKEN_PEPPER` でハッシュ化して DB に保存。`refresh` / `logout` はハッシュ照合で動作する
30. **email 正規化** — `normalize_email` (`trim` + `lowercase`) でアプリ側を統一し、DB 側は `unique(lower(email))` で case-insensitive な一意制約を適用
31. **login のタイミング差対策** — ユーザー不在時もダミー bcrypt ハッシュに対して `verify` を実行し、レスポンス時間の差を軽減
32. **JWT 検証の追加強化** — `iss` / `aud` 必須化、HS256 のみ許可、`iat` の未来チェック（leeway 超過で拒否）、`JWT_SECRET` の最小長チェック（32文字）
33. **レート制限 IP モード** — `RATE_LIMIT_IP_MODE` で `peer`（既定）/ `smart` を切替。`peer` はヘッダーを一切信頼しない
34. **UNIQUE 違反の 409 マッピング** — `register` / `update_user` で email の UNIQUE 違反を `409 Conflict` に変換
35. **リクエストID** — `SetRequestIdLayer` で全リクエストに UUID v4 の `x-request-id` を付与

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

# ユーザー一覧（認証不要、カーソルページネーション）
curl http://localhost:3000/users
# → 200 + [{"id":...,"username":"taro"}, ...]

curl "http://localhost:3000/users?limit=10&after_id=123"
# → 200 + [{"id":124,"username":"jiro"}, ...]

# ユーザー詳細（認証不要）
curl http://localhost:3000/users/123
# → 200 + {"id":123,"username":"taro"}

# 本人による更新（OK — claims.sub == 123）
curl -X PUT http://localhost:3000/users/123 \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer eyJ..." \
  -d '{"username":"taro_updated"}'
# → 200 + {"id":123,"username":"taro_updated"}

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
