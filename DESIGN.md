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
| `create_pool` | `pub async fn create_pool() -> Result<PgPool, sqlx::Error>` | `DATABASE_URL` 環境変数から接続文字列を取得し、`PgPoolOptions` で最大接続数 5 を指定して `PgPool` を作成して返す。接続失敗時はエラーを返す |

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
