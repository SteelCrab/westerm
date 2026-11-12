//! The tree, one level at a time: databases, schemas, then tables and views.
//! System databases and schemas are left out. Row counts are the database's
//! own estimates, so reading a level never counts rows.

use serde::Serialize;

use super::driver::{quote_ident, DbError, Pool};
use super::store::Engine;

/// What to list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Level {
    Databases,
    Schemas { db: String },
    Tables { db: String, schema: Option<String> },
}

impl Level {
    /// The level the page asks for with `db` and `schema` (both optional).
    pub fn of(engine: Engine, db: Option<&str>, schema: Option<&str>) -> Self {
        let db = db.filter(|db| !db.is_empty());
        let schema = schema.filter(|schema| !schema.is_empty());
        match (engine, db, schema) {
            (Engine::Sqlite, _, _) => Self::Tables { db: String::new(), schema: None },
            (_, None, _) => Self::Databases,
            (Engine::Mysql, Some(db), _) => Self::Tables { db: db.to_owned(), schema: None },
            (Engine::Postgres, Some(db), None) => Self::Schemas { db: db.to_owned() },
            (Engine::Postgres, Some(db), Some(schema)) => Self::Tables { db: db.to_owned(), schema: Some(schema.to_owned()) },
        }
    }

    /// The database whose pool reads this level ("" is the connection's default).
    pub fn pool_db(&self, engine: Engine) -> &str {
        match (engine, self) {
            (Engine::Postgres, Self::Schemas { db } | Self::Tables { db, .. }) => db,
            _ => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Database,
    Schema,
    Table,
    View,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    /// The database's estimate; none for views, SQLite, or tables never analyzed.
    pub rows: Option<i64>,
    /// The first SQL a table pane runs.
    pub preview: Option<String>,
}

/// `SELECT * FROM <schema>.<table> LIMIT 100`, quoted for the engine.
pub fn preview_sql(engine: Engine, schema: Option<&str>, table: &str) -> String {
    let table = quote_ident(engine, table);
    match schema {
        Some(schema) => format!("SELECT * FROM {}.{table} LIMIT 100", quote_ident(engine, schema)),
        None => format!("SELECT * FROM {table} LIMIT 100"),
    }
}

fn failed(error: sqlx::Error) -> DbError {
    DbError::Db(error.to_string().lines().next().unwrap_or_default().to_owned())
}

fn named(kind: NodeKind) -> impl Fn(String) -> Node {
    move |name| Node { name, kind, rows: None, preview: None }
}

fn table(engine: Engine, schema: Option<&str>, name: String, view: bool, rows: Option<i64>) -> Node {
    let preview = Some(preview_sql(engine, schema, &name));
    let (kind, rows) = if view { (NodeKind::View, None) } else { (NodeKind::Table, rows.filter(|rows| *rows >= 0)) };
    Node { name, kind, rows, preview }
}

const PG_DATABASES: &str = "SELECT datname::text FROM pg_database WHERE NOT datistemplate AND datallowconn ORDER BY datname";
const PG_SCHEMAS: &str = "SELECT nspname::text FROM pg_namespace \
    WHERE nspname NOT IN ('pg_catalog', 'information_schema') AND nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp\\_%' \
    ORDER BY nspname";
const PG_TABLES: &str = "SELECT c.relname::text, c.relkind IN ('v', 'm'), c.reltuples::float8 FROM pg_class c \
    JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'v', 'm', 'f') ORDER BY c.relname";
const MY_DATABASES: &str = "SELECT CAST(schema_name AS CHAR) FROM information_schema.schemata \
    WHERE schema_name NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') ORDER BY schema_name";
const MY_TABLES: &str = "SELECT CAST(table_name AS CHAR), table_type = 'VIEW', CAST(table_rows AS SIGNED) FROM information_schema.tables \
    WHERE table_schema = ? ORDER BY table_name";
const LITE_TABLES: &str = "SELECT name, type = 'view' FROM sqlite_master \
    WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name";

/// The nodes of `level`, by name. `only_db` limits a MySQL database list to the connection's own database.
pub async fn read_level(pool: &Pool, engine: Engine, level: &Level, only_db: Option<&str>) -> Result<Vec<Node>, DbError> {
    match (pool, level) {
        (Pool::Pg(pg), Level::Databases) => {
            let names: Vec<String> = sqlx::query_scalar(PG_DATABASES).fetch_all(pg).await.map_err(failed)?;
            Ok(names.into_iter().map(named(NodeKind::Database)).collect())
        }
        (Pool::Pg(pg), Level::Schemas { .. }) => {
            let names: Vec<String> = sqlx::query_scalar(PG_SCHEMAS).fetch_all(pg).await.map_err(failed)?;
            Ok(names.into_iter().map(named(NodeKind::Schema)).collect())
        }
        (Pool::Pg(pg), Level::Tables { schema, .. }) => {
            let schema = schema.as_deref().unwrap_or("public");
            let rows: Vec<(String, bool, f64)> = sqlx::query_as(PG_TABLES).bind(schema).fetch_all(pg).await.map_err(failed)?;
            Ok(rows.into_iter().map(|(name, view, rows)| table(engine, Some(schema), name, view, Some(rows as i64))).collect())
        }
        (Pool::My(my), Level::Databases) => {
            let names: Vec<String> = match only_db.filter(|db| !db.is_empty()) {
                Some(db) => vec![db.to_owned()],
                None => sqlx::query_scalar(MY_DATABASES).fetch_all(my).await.map_err(failed)?,
            };
            Ok(names.into_iter().map(named(NodeKind::Database)).collect())
        }
        (Pool::My(my), Level::Tables { db, .. }) => {
            let rows: Vec<(String, bool, Option<i64>)> = sqlx::query_as(MY_TABLES).bind(db).fetch_all(my).await.map_err(failed)?;
            Ok(rows.into_iter().map(|(name, view, rows)| table(engine, Some(db), name, view, rows)).collect())
        }
        (Pool::Lite(lite), _) => {
            let rows: Vec<(String, bool)> = sqlx::query_as(LITE_TABLES).fetch_all(lite).await.map_err(failed)?;
            Ok(rows.into_iter().map(|(name, view)| table(engine, None, name, view, None)).collect())
        }
        (_, level) => Err(DbError::Db(format!("Nothing to list at {level:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::plugins::db::driver::tests::{pool, sql};
    use crate::server::plugins::db::driver::{run, Limits};

    #[test]
    fn preview_sql_quotes_per_engine() {
        assert_eq!(preview_sql(Engine::Postgres, Some("public"), "orders"), r#"SELECT * FROM "public"."orders" LIMIT 100"#);
        assert_eq!(preview_sql(Engine::Mysql, Some("app"), "a`b"), "SELECT * FROM `app`.`a``b` LIMIT 100");
        assert_eq!(preview_sql(Engine::Sqlite, None, r#"we"ird"#), r#"SELECT * FROM "we""ird" LIMIT 100"#);
    }

    #[test]
    fn levels_follow_the_engine() {
        assert_eq!(Level::of(Engine::Sqlite, Some("x"), Some("y")), Level::Tables { db: String::new(), schema: None });
        assert_eq!(Level::of(Engine::Postgres, None, None), Level::Databases);
        assert_eq!(Level::of(Engine::Postgres, Some("app"), None), Level::Schemas { db: "app".into() });
        assert_eq!(Level::of(Engine::Postgres, Some("app"), Some("public")), Level::Tables { db: "app".into(), schema: Some("public".into()) });
        assert_eq!(Level::of(Engine::Mysql, None, None), Level::Databases);
        assert_eq!(Level::of(Engine::Mysql, Some("shop"), Some("ignored")), Level::Tables { db: "shop".into(), schema: None });
        assert_eq!(Level::Databases.pool_db(Engine::Postgres), "");
        assert_eq!(Level::Schemas { db: "app".into() }.pool_db(Engine::Postgres), "app");
        assert_eq!(Level::Tables { db: "shop".into(), schema: None }.pool_db(Engine::Mysql), "", "MySQL reads every database from one pool");
    }

    #[tokio::test]
    async fn sqlite_lists_tables_and_views_and_hides_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        for statement in ["CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT)", "INSERT INTO t DEFAULT VALUES", "CREATE VIEW v AS SELECT * FROM t", "CREATE TABLE a (x)"] {
            sql(&pool, statement).await.unwrap();
        }
        let nodes = read_level(&pool, Engine::Sqlite, &Level::Tables { db: String::new(), schema: None }, None).await.unwrap();
        let names: Vec<(&str, NodeKind)> = nodes.iter().map(|node| (node.name.as_str(), node.kind)).collect();
        assert_eq!(names, vec![("a", NodeKind::Table), ("t", NodeKind::Table), ("v", NodeKind::View)]);
        assert!(nodes.iter().all(|node| node.rows.is_none()));
        assert_eq!(nodes[1].preview.as_deref(), Some(r#"SELECT * FROM "t" LIMIT 100"#));
    }

    #[tokio::test]
    async fn preview_sql_runs_for_odd_table_names() {
        let dir = tempfile::tempdir().unwrap();
        let pool = pool(dir.path(), false).await;
        for statement in [r#"CREATE TABLE "we""ird name" (x)"#, r#"CREATE TABLE "주문" (x)"#, r#"CREATE TABLE "select" (x)"#] {
            sql(&pool, statement).await.unwrap();
        }
        let nodes = read_level(&pool, Engine::Sqlite, &Level::Tables { db: String::new(), schema: None }, None).await.unwrap();
        assert_eq!(nodes.len(), 3);
        for node in nodes {
            let preview = node.preview.unwrap();
            assert!(run(&pool, &preview, None, &Limits::DEFAULT, Default::default()).await.is_ok(), "{preview}");
        }
    }

    #[tokio::test]
    async fn pg_tree_hides_system_schemas_and_reads_row_estimates() {
        use crate::server::plugins::db::driver::{connect, tests::from_env, Endpoint};
        let Some((connection, password)) = from_env("WESTERM_TEST_PG_URL", Engine::Postgres) else { return };
        let pool = connect(&connection, "", Some(&password), &Endpoint::direct(&connection), &Limits::DEFAULT).await.unwrap();
        let databases = read_level(&pool, Engine::Postgres, &Level::Databases, None).await.unwrap();
        assert!(databases.iter().all(|node| !node.name.starts_with("template")));
        let schemas = read_level(&pool, Engine::Postgres, &Level::Schemas { db: String::new() }, None).await.unwrap();
        assert!(schemas.iter().any(|node| node.name == "public"));
        assert!(schemas.iter().all(|node| node.name != "pg_catalog" && node.name != "information_schema" && !node.name.starts_with("pg_toast")));
    }

    #[tokio::test]
    async fn mysql_tree_hides_system_databases() {
        use crate::server::plugins::db::driver::{connect, tests::from_env, Endpoint};
        let Some((connection, password)) = from_env("WESTERM_TEST_MYSQL_URL", Engine::Mysql) else { return };
        let pool = connect(&connection, "", Some(&password), &Endpoint::direct(&connection), &Limits::DEFAULT).await.unwrap();
        let databases = read_level(&pool, Engine::Mysql, &Level::Databases, None).await.unwrap();
        for hidden in ["mysql", "information_schema", "performance_schema", "sys"] {
            assert!(databases.iter().all(|node| node.name != hidden), "{hidden}");
        }
    }
}
