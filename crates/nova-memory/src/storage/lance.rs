use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    FixedSizeListArray, Float32Array, Int32Array, RecordBatch, RecordBatchIterator, StringArray,
    TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use lancedb::connection::Connection;
use lancedb::index::vector::IvfPqIndexBuilder;
use lancedb::index::Index;
use lancedb::Table;

use crate::error::{NovaError, Result};

const EMBEDDING_DIMENSIONS: i32 = 384;
const MEMORIES_TABLE: &str = "memories";

pub struct LanceStore {
    connection: Connection,
    memories_table: Option<Table>,
}

impl LanceStore {
    pub async fn connect(path: &Path) -> Result<Self> {
        let uri = path
            .to_str()
            .ok_or_else(|| NovaError::Storage("Invalid path encoding".to_string()))?;

        let connection = lancedb::connect(uri)
            .execute()
            .await
            .map_err(|e| NovaError::Storage(format!("Failed to connect to LanceDB: {}", e)))?;

        Ok(Self {
            connection,
            memories_table: None,
        })
    }

    fn memories_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    EMBEDDING_DIMENSIONS,
                ),
                false,
            ),
            Field::new("memory_type", DataType::Utf8, false),
            Field::new("weight", DataType::Float32, false),
            Field::new(
                "created_at",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new(
                "last_accessed",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("access_count", DataType::Int32, false),
            Field::new("conversation_id", DataType::Utf8, true),
            Field::new("source", DataType::Utf8, false),
            Field::new("tier", DataType::Utf8, false),
            Field::new("compression", DataType::Utf8, false),
        ]))
    }

    fn create_empty_batch(schema: Arc<Schema>) -> RecordBatch {
        let empty_strings: Vec<Option<&str>> = vec![];
        let empty_floats: Vec<f32> = vec![];
        let empty_timestamps: Vec<i64> = vec![];
        let empty_ints: Vec<i32> = vec![];
        let empty_embeddings: Vec<Option<Vec<Option<f32>>>> = vec![];

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(FixedSizeListArray::from_iter_primitive::<
                    arrow_array::types::Float32Type,
                    _,
                    _,
                >(empty_embeddings, EMBEDDING_DIMENSIONS)),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(Float32Array::from(empty_floats)),
                Arc::new(
                    TimestampMicrosecondArray::from(empty_timestamps.clone())
                        .with_timezone("UTC"),
                ),
                Arc::new(
                    TimestampMicrosecondArray::from(empty_timestamps).with_timezone("UTC"),
                ),
                Arc::new(Int32Array::from(empty_ints)),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings)),
            ],
        )
        .expect("Schema matches columns")
    }

    pub async fn create_memories_table(&mut self) -> Result<()> {
        let schema = Self::memories_schema();
        let batch = Self::create_empty_batch(schema.clone());
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);

        let table = self
            .connection
            .create_table(MEMORIES_TABLE, Box::new(batches))
            .execute()
            .await
            .map_err(|e| NovaError::Storage(format!("Failed to create memories table: {}", e)))?;

        self.memories_table = Some(table);
        Ok(())
    }

    pub async fn create_vector_index(&self) -> Result<()> {
        let table = self.memories_table.as_ref().ok_or_else(|| {
            NovaError::Storage("Memories table not initialized".to_string())
        })?;

        let row_count = table
            .count_rows(None)
            .await
            .map_err(|e| NovaError::Storage(format!("Failed to count rows: {}", e)))?;

        // IVF-PQ requires at least 256 rows for training
        if row_count < 256 {
            return Ok(());
        }

        let ivf_pq = IvfPqIndexBuilder::default()
            .num_partitions(256)
            .num_sub_vectors(16);

        table
            .create_index(&["embedding"], Index::IvfPq(ivf_pq))
            .execute()
            .await
            .map_err(|e| NovaError::Storage(format!("Failed to create vector index: {}", e)))?;

        Ok(())
    }

    pub async fn open_memories_table(&mut self) -> Result<()> {
        let table = self
            .connection
            .open_table(MEMORIES_TABLE)
            .execute()
            .await
            .map_err(|e| NovaError::Storage(format!("Failed to open memories table: {}", e)))?;

        self.memories_table = Some(table);
        Ok(())
    }

    pub async fn table_exists(&self, name: &str) -> Result<bool> {
        let names = self
            .connection
            .table_names()
            .execute()
            .await
            .map_err(|e| NovaError::Storage(format!("Failed to list tables: {}", e)))?;

        Ok(names.contains(&name.to_string()))
    }

    pub fn memories_table(&self) -> Option<&Table> {
        self.memories_table.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_and_create_table() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();

        assert!(!store.table_exists(MEMORIES_TABLE).await.unwrap());

        store.create_memories_table().await.unwrap();

        assert!(store.table_exists(MEMORIES_TABLE).await.unwrap());
        assert!(store.memories_table().is_some());
    }

    #[tokio::test]
    async fn test_open_existing_table() {
        let temp_dir = tempfile::tempdir().unwrap();

        {
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
        }

        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
        store.open_memories_table().await.unwrap();

        assert!(store.memories_table().is_some());
    }

    #[tokio::test]
    async fn test_schema_has_correct_fields() {
        let schema = LanceStore::memories_schema();

        assert_eq!(schema.fields().len(), 12);

        let field_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(field_names.contains(&"id"));
        assert!(field_names.contains(&"content"));
        assert!(field_names.contains(&"embedding"));
        assert!(field_names.contains(&"memory_type"));
        assert!(field_names.contains(&"weight"));
        assert!(field_names.contains(&"created_at"));
        assert!(field_names.contains(&"last_accessed"));
        assert!(field_names.contains(&"access_count"));
        assert!(field_names.contains(&"conversation_id"));
        assert!(field_names.contains(&"source"));
        assert!(field_names.contains(&"tier"));
        assert!(field_names.contains(&"compression"));
    }

    #[tokio::test]
    async fn test_embedding_field_dimensions() {
        let schema = LanceStore::memories_schema();
        let embedding_field = schema.field_with_name("embedding").unwrap();

        match embedding_field.data_type() {
            DataType::FixedSizeList(_, size) => {
                assert_eq!(*size, EMBEDDING_DIMENSIONS);
            }
            _ => panic!("Expected FixedSizeList type for embedding field"),
        }
    }
}
