use super::{Pool, TableRow, RECORDS_LIMIT_PER_PAGE};
use async_trait::async_trait;
use chrono::NaiveDate;
use database_tree::{Child, Database, Table};
use futures::TryStreamExt;
use sqlx::mysql::{MySqlColumn, MySqlPool as MPool, MySqlRow};
use sqlx::{Column as _, Row as _, TypeInfo as _};

pub struct MySqlPool {
    pool: MPool,
}

impl MySqlPool {
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            pool: MPool::connect(database_url).await?,
        })
    }
}

pub struct Constraint {
    name: String,
    column_name: String,
}

impl TableRow for Constraint {
    fn fields(&self) -> Vec<String> {
        vec!["name".to_string(), "column_name".to_string()]
    }

    fn columns(&self) -> Vec<String> {
        vec![self.name.to_string(), self.column_name.to_string()]
    }
}

pub struct Column {
    name: Option<String>,
    r#type: Option<String>,
    null: Option<String>,
    default: Option<String>,
    comment: Option<String>,
}

impl TableRow for Column {
    fn fields(&self) -> Vec<String> {
        vec![
            "name".to_string(),
            "type".to_string(),
            "null".to_string(),
            "default".to_string(),
            "comment".to_string(),
        ]
    }

    fn columns(&self) -> Vec<String> {
        vec![
            self.name
                .as_ref()
                .map_or(String::new(), |name| name.to_string()),
            self.r#type
                .as_ref()
                .map_or(String::new(), |r#type| r#type.to_string()),
            self.null
                .as_ref()
                .map_or(String::new(), |null| null.to_string()),
            self.default
                .as_ref()
                .map_or(String::new(), |default| default.to_string()),
            self.comment
                .as_ref()
                .map_or(String::new(), |comment| comment.to_string()),
        ]
    }
}

pub struct ForeignKey {
    name: Option<String>,
    column_name: Option<String>,
    ref_table: Option<String>,
    ref_column: Option<String>,
}

impl TableRow for ForeignKey {
    fn fields(&self) -> Vec<String> {
        vec![
            "name".to_string(),
            "column_name".to_string(),
            "ref_table".to_string(),
            "ref_column".to_string(),
        ]
    }

    fn columns(&self) -> Vec<String> {
        vec![
            self.name
                .as_ref()
                .map_or(String::new(), |name| name.to_string()),
            self.column_name
                .as_ref()
                .map_or(String::new(), |r#type| r#type.to_string()),
            self.ref_table
                .as_ref()
                .map_or(String::new(), |r#type| r#type.to_string()),
            self.ref_column
                .as_ref()
                .map_or(String::new(), |r#type| r#type.to_string()),
        ]
    }
}

#[async_trait]
impl Pool for MySqlPool {
    async fn get_databases(&self) -> anyhow::Result<Vec<Database>> {
        let databases = sqlx::query("SHOW DATABASES")
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|table| table.get(0))
            .collect::<Vec<String>>();
        let mut list = vec![];
        for db in databases {
            list.push(Database::new(
                db.clone(),
                self.get_tables(db.clone()).await?,
            ))
        }
        Ok(list)
    }

    async fn get_tables(&self, database: String) -> anyhow::Result<Vec<Child>> {
        let tables =
            sqlx::query_as::<_, Table>(format!("SHOW TABLE STATUS FROM `{}`", database).as_str())
                .fetch_all(&self.pool)
                .await?;
        Ok(tables.into_iter().map(|table| table.into()).collect())
    }

    async fn get_records(
        &self,
        database: &Database,
        table: &Table,
        page: u16,
        filter: Option<String>,
    ) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
        let query = if let Some(filter) = filter {
            format!(
                "SELECT * FROM `{database}`.`{table}` WHERE {filter} LIMIT {page}, {limit}",
                database = database.name,
                table = table.name,
                filter = filter,
                page = page,
                limit = RECORDS_LIMIT_PER_PAGE
            )
        } else {
            format!(
                "SELECT * FROM `{}`.`{}` limit {page}, {limit}",
                database.name,
                table.name,
                page = page,
                limit = RECORDS_LIMIT_PER_PAGE
            )
        };
        let mut rows = sqlx::query(query.as_str()).fetch(&self.pool);
        let mut headers = vec![];
        let mut records = vec![];
        while let Some(row) = rows.try_next().await? {
            headers = row
                .columns()
                .iter()
                .map(|column| column.name().to_string())
                .collect();
            let mut new_row = vec![];
            for column in row.columns() {
                new_row.push(convert_column_value_to_string(&row, column)?)
            }
            records.push(new_row)
        }
        Ok((headers, records))
    }

    async fn get_columns(
        &self,
        database: &Database,
        table: &Table,
    ) -> anyhow::Result<Vec<Box<dyn TableRow>>> {
        let query = format!(
            "SHOW FULL COLUMNS FROM `{}`.`{}`",
            database.name, table.name
        );
        let mut rows = sqlx::query(query.as_str()).fetch(&self.pool);
        let mut columns: Vec<Box<dyn TableRow>> = vec![];
        while let Some(row) = rows.try_next().await? {
            columns.push(Box::new(Column {
                name: row.try_get("Field")?,
                r#type: row.try_get("Type")?,
                null: row.try_get("Null")?,
                default: row.try_get("Default")?,
                comment: row.try_get("Comment")?,
            }))
        }
        Ok(columns)
    }

    async fn get_constraints(
        &self,
        database: &Database,
        table: &Table,
    ) -> anyhow::Result<Vec<Box<dyn TableRow>>> {
        let mut rows = sqlx::query(
            "
        SELECT
            COLUMN_NAME,
            CONSTRAINT_NAME
        FROM
            information_schema.KEY_COLUMN_USAGE
        WHERE
            REFERENCED_TABLE_SCHEMA IS NULL
            AND REFERENCED_TABLE_NAME IS NULL
            AND TABLE_SCHEMA = ?
            AND TABLE_NAME = ?
        ",
        )
        .bind(&database.name)
        .bind(&table.name)
        .fetch(&self.pool);
        let mut constraints: Vec<Box<dyn TableRow>> = vec![];
        while let Some(row) = rows.try_next().await? {
            constraints.push(Box::new(Constraint {
                name: row.try_get("CONSTRAINT_NAME")?,
                column_name: row.try_get("COLUMN_NAME")?,
            }))
        }
        Ok(constraints)
    }

    async fn get_foreign_keys(
        &self,
        database: &Database,
        table: &Table,
    ) -> anyhow::Result<Vec<Box<dyn TableRow>>> {
        let mut rows = sqlx::query(
            "
        SELECT
            TABLE_NAME,
            COLUMN_NAME,
            CONSTRAINT_NAME,
            REFERENCED_TABLE_SCHEMA,
            REFERENCED_TABLE_NAME,
            REFERENCED_COLUMN_NAME
        FROM
            INFORMATION_SCHEMA.KEY_COLUMN_USAGE
        WHERE
            REFERENCED_TABLE_SCHEMA IS NOT NULL
            AND REFERENCED_TABLE_NAME IS NOT NULL
            AND TABLE_SCHEMA = ?
            AND TABLE_NAME = ?
        ",
        )
        .bind(&database.name)
        .bind(&table.name)
        .fetch(&self.pool);
        let mut foreign_keys: Vec<Box<dyn TableRow>> = vec![];
        while let Some(row) = rows.try_next().await? {
            foreign_keys.push(Box::new(ForeignKey {
                name: row.try_get("CONSTRAINT_NAME")?,
                column_name: row.try_get("COLUMN_NAME")?,
                ref_table: row.try_get("REFERENCED_TABLE_NAME")?,
                ref_column: row.try_get("REFERENCED_COLUMN_NAME")?,
            }))
        }
        Ok(foreign_keys)
    }

    async fn close(&self) {
        self.pool.close().await;
    }
}

fn convert_column_value_to_string(row: &MySqlRow, column: &MySqlColumn) -> anyhow::Result<String> {
    let column_name = column.name();
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<String> = value;
        return Ok(value.unwrap_or_else(|| "NULL".to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<&str> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<i8> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<i32> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<i64> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<f32> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<rust_decimal::Decimal> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<u8> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<u16> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<u32> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<u64> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<NaiveDate> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<chrono::DateTime<chrono::Utc>> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    if let Ok(value) = row.try_get(column_name) {
        let value: Option<bool> = value;
        return Ok(value.map_or("NULL".to_string(), |v| v.to_string()));
    }
    Err(anyhow::anyhow!(
        "column type not implemented: `{}` {}",
        column_name,
        column.type_info().clone().name()
    ))
}